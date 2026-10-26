//! A running plugin process.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use basecraft_platform::fdpass::{self, Receiver};
use basecraft_protocol::plugin::{Invoke, InvokeResult, Message, PROTOCOL_VERSION, PluginInfo};
use serde_json::{Value, json};

use crate::{Error, Manifest, Result, Snapshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct PluginProcess {
    pub info: PluginInfo,
    /// Time from spawning to the `initialize` reply.
    pub startup: Duration,
    child: Child,
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
    std::env::var_os("BASECRAFT_PYTHONPATH").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"),
        PathBuf::from,
    )
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

    fn spawn_allocated(manifest: &Manifest, python: &Path, budget: usize) -> Result<Self> {
        let cpu_budget = budget.clamp(1, crate::compute::available());
        let started = Instant::now();
        let script = manifest.python_entry()?;
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
        let mut child = command
            .arg(&script)
            .env("BASECRAFT_SOCKET", &socket)
            .env("PYTHONPATH", sdk_path())
            .stdin(Stdio::null())
            .spawn()?;
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if child.try_wait()?.is_some() {
                        return Err(Error::Exited(manifest.id.clone()));
                    }
                    if started.elapsed() > CONNECT_TIMEOUT {
                        let _ = child.kill();
                        return Err(Error::Timeout(manifest.id.clone()));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(e.into()),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
        let mut p = Self {
            info: PluginInfo {
                protocol: 0,
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                operations: Vec::new(),
                capabilities: Vec::new(),
            },
            startup: Duration::ZERO,
            child,
            rx: Receiver::new(stream),
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
        p.rx.stream().set_read_timeout(None)?;
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
        line.push(b'\n');
        fdpass::send(self.rx.stream(), &line, fds)?;
        let reply = self
            .rx
            .line()?
            .ok_or_else(|| Error::Protocol("plugin closed the connection".into()))?;
        let raw: Value = serde_json::from_slice(&reply)?;
        let msg: Message = serde_json::from_value(raw.clone())?;
        if msg.jsonrpc != "2.0"
            || msg.method.is_some()
            || msg.params.is_some()
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
        let _ = self
            .rx
            .stream()
            .set_read_timeout(Some(Duration::from_millis(500)));
        let _ = self
            .rx
            .stream()
            .set_write_timeout(Some(Duration::from_millis(500)));
        let _ = self.call("shutdown", Value::Null, &[]);
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
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
