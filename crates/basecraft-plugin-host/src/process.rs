//! A running plugin process.

use basecraft_platform::process::OwnedProcess;
use std::io::Read;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use basecraft_platform::fdpass::{self, Receiver};
use basecraft_protocol::plugin::{Invoke, InvokeResult, Message, PROTOCOL_VERSION, PluginInfo};
use serde_json::{Value, json};

use crate::{Error, Manifest, Result, Snapshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct PluginProcess {
    pub info: PluginInfo,
    manifest: Manifest,
    /// Time from spawning to the `initialize` reply.
    pub startup: Duration,
    child: OwnedProcess,
    cancelled: Option<Arc<AtomicBool>>,
    cancel_path: Option<PathBuf>,
    deadline: Option<Instant>,
    capture: Arc<Mutex<CapturedOutput>>,
    capture_workers: Vec<std::thread::JoinHandle<()>>,
    stopped: bool,
    rx: Receiver,
    next_id: u64,
    cpu_budget: usize,
    _socket_dir: tempfile::TempDir,
}

/// The Python interpreter for plugins: `$BASECRAFT_PYTHON`, else the
/// project's pixi environment, else `python3` from `PATH`.
pub fn python_interpreter() -> PathBuf {
    if let Some(p) = std::env::var_os("BASECRAFT_PYTHON") {
        return PathBuf::from(p);
    }
    if let Some(python) = crate::installation::bundled_python() {
        return python;
    }
    let pixi_envs = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/pixi/envs"));
    if let Some(dir) = pixi_envs
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        for e in entries.flatten() {
            let name = e.file_name();
            if name.to_string_lossy().starts_with("basecraft-") {
                let py = e.path().join("envs/default/bin/python");
                if py.exists() {
                    return py;
                }
            }
        }
    }
    PathBuf::from("python3")
}

/// Where the `basecraft` Python SDK lives during development.
fn sdk_path() -> PathBuf {
    crate::installation::sdk()
}

impl PluginProcess {
    pub fn spawn(manifest: &Manifest) -> Result<Self> {
        Self::spawn_with(manifest, &python_interpreter())
    }

    pub fn spawn_with(manifest: &Manifest, python: &Path) -> Result<Self> {
        Self::spawn_allocated(manifest, python, crate::compute::effective_allocation()?)
    }

    pub fn spawn_with_allocation(manifest: &Manifest, budget: usize) -> Result<Self> {
        Self::spawn_allocated(manifest, &python_interpreter(), budget)
    }

    pub fn spawn_allocated(manifest: &Manifest, python: &Path, budget: usize) -> Result<Self> {
        crate::registry::require_enabled(&manifest.id)?;
        let cpu_budget = budget.clamp(1, crate::compute::available());
        let runtime_limit = std::env::var("BASECRAFT_PLUGIN_TIMEOUT_SECONDS")
            .ok()
            .map(|value| {
                let seconds: u64 = value.parse().map_err(|_| {
                    Error::Protocol(
                        "BASECRAFT_PLUGIN_TIMEOUT_SECONDS must be positive seconds".into(),
                    )
                })?;
                if seconds == 0 {
                    return Err(Error::Protocol(
                        "BASECRAFT_PLUGIN_TIMEOUT_SECONDS must be positive".into(),
                    ));
                }
                Ok(Duration::from_secs(seconds))
            })
            .transpose()?;
        let started = Instant::now();
        let script = manifest.python_entry()?.canonicalize()?;
        let dir = tempfile::Builder::new()
            .prefix("basecraft-plugin")
            .tempdir()?;
        let socket = dir.path().join("rpc.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let mut command = Command::new(python);
        for name in [
            "BASECRAFT_CPU_THREADS",
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
            "NUMEXPR_NUM_THREADS",
            "RAYON_NUM_THREADS",
        ] {
            command.env(name, cpu_budget.to_string());
        }
        command
            .arg("-B")
            .arg(&script)
            .env("BASECRAFT_SOCKET", &socket)
            .env("PYTHONPATH", sdk_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(manifest.dir.canonicalize()?);
        let mut child = OwnedProcess::spawn(&mut command)?;
        let capture = Arc::new(Mutex::new(CapturedOutput::default()));
        let mut capture_workers = Vec::new();
        if let Some(stdout) = child.take_stdout() {
            capture_workers.push(capture_output(stdout, capture.clone(), false)?);
        }
        if let Some(stderr) = child.take_stderr() {
            capture_workers.push(capture_output(stderr, capture.clone(), true)?);
        }
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if child.exited()? {
                        return Err(Error::Exited(manifest.id.clone()));
                    }
                    if started.elapsed() > CONNECT_TIMEOUT {
                        child.terminate();
                        return Err(Error::Timeout(manifest.id.clone()));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(e.into()),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        let mut p = Self {
            manifest: manifest.clone(),
            info: PluginInfo {
                protocol: 0,
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                operations: Vec::new(),
                capabilities: Vec::new(),
            },
            startup: Duration::ZERO,
            child,
            cancelled: None,
            cancel_path: None,
            deadline: Some(started + CONNECT_TIMEOUT),
            capture,
            capture_workers,
            stopped: false,
            rx: Receiver::with_limits(
                stream,
                basecraft_protocol::operation::MAX_FRAME_BYTES,
                basecraft_protocol::operation::MAX_FDS,
            ),
            next_id: 1,
            cpu_budget,
            _socket_dir: dir,
        };
        let (result, _) = p.call("initialize", json!({ "protocol": PROTOCOL_VERSION }), &[])?;
        p.info = serde_json::from_value(result)?;
        if p.info.protocol != PROTOCOL_VERSION {
            return Err(Error::Protocol(format!(
                "plugin speaks protocol {}",
                p.info.protocol
            )));
        }
        verify_handshake(manifest, &p.info)?;
        p.runtime_limit(runtime_limit);
        p.startup = started.elapsed();
        Ok(p)
    }

    /// One request and its response, with descriptors either way.
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        fds: &[BorrowedFd<'_>],
    ) -> Result<(Value, Vec<OwnedFd>)> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Message::request(id, method, params, fds.len()))?;
        if line.len() > basecraft_protocol::operation::MAX_FRAME_BYTES
            || fds.len() > basecraft_protocol::operation::MAX_FDS
        {
            return Err(Error::Protocol(
                "outgoing frame/descriptor limit exceeded".into(),
            ));
        }
        line.push(b'\n');
        fdpass::send(self.rx.stream(), &line, fds)?;
        let reply = loop {
            if self.cancel_path.as_ref().is_some_and(|path| path.exists()) {
                return Err(Error::Protocol("cancelled".into()));
            }
            if self
                .cancelled
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err(Error::Protocol("cancelled".into()));
            }
            if self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(Error::Timeout(self.info.id.clone()));
            }
            if self
                .capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .exceeded
            {
                return Err(Error::Protocol(
                    "plugin stdout/stderr quota exceeded".into(),
                ));
            }
            let cancelled = self.cancelled.clone();
            let deadline = self.deadline;
            let capture = self.capture.clone();
            let cancel_path = self.cancel_path.clone();
            match self.rx.line_checked(|| {
                if cancel_path.as_ref().is_some_and(|path| path.exists()) {
                    return Err(std::io::Error::other("cancelled"));
                }
                if cancelled
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Acquire))
                {
                    return Err(std::io::Error::other("cancelled"));
                }
                if deadline.is_some_and(|limit| Instant::now() >= limit) {
                    return Err(std::io::Error::other("plugin deadline exceeded"));
                }
                if capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .exceeded
                {
                    return Err(std::io::Error::other("plugin stdout/stderr quota exceeded"));
                }
                Ok(())
            }) {
                Ok(Some(reply)) => break reply,
                Ok(None) => return Err(Error::Protocol("plugin closed the connection".into())),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        };
        let raw: Value = serde_json::from_slice(&reply)?;
        let msg: Message = serde_json::from_value(raw.clone())?;
        if msg.jsonrpc != "2.0"
            || msg.method.is_some()
            || msg.params.is_some()
            || (raw.get("error").is_some() && msg.error.is_none())
            || (raw.get("result").is_some() == raw.get("error").is_some())
        {
            return Err(Error::Protocol("invalid JSON-RPC response shape".into()));
        }
        let received = self.rx.take_fds(msg.fds)?;
        if msg.id != Some(id) {
            return Err(Error::Protocol(format!(
                "reply to {:?}, expected {id}",
                msg.id
            )));
        }
        if let Some(e) = msg.error {
            return Err(Error::Remote {
                code: e.code,
                message: e.message,
            });
        }
        Ok((msg.result.unwrap_or(Value::Null), received))
    }

    pub fn ping(&mut self) -> Result<()> {
        self.call("ping", Value::Null, &[]).map(|_| ())
    }

    /// Run one of the plugin's operations, on a snapshot if given.
    pub fn invoke(
        &mut self,
        operation: &str,
        params: Value,
        snapshot: Option<&Snapshot>,
    ) -> Result<InvokeResult> {
        self.cancel_path = params
            .get("cancel_file")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        if let Some(contract) = self.manifest.operation_contract()? {
            let matching: Vec<_> = contract
                .operations
                .iter()
                .filter(|descriptor| descriptor.implementation == operation)
                .collect();
            if matching.len() > 1 {
                return Err(Error::Protocol(
                    "ambiguous legacy operation major; use its canonical registry ID".into(),
                ));
            }
            if let Some(descriptor) = matching.first() {
                let given = params.as_object().ok_or_else(|| {
                    Error::Protocol("operation parameters must be an object".into())
                })?;
                let properties = descriptor.parameters["properties"]
                    .as_object()
                    .ok_or_else(|| Error::Protocol("missing parameter properties".into()))?;
                let scientific: serde_json::Map<String, Value> = given
                    .iter()
                    .filter(|(key, _)| properties.contains_key(*key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                let scientific =
                    crate::contract::normalize_parameters(descriptor, &Value::Object(scientific))?;
                let scientific = crate::compute::parameters_for(
                    descriptor,
                    scientific
                        .as_object()
                        .cloned()
                        .ok_or_else(|| Error::Protocol("invalid normalized parameters".into()))?,
                    self.cpu_budget,
                );
                crate::contract::BoundedSchema::compile(&descriptor.parameters)?
                    .validate(&Value::Object(scientific.clone()))?;
                let controls: std::collections::BTreeMap<_, _> = given
                    .iter()
                    .filter(|(key, _)| !properties.contains_key(*key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                let context = basecraft_protocol::operation::InvocationContext {
                    schema: 1,
                    operation: crate::contract::operation_id(&self.manifest.id, descriptor)?,
                    run_id: given
                        .get("job_id")
                        .and_then(Value::as_str)
                        .map_or_else(|| ulid::Ulid::generate().to_string(), str::to_owned),
                    inputs: std::collections::BTreeMap::new(),
                    outputs: std::collections::BTreeMap::new(),
                    cpu_threads: self.cpu_budget,
                    cancel_file: given
                        .get("cancel_file")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    progress_file: given
                        .get("progress")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    input_evidence: std::collections::BTreeMap::new(),
                    legacy_parameters: controls,
                };
                return self.invoke_context(
                    &descriptor.implementation,
                    Value::Object(scientific),
                    context,
                    snapshot,
                );
            }
        }
        let params = match params {
            Value::Object(params) => Value::Object(crate::compute::parameters(
                &self.info.id,
                params,
                self.cpu_budget,
            )),
            value => value,
        };
        let req = Invoke {
            operation: operation.to_string(),
            params,
            snapshot: snapshot.map(Snapshot::describe),
            context: None,
        };
        let fds: Vec<BorrowedFd<'_>> = snapshot
            .map(|s| vec![s.residues.as_fd(), s.features.as_fd()])
            .unwrap_or_default();
        let (result, _) = self.call("invoke", serde_json::to_value(req)?, &fds)?;
        Ok(serde_json::from_value(result)?)
    }
    /// Contract calls keep scientific parameters separate from allocated controls.
    pub fn invoke_context(
        &mut self,
        implementation: &str,
        parameters: Value,
        context: basecraft_protocol::operation::InvocationContext,
        snapshot: Option<&Snapshot>,
    ) -> Result<InvokeResult> {
        if !self
            .info
            .capabilities
            .iter()
            .any(|c| c == basecraft_protocol::operation::CONTRACT_CAPABILITY)
        {
            return Err(Error::Protocol(
                "runtime lacks operation-contract-v1".into(),
            ));
        }
        if !self
            .info
            .operations
            .iter()
            .any(|name| name == implementation)
        {
            return Err(Error::Protocol(
                "runtime lacks declared implementation".into(),
            ));
        }
        let request = Invoke {
            operation: implementation.into(),
            params: parameters,
            snapshot: snapshot.map(Snapshot::describe),
            context: Some(context),
        };
        let fds: Vec<_> = snapshot
            .map(|s| vec![s.residues.as_fd(), s.features.as_fd()])
            .unwrap_or_default();
        let (result, received) = self.call("invoke", serde_json::to_value(request)?, &fds)?;
        if !received.is_empty() {
            return Err(Error::Protocol("unexpected result descriptors".into()));
        }
        Ok(serde_json::from_value(result)?)
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Default)]
struct CapturedOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exceeded: bool,
}
const OUTPUT_QUOTA: usize = 1024 * 1024;
fn capture_output(
    mut reader: impl Read + Send + 'static,
    captured: Arc<Mutex<CapturedOutput>>,
    stderr: bool,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("plugin-output".into())
        .spawn(move || {
            let mut chunk = [0_u8; 8192];
            loop {
                let size = match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(size) => size,
                };
                let mut capture = captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let target = if stderr {
                    &mut capture.stderr
                } else {
                    &mut capture.stdout
                };
                let remaining = OUTPUT_QUOTA.saturating_sub(target.len());
                target.extend_from_slice(&chunk[..size.min(remaining)]);
                if size > remaining {
                    capture.exceeded = true;
                }
            }
        })
}

impl PluginProcess {
    pub fn finish(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.deadline = Some(Instant::now() + Duration::from_millis(500));
        if !self
            .cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            let _ = self.call("shutdown", Value::Null, &[]);
        }
        self.child.terminate();
        self.stopped = true;
        let deadline = Instant::now() + Duration::from_millis(500);
        while self
            .capture_workers
            .iter()
            .any(|worker| !worker.is_finished())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut unfinished = false;
        for worker in self.capture_workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            } else {
                unfinished = true;
            }
        }
        if unfinished {
            return Err(Error::Protocol(
                "plugin retained open output streams after teardown".into(),
            ));
        }
        if self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exceeded
        {
            return Err(Error::Protocol(
                "plugin stdout/stderr quota exceeded".into(),
            ));
        }
        Ok(())
    }
    pub fn cancellation(&mut self, flag: Arc<AtomicBool>) {
        self.cancelled = Some(flag);
    }
    pub fn runtime_limit(&mut self, limit: Option<Duration>) {
        self.deadline = limit.map(|limit| Instant::now() + limit);
    }
    pub fn captured_output(&self) -> (String, String) {
        let capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            String::from_utf8_lossy(&capture.stdout).into_owned(),
            String::from_utf8_lossy(&capture.stderr).into_owned(),
        )
    }
}

/// Verify static package identity before any operation is invoked.
pub fn verify_handshake(manifest: &Manifest, info: &PluginInfo) -> Result<()> {
    if info.protocol != PROTOCOL_VERSION
        || info.id != manifest.id
        || info.version != manifest.version
    {
        return Err(Error::Protocol(
            "runtime identity/version does not match manifest".into(),
        ));
    }
    let unique: std::collections::BTreeSet<_> = info.operations.iter().collect();
    if unique.len() != info.operations.len() {
        return Err(Error::Protocol("duplicate runtime implementations".into()));
    }
    if let Some(contract) = manifest.operation_contract()? {
        if !info
            .capabilities
            .iter()
            .any(|c| c == basecraft_protocol::operation::CONTRACT_CAPABILITY)
        {
            return Err(Error::Protocol(
                "schema 2 requires runtime operation-contract-v1 capability".into(),
            ));
        }
        for descriptor in contract.operations {
            if !info.operations.contains(&descriptor.implementation) {
                return Err(Error::Protocol(format!(
                    "runtime missing implementation {}",
                    descriptor.implementation
                )));
            }
        }
    }
    Ok(())
}
