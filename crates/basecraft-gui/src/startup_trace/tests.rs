// Exercise the production recorder without creating Qt objects or reading clocks.
use super::{Event, PREFIX, Recorder};
use std::ffi::OsStr;
use std::io::{self, Write};

fn records(bytes: &[u8]) -> Vec<serde_json::Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| {
            assert!(line.len() < 512);
            serde_json::from_str(line.strip_prefix(PREFIX).unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn missing_token_is_silent() {
    let mut bytes = Vec::new();
    let mut recorder = Recorder::new(None, 42, &mut bytes);
    assert!(!recorder.enabled());
    recorder.record(Event::MainEntered, 0);
    recorder.record(Event::QmlCreationFailed, 1);
    assert_eq!(bytes, [] as [u8; 0]);
}

#[test]
fn invalid_tokens_get_one_safe_diagnostic() {
    for token in ["", "bad\nINJECT", "a b", "é", &"a".repeat(65)] {
        let mut bytes = Vec::new();
        let mut recorder = Recorder::new(Some(OsStr::new(token)), 42, &mut bytes);
        assert!(!recorder.enabled());
        recorder.record(Event::MainEntered, 0);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.starts_with("Basecraft startup trace disabled:"));
        assert!(!text.contains("INJECT"));
        assert!(!text.contains(PREFIX));
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_token_is_disabled() {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = Vec::new();
    let recorder = Recorder::new(Some(OsStr::from_bytes(b"\xff")), 1, &mut bytes);
    assert!(!recorder.enabled());
    assert_eq!(std::str::from_utf8(&bytes).unwrap().lines().count(), 1);
}

#[test]
fn bounded_versioned_records_and_duplicates() {
    let token = format!("Ab_9-{}", "x".repeat(59));
    let mut bytes = Vec::new();
    let mut recorder = Recorder::new(Some(OsStr::new(&token)), u32::MAX, &mut bytes);
    let events = [
        Event::MainEntered,
        Event::ApplicationCreated,
        Event::EngineCreated,
        Event::QmlRootCreated,
        Event::QmlLoadReturned,
    ];
    for (n, event) in events.into_iter().enumerate() {
        recorder.record(event, n as u64);
        recorder.record(event, n as u64);
    }
    let rows = records(&bytes);
    assert_eq!(rows.len(), 5);
    for (n, row) in rows.iter().enumerate() {
        assert_eq!(row["protocol"], "gui-startup-lifecycle-v1");
        assert_eq!(row["token"], token);
        assert_eq!(row["pid"], u32::MAX);
        assert_eq!(row["elapsed_ns"], n as u64);
        assert_eq!(row.as_object().unwrap().len(), 5);
    }
    assert_eq!(rows[0]["event"], "main_entered");
    assert_eq!(rows[1]["event"], "application_created");
    assert_eq!(rows[2]["event"], "engine_created");
    assert_eq!(rows[3]["event"], "qml_root_created");
    assert_eq!(rows[4]["event"], "qml_load_returned");
}

#[test]
fn root_and_load_return_can_arrive_in_either_order() {
    for events in [
        [Event::QmlRootCreated, Event::QmlLoadReturned],
        [Event::QmlLoadReturned, Event::QmlRootCreated],
    ] {
        let mut bytes = Vec::new();
        let mut recorder = Recorder::new(Some(OsStr::new("run")), 1, &mut bytes);
        recorder.record(Event::MainEntered, 0);
        recorder.record(events[0], 10);
        recorder.record(events[1], 9); // Rejected, not clamped or marked seen.
        recorder.record(events[1], 10); // Equal timestamps are valid.
        let rows = records(&bytes);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1]["elapsed_ns"], 10);
        assert_eq!(rows[2]["elapsed_ns"], 10);
        assert_ne!(rows[1]["event"], rows[2]["event"]);
    }
}

#[test]
fn initialization_failures_are_explicit_and_once_only() {
    for (event, name) in [
        (
            Event::ApplicationCreationFailed,
            "application_creation_failed",
        ),
        (Event::EngineCreationFailed, "engine_creation_failed"),
        (Event::QmlCreationFailed, "qml_creation_failed"),
    ] {
        let mut bytes = Vec::new();
        let mut recorder = Recorder::new(Some(OsStr::new("run")), 1, &mut bytes);
        recorder.record(event, u64::MAX);
        recorder.record(event, u64::MAX);
        let rows = records(&bytes);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["event"], name);
        assert_eq!(rows[0]["elapsed_ns"], u64::MAX);
    }
}

struct BrokenSink {
    calls: usize,
}
impl Write for BrokenSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failure_disables_further_writes_without_panicking() {
    let mut sink = BrokenSink { calls: 0 };
    let mut recorder = Recorder::new(Some(OsStr::new("run")), 1, &mut sink);
    recorder.record(Event::MainEntered, 0);
    assert!(!recorder.enabled());
    recorder.record(Event::ApplicationCreated, 1);
    assert_eq!(sink.calls, 1);
    let recorder = Recorder::new(Some(OsStr::new("\n")), 1, &mut sink);
    assert!(!recorder.enabled());
    assert_eq!(sink.calls, 2);
}
