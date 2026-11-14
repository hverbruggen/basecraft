//! A running plugin process.

#[cfg(windows)]
use crate::windows_transport::{BorrowedFd, OwnedFd, Receiver};
use basecraft_platform::process::OwnedProcess;
use basecraft_protocol::operation::OperationErrorCode as Code;
use std::io::Read;
#[cfg(windows)]
use std::net::TcpListener;
#[cfg(unix)]
use std::os::fd::{BorrowedFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[cfg(unix)]
use basecraft_platform::fdpass::{self, Receiver};
use basecraft_protocol::plugin::{Invoke, InvokeResult, Message, PROTOCOL_VERSION, PluginInfo};
use serde_json::{Value, json};

use crate::{Error, Manifest, Result, Snapshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct PluginProcess {
    pub info: PluginInfo,
    queries: Vec<basecraft_protocol::operation::QueryDescriptor>,
    /// Time from spawning to the `initialize` reply.
    pub startup: Duration,
    child: OwnedProcess,
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    capture: Arc<Mutex<CapturedOutput>>,
    capture_workers: Vec<std::thread::JoinHandle<()>>,
    stopped: bool,
    rx: Receiver,
    next_id: u64,
    credentials: crate::policy::RuntimeCredentials,
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
    if let Some(python) = crate::installation::source_python() {
        return python;
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

    /// Start a plugin directly, outside the runner, with no credentials.
    pub fn spawn_with(manifest: &Manifest, python: &Path) -> Result<Self> {
        crate::registry::require_enabled(&manifest.id)?;
        Self::spawn_with_credentials(
            manifest,
            python,
            crate::compute::effective_allocation()?,
            &crate::policy::RuntimeCredentials::default(),
        )
    }

    /// The runner's start, with the package and grants frozen at acceptance.
    /// Later disable, update or removal affects new submissions only.
    pub(crate) fn spawn_with_credentials(
        manifest: &Manifest,
        python: &Path,
        budget: usize,
        credentials: &crate::policy::RuntimeCredentials,
    ) -> Result<Self> {
        let python = if manifest.runtime.is_some() {
            let runtime = manifest.managed_runtime.as_ref().ok_or_else(|| {
                Error::Protocol(
                    "declared plugin runtime unavailable; install the plugin first".into(),
                )
            })?;
            runtime.verify()?;
            runtime.interpreter.as_path()
        } else {
            python
        };
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
        #[cfg(unix)]
        let socket = dir.path().join("rpc.sock");
        #[cfg(unix)]
        let listener = UnixListener::bind(&socket)?;
        #[cfg(windows)]
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        #[cfg(windows)]
        let token = format!("{}{}", ulid::Ulid::generate(), ulid::Ulid::generate());
        listener.set_nonblocking(true)?;
        let mut command = Command::new(python);
        crate::runtime::configure_command(&mut command, manifest, python)?;
        // Ambient credentials are input to host providers, never implicit grants
        // to child code, including during initialization.
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|name| {
                crate::policy::credential_environment(&name.to_ascii_uppercase())
            }) {
                command.env_remove(name);
            }
        }
        command.envs(&credentials.0);
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
            // Child tools share this process-owned scratch root. Host teardown
            // removes it even when cancellation kills Python before finally.
            .env("TMPDIR", dir.path())
            .env("TMP", dir.path())
            .env("TEMP", dir.path())
            .env("PYTHONPATH", sdk_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(manifest.dir.canonicalize()?);
        #[cfg(unix)]
        command
            .env("BASECRAFT_SOCKET", &socket)
            .env_remove("BASECRAFT_TCP")
            .env_remove("BASECRAFT_TOKEN");
        #[cfg(windows)]
        command
            .env("BASECRAFT_TCP", listener.local_addr()?.to_string())
            .env("BASECRAFT_TOKEN", &token)
            .env_remove("BASECRAFT_SOCKET");
        let mut child = OwnedProcess::spawn(&mut command).map_err(|e| {
            Error::Protocol(format!(
                "cannot start plugin {} with interpreter {}: {e}",
                manifest.id,
                python.display()
            ))
        })?;
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
                Ok((s, _)) => {
                    #[cfg(windows)]
                    let mut s = s;
                    #[cfg(windows)]
                    {
                        // Winsock accept inherits the listener's nonblocking mode.
                        // Switch before authentication, not just after it succeeds.
                        s.set_nonblocking(false)?;
                        // The new process can be descheduled between connect and
                        // sending its token. A polling interval is too short for
                        // this one-time authentication read under Windows load.
                        s.set_read_timeout(Some(Duration::from_secs(2)))?;
                        let mut supplied = vec![0; token.len()];
                        if s.read_exact(&mut supplied).is_err() || supplied != token.as_bytes() {
                            if started.elapsed() > CONNECT_TIMEOUT {
                                return Err(Error::Timeout(manifest.id.clone()));
                            }
                            continue;
                        }
                    }
                    break s;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if child.exited()? {
                        let status = child.finish()?;
                        let drain = Instant::now();
                        while capture_workers.iter().any(|worker| !worker.is_finished())
                            && drain.elapsed() < Duration::from_secs(1)
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        for worker in capture_workers {
                            if worker.is_finished() {
                                let _ = worker.join();
                            }
                        }
                        let output = capture
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        return Err(Error::Protocol(format!(
                            "plugin {} exited before connecting ({status}): {}",
                            manifest.id,
                            String::from_utf8_lossy(
                                &output.stderr[..output.stderr.len().min(8192)]
                            )
                        )));
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
            queries: manifest.operation_contract()?.queries,
            info: PluginInfo {
                protocol: 0,
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                operations: Vec::new(),
                interpreter: None,
            },
            startup: Duration::ZERO,
            child,
            cancelled: None,
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
            credentials: credentials.clone(),
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
        let message = Message::request(id, method, params, fds.len());
        #[cfg(unix)]
        let mut line = serde_json::to_vec(&message)?;
        #[cfg(windows)]
        let mut line = {
            let mut value = serde_json::to_value(&message)?;
            value["buffers"] = serde_json::to_value(fds)?;
            serde_json::to_vec(&value)?
        };
        if line.len() > basecraft_protocol::operation::MAX_FRAME_BYTES
            || fds.len() > basecraft_protocol::operation::MAX_FDS
        {
            return Err(Error::Protocol(
                "outgoing frame/descriptor limit exceeded".into(),
            ));
        }
        line.push(b'\n');
        #[cfg(unix)]
        fdpass::send(self.rx.stream(), &line, fds)?;
        #[cfg(windows)]
        {
            use std::io::Write;
            self.rx.stream().write_all(&line)?;
        }
        let reply = loop {
            if self
                .cancelled
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err(Error::refused(Code::Cancelled, String::new()));
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
            match self.rx.line_checked(|| {
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
        let mut raw: Value = serde_json::from_slice(&reply)?;
        self.credentials.redact_json(&mut raw);
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

    /// Run one of the plugin's operations in the run the host allocated, on a
    /// snapshot if given.
    pub fn invoke(
        &mut self,
        implementation: &str,
        parameters: Value,
        context: basecraft_protocol::operation::InvocationContext,
        snapshot: Option<&Snapshot>,
    ) -> Result<InvokeResult> {
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
            context,
        };
        let fds: Vec<_> = snapshot
            .map(|s| vec![s.residues.transport_handle(), s.features.transport_handle()])
            .unwrap_or_default();
        let (result, received) = self.call("invoke", serde_json::to_value(request)?, &fds)?;
        if !received.is_empty() {
            return Err(Error::Protocol("unexpected result descriptors".into()));
        }
        Ok(serde_json::from_value(result)?)
    }

    /// Ask one of the questions the contract declares, such as which engines
    /// are installed. It takes no inputs and allocates nothing; the answer is
    /// checked against the declared result schema.
    pub fn query(&mut self, name: &str) -> Result<Value> {
        let schema = self
            .queries
            .iter()
            .find(|query| query.name == name)
            .map(|query| query.result.clone())
            .ok_or_else(|| Error::Protocol(format!("the plugin declares no query {name}")))?;
        let query = basecraft_protocol::plugin::Query { name: name.into() };
        let answer = self.call("query", serde_json::to_value(query)?, &[])?.0;
        crate::contract::BoundedSchema::compile(&schema)?.validate(&answer)?;
        Ok(answer)
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Default)]
pub(crate) struct CapturedOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) exceeded: bool,
}
const OUTPUT_QUOTA: usize = 1024 * 1024;
pub(crate) fn capture_output(
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
            self.credentials
                .redact(&String::from_utf8_lossy(&capture.stdout)),
            self.credentials
                .redact(&String::from_utf8_lossy(&capture.stderr)),
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
    for descriptor in manifest.operation_contract()?.operations {
        if !info.operations.contains(&descriptor.implementation) {
            return Err(Error::Protocol(format!(
                "runtime missing implementation {}",
                descriptor.implementation
            )));
        }
    }
    Ok(())
}
