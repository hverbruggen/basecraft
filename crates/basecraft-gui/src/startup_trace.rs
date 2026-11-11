//! Opt-in startup lifecycle and first swapped frame diagnostics.
//!
//! Set `BASECRAFT_STARTUP_TRACE_TOKEN` to 1–64 ASCII letters, digits, `_` or `-`.
//! Each event is attempted once; an output error disables the recorder. There are
//! at most nine records, each under 512 bytes. No files or timers are created.

use serde::Serialize;
use std::ffi::OsStr;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const PREFIX: &str = "basecraft-gui-startup-v1 ";
pub const MAIN_QML_URL: &str = "qrc:/qt/qml/Basecraft/qml/Main.qml";
const INVALID_TOKEN: &[u8] = b"Basecraft startup trace disabled: token must contain 1-64 ASCII letters, digits, underscores or hyphens.\n";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    MainEntered,
    ApplicationCreated,
    EngineCreated,
    QmlRootCreated,
    QmlLoadReturned,
    FirstFrame,
    ApplicationCreationFailed,
    EngineCreationFailed,
    QmlCreationFailed,
}

#[derive(Serialize)]
struct Record<'a> {
    protocol: &'static str,
    token: &'a str,
    pid: u32,
    event: Event,
    elapsed_ns: u64,
}

pub struct Recorder<W> {
    sink: W,
    token: Option<String>,
    pid: u32,
    seen: u16,
    last_ns: u64,
}

impl<W: Write> Recorder<W> {
    pub fn new(token: Option<&OsStr>, pid: u32, mut sink: W) -> Self {
        let valid = token.and_then(OsStr::to_str).filter(|s| {
            (1..=64).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        });
        if token.is_some() && valid.is_none() {
            // Never echo untrusted environment contents or panic on a closed pipe.
            let _ = sink.write_all(INVALID_TOKEN);
        }
        Self {
            sink,
            token: valid.map(str::to_owned),
            pid,
            seen: 0,
            last_ns: 0,
        }
    }

    pub fn enabled(&self) -> bool {
        self.token.is_some()
    }

    pub fn record(&mut self, event: Event, elapsed_ns: u64) {
        let Some(token) = &self.token else { return };
        let bit = 1 << event as u16;
        if self.seen & bit != 0 || elapsed_ns < self.last_ns {
            return;
        }
        self.seen |= bit;
        self.last_ns = elapsed_ns;
        let record = Record {
            protocol: "gui-startup-lifecycle-v1",
            token,
            pid: self.pid,
            event,
            elapsed_ns,
        };
        // Serialize before writing so a serialization failure cannot leave a prefix.
        let result = serde_json::to_vec(&record).is_ok_and(|json| {
            let mut line = Vec::with_capacity(PREFIX.len() + json.len() + 1);
            line.extend_from_slice(PREFIX.as_bytes());
            line.extend_from_slice(&json);
            line.push(b'\n');
            self.sink.write_all(&line).is_ok()
        });
        if !result {
            self.token = None;
        }
    }
}

/// The disabled path allocates no shared recorder and installs no trace signals.
/// Main still captures one Instant and reads the opt-in environment variable.
#[derive(Clone)]
pub struct Trace {
    start: Instant,
    recorder: Option<Arc<Mutex<Recorder<std::io::Stderr>>>>,
}

impl Trace {
    pub fn from_env(start: Instant) -> Self {
        let token = std::env::var_os("BASECRAFT_STARTUP_TRACE_TOKEN").or_else(|| {
            std::env::args()
                .any(|arg| arg == "--startup-trace")
                .then(|| "startup".into())
        });
        let mut recorder = Recorder::new(token.as_deref(), std::process::id(), std::io::stderr());
        recorder.record(Event::MainEntered, 0);
        let recorder = recorder.enabled().then(|| Arc::new(Mutex::new(recorder)));
        Self { start, recorder }
    }

    pub fn enabled(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn record(&self, event: Event) {
        if let Some(recorder) = &self.recorder {
            // Read the clock under the lock so render-thread events cannot lose
            // their first record to a later timestamp from the interface thread.
            if let Ok(mut recorder) = recorder.lock() {
                let elapsed_ns = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
                recorder.record(event, elapsed_ns);
            }
        }
    }
}

#[cfg(test)]
#[path = "startup_trace/tests.rs"]
mod tests;

#[path = "startup_frame_bridge.rs"]
pub mod frame_bridge;
