//! Asynchronous, read-only inspection and explicit provenance exports.
use basecraft_ops::records::ReportExt;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::{path::Path, pin::Pin};

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for AppProvenance {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        #[qproperty(QString, message)]
        #[qproperty(QString, text)]
        #[qproperty(QString, notifications)]
        type AppProvenance = super::ProvenanceRust;
        #[qinvokable]
        fn relink_run(self: Pin<&mut AppProvenance>, run: &QString, original: &QString) -> bool;
        #[qinvokable]
        fn relink_input(
            self: Pin<&mut AppProvenance>,
            result: &QString,
            original: &QString,
        ) -> bool;
        #[qinvokable]
        fn cancel_run(self: Pin<&mut AppProvenance>, id: &QString);
        #[qinvokable]
        fn saved_report(self: &AppProvenance, path: &QString) -> QString;
        #[qinvokable]
        fn save_authored(
            self: Pin<&mut AppProvenance>,
            path: &QString,
            text: &QString,
            revision: &QString,
        ) -> bool;
        #[qinvokable]
        fn export_report(self: Pin<&mut AppProvenance>, snapshot: &QString, directory: &QString);
        #[qinvokable]
        fn notification(
            self: Pin<&mut AppProvenance>,
            document: &QString,
            message: &QString,
            kind: &QString,
        );
        #[qinvokable]
        fn clear_notifications(self: Pin<&mut AppProvenance>);
        #[qinvokable]
        fn jobs(self: &AppProvenance, previous: bool) -> QString;
        #[qinvokable]
        fn start(
            self: Pin<&mut AppProvenance>,
            operation: &QString,
            result: &QString,
            destination: &QString,
        );
        #[qinvokable]
        fn bundle_path(self: Pin<&mut AppProvenance>, result: &QString) -> QString;
        #[qinvokable]
        fn recovery_sessions(self: Pin<&mut AppProvenance>) -> QString;
    }
}
#[derive(Default)]
pub struct ProvenanceRust {
    busy: bool,
    message: QString,
    text: QString,
    notifications: QString,
    notices: Vec<serde_json::Value>,
}
fn request_task_cancellation(id: &str) -> String {
    match basecraft_plugin_host::file_tool::cancel_run(id) {
        Ok(()) => "notify: Cancellation requested".into(),
        Err(error) => format!("fail: {error}"),
    }
}

// The capability comes from the live owner, never from a retained receipt.
fn task_capabilities(data: &mut serde_json::Value, previous: bool) {
    if let Some(jobs) = data["jobs"].as_array_mut() {
        for job in jobs {
            job["cancellable"] = serde_json::json!(
                !previous
                    && job["id"]
                        .as_str()
                        .is_some_and(basecraft_plugin_host::file_tool::can_cancel_run)
            );
        }
    }
}

fn run(operation: &str, result: &Path, destination: &Path) -> Result<String, String> {
    use basecraft_ops::records;
    let outcome = (|| -> basecraft_ops::Result<String> {
        match operation {
            "recover" => {
                let outputs = basecraft_ops::records::recover(result, destination)?;
                Ok(format!(
                    "Recovered copies:\n{}\nOriginal files and recovery records are preserved.",
                    outputs
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            "history" => {
                let publication = records::inspect(result)?;
                Ok(serde_json::to_string_pretty(&publication)?)
            }
            "verify" => {
                let issues = records::verify(result)?;
                if issues.is_empty() {
                    Ok("Recorded artifact hashes match. Replay verification has not been performed.".into())
                } else {
                    Err(basecraft_ops::Error::Invalid(format!(
                        "Verification issues:\n{}",
                        issues.join("\n")
                    )))
                }
            }
            "methods" => {
                let publication = records::inspect(result)?;
                records::write_reports(destination, &publication)?;
                Ok(format!(
                    "Generated reports: {}\nUser-authored methods.md is preserved.",
                    destination.join("reports").display()
                ))
            }
            "package" => {
                records::package(result, destination)?;
                Ok(format!(
                    "Package saved: {}\nThis contains retained data and recorded history; external-tool replay has not been verified.",
                    destination.display()
                ))
            }
            _ => Err(basecraft_ops::Error::Invalid(
                "unsupported provenance action".into(),
            )),
        }
    })();
    outcome.map_err(|e| e.to_string())
}
impl qobject::AppProvenance {
    fn relink_run(self: Pin<&mut Self>, run: &QString, original: &QString) -> bool {
        let result = basecraft_ops::records::relink(
            Path::new(&run.to_string()),
            Path::new(&original.to_string()),
        );
        let success = result.is_ok();
        self.set_message(QString::from(
            match result {
                Ok(()) => "done: Verified original relinked".into(),
                Err(e) => format!("fail: {e}"),
            }
            .as_str(),
        ));
        success
    }
    fn relink_input(self: Pin<&mut Self>, result: &QString, original: &QString) -> bool {
        let result = (|| -> basecraft_ops::Result<()> {
            let result = std::path::PathBuf::from(result.to_string());
            basecraft_ops::records::inspect(&result)?;
            let run = basecraft_ops::records::bundle_for_result(&result);
            basecraft_ops::records::relink(&run, Path::new(&original.to_string()))
        })();
        let success = result.is_ok();
        self.set_message(QString::from(
            match result {
                Ok(()) => "done: Verified original relinked".into(),
                Err(e) => format!("fail: {e}"),
            }
            .as_str(),
        ));
        success
    }
    fn cancel_run(self: Pin<&mut Self>, id: &QString) {
        self.set_message(QString::from(&request_task_cancellation(&id.to_string())));
    }
    fn saved_report(&self, path: &QString) -> QString {
        report_json(
            basecraft_ops::records::inspect(Path::new(&path.to_string()))
                .and_then(|p| basecraft_ops::records::DocumentRecord::from_publication(&p).report())
                .and_then(|r| serde_json::to_string(&r).map_err(Into::into))
                .map_err(|e| e.to_string()),
        )
    }

    fn save_authored(
        self: Pin<&mut Self>,
        path: &QString,
        text: &QString,
        revision: &QString,
    ) -> bool {
        let contents = format!("<!-- Basecraft source revision: {revision} -->\n\n{text}");
        let result = basecraft_io::write_atomic(Path::new(&path.to_string()), |w| {
            w.write_all(contents.as_bytes())
        });
        let success = result.is_ok();
        self.set_message(QString::from(
            match result {
                Ok(()) => "done: Authored draft saved".into(),
                Err(e) => format!("fail: Could not save authored draft: {e}"),
            }
            .as_str(),
        ));
        success
    }
    fn export_report(self: Pin<&mut Self>, snapshot: &QString, directory: &QString) {
        let snapshot = snapshot.to_string();
        let directory = std::path::PathBuf::from(directory.to_string());
        self.start_work(move || {
            let report: basecraft_ops::records::Report =
                serde_json::from_str(&snapshot).map_err(|e| e.to_string())?;
            report
                .export(&directory)
                .map_err(|e| format!("Report export incomplete: {e}"))?;
            Ok("Report exported for the selected revision".into())
        });
    }
    fn notification(
        mut self: Pin<&mut Self>,
        document: &QString,
        message: &QString,
        kind: &QString,
    ) {
        let message = message.to_string();
        if message.is_empty() {
            return;
        }
        let notice = serde_json::json!({"document":document.to_string(), "message":message, "kind":match kind.to_string().as_str() { "ok" | "done" => "done", "error" | "fail" => "fail", _ => "notify" }, "time":basecraft_platform::now_rfc3339()});
        let mut state = self.as_mut().rust_mut();
        state.notices.insert(0, notice);
        state.notices.truncate(200);
        let text = serde_json::Value::Array(state.notices.clone()).to_string();
        self.set_notifications(QString::from(text.as_str()));
    }
    fn clear_notifications(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().notices.clear();
        self.set_notifications(QString::from("[]"));
    }
    fn jobs(&self, previous: bool) -> QString {
        report_json(
            basecraft_ops::records::jobs(previous)
                .map(|mut data| {
                    task_capabilities(&mut data, previous);
                    data.to_string()
                })
                .map_err(|e| e.to_string()),
        )
    }
    fn recovery_sessions(mut self: Pin<&mut Self>) -> QString {
        let result = (|| -> Result<String, String> {
            let root = basecraft_platform::paths::AppPaths::discover()
                .map_err(|e| e.to_string())?
                .state
                .join("recovery");
            let entries = match std::fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok("[]".into());
                }
                Err(error) => return Err(error.to_string()),
            };
            let mut sessions = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    continue;
                }
                if !basecraft_ops::log::log_dir(&entry.path()).is_dir() {
                    continue;
                }
                if sessions.len() >= 256 {
                    return Err("More than 256 recovery sessions; inspect the recovery directory with the CLI.".into());
                }
                let name = basecraft_ops::log::read_all(&entry.path())
                    .ok()
                    .and_then(|entries| entries.first().map(|entry| entry.doc.clone()))
                    .unwrap_or_else(|| "Damaged journal; recovery will validate it".into());
                let path = entry.path();
                let path = path.to_str().ok_or("Recovery path cannot be represented in this GUI; use the CLI with the native path")?;
                sessions.push(serde_json::json!({"path": path, "id": entry.file_name().to_string_lossy(), "name": name}));
            }
            sessions.sort_by_key(|value| value["id"].as_str().unwrap_or_default().to_owned());
            serde_json::to_string(&sessions).map_err(|e| e.to_string())
        })();
        match result {
            Ok(text) => {
                self.as_mut().set_message(QString::default());
                QString::from(text.as_str())
            }
            Err(error) => {
                self.as_mut().set_text(QString::default());
                self.set_message(QString::from(
                    format!("fail: Recovery sessions could not be listed: {error}").as_str(),
                ));
                QString::from("[]")
            }
        }
    }

    fn bundle_path(mut self: Pin<&mut Self>, result: &QString) -> QString {
        let path = std::path::PathBuf::from(result.to_string());
        match basecraft_ops::records::inspect(&path) {
            Ok(_) => QString::from(
                basecraft_ops::records::bundle_for_result(&path)
                    .to_string_lossy()
                    .as_ref(),
            ),
            Err(error) => {
                self.as_mut().set_text(QString::default());
                self.set_message(QString::from(format!("fail: {error}").as_str()));
                QString::default()
            }
        }
    }
    fn start(self: Pin<&mut Self>, operation: &QString, result: &QString, destination: &QString) {
        let operation = operation.to_string();
        let result = std::path::PathBuf::from(result.to_string());
        let destination = std::path::PathBuf::from(destination.to_string());
        self.start_work(move || run(&operation, &result, &destination));
    }
    fn start_work(
        mut self: Pin<&mut Self>,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if *self.busy() {
            return;
        }
        self.as_mut().set_busy(true);
        self.as_mut().set_text(QString::default());
        self.as_mut().set_message(QString::from("notify: Working…"));
        let spawn = crate::jobs::start(
            self.qt_thread(),
            move |_| work(),
            |mut state, result| {
                state.as_mut().set_busy(false);
                match result {
                    Ok(mut text) => {
                        const LIMIT: usize = 1024 * 1024;
                        if text.len() > LIMIT {
                            let mut end = LIMIT;
                            while !text.is_char_boundary(end) {
                                end -= 1;
                            }
                            text.truncate(end);
                            text.push_str("\nDisplay truncated. Inspect the readable provenance files for the full record.");
                        }
                        state.as_mut().set_text(QString::from(text.as_str()));
                        state.set_message(QString::from("done"));
                    }
                    Err(error) => {
                        state.set_message(QString::from(format!("fail: {error}").as_str()));
                    }
                }
            },
        );
        if let Err(error) = spawn {
            self.as_mut().set_busy(false);
            self.set_message(QString::from(
                format!("fail: Could not start provenance task: {error}").as_str(),
            ));
        }
    }
}

pub(crate) fn session_report(
    session: Option<&basecraft_ops::Session>,
    doc: Option<basecraft_ops::DocId>,
) -> QString {
    let result = match (session, doc) {
        (Some(session), Some(doc)) => session
            .document_record(doc)
            .and_then(|r| r.report())
            .and_then(|r| serde_json::to_string(&r).map_err(Into::into)),
        _ => return QString::from("{\"error\":\"No document is open\"}"),
    };
    report_json(result.map_err(|e| e.to_string()))
}

pub(crate) fn report_json(result: Result<String, String>) -> QString {
    QString::from(
        match result {
            Ok(text) => text,
            Err(error) => serde_json::json!({"error": error}).to_string(),
        }
        .as_str(),
    )
}

pub(crate) fn session_revision(
    session: Option<&basecraft_ops::Session>,
    doc: Option<basecraft_ops::DocId>,
) -> QString {
    let value = session
        .zip(doc)
        .and_then(|(s, d)| s.record_revision(d).ok())
        .unwrap_or_default();
    QString::from(value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_bridge_cancels_live_registered_run_and_refuses_retained_ids() {
        use basecraft_plugin_host::{
            jobs::JobManager,
            policy::SessionPolicy,
            registry::{Registry, RegistryConfig},
        };
        use basecraft_protocol::operation::RuntimeState;
        use serde_json::json;
        use std::time::{Duration, Instant};
        let temp = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt"] {
            std::fs::write(temp.path().join(name), "a").unwrap();
        }
        let registry = Registry::load(&RegistryConfig {
            bundled: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures"),
            packages: temp.path().join("plugins"),
            index: temp.path().join("index.json"),
        })
        .unwrap();
        let mut manager = JobManager::default().with_task_cancellation();
        let request = serde_json::from_value(json!({
            "operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"file","path":"a.txt"},{"kind":"file","path":"b.txt"}]},
            "parameters":{"mode":"hang"},"destinations":{"report":"report.json"}
        }))
        .unwrap();
        let accepted = manager
            .submit(
                &registry,
                request,
                SessionPolicy::new(temp.path(), true).unwrap(),
                "GUI",
            )
            .unwrap();
        let id = &accepted.run_id;
        let deadline = Instant::now() + Duration::from_secs(30);
        while manager.get(id).unwrap().state == RuntimeState::Queued {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut rows = json!({"jobs":[{"id":id},{"id":"other-process"}]});
        task_capabilities(&mut rows, false);
        assert_eq!(rows["jobs"][0]["cancellable"], true);
        assert_eq!(rows["jobs"][1]["cancellable"], false);
        task_capabilities(&mut rows, true);
        assert_eq!(rows["jobs"][0]["cancellable"], false);
        assert!(request_task_cancellation("other-process").starts_with("fail:"));
        assert_eq!(
            request_task_cancellation(id),
            "notify: Cancellation requested"
        );
        loop {
            let outcome = manager.get(id).unwrap();
            if outcome.state.terminal() {
                assert_eq!(outcome.state, RuntimeState::Cancelled);
                assert_eq!(outcome.artifacts, Vec::new());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!temp.path().join("report.json").exists());
        task_capabilities(&mut rows, false);
        assert_eq!(rows["jobs"][0]["cancellable"], false);
        assert!(request_task_cancellation(id).starts_with("fail:"));
    }

    #[test]
    fn gui_provenance_exports_preserve_authored_methods_and_report_changed_results() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let paths = basecraft_platform::paths::AppPaths {
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state"),
            cache: root.join("cache"),
            scratch: root.join("scratch"),
            home: None,
        };
        let file = root.join("sequence.fasta");
        std::fs::write(&file, ">example\nACGT\n").unwrap();
        let mut session = basecraft_ops::Session::recoverable_with_paths(root, &paths).unwrap();
        let document = session.open(&file).unwrap();
        session
            .apply(
                document,
                basecraft_protocol::Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                },
            )
            .unwrap();
        session.save(document, None).unwrap();
        assert!(run("history", &file, root).unwrap().contains("seq.replace"));
        assert!(run("verify", &file, root).is_ok());
        let export = root.join("export");
        std::fs::create_dir_all(export.join("reports")).unwrap();
        let authored = export.join("reports/methods.md");
        std::fs::write(&authored, "My laboratory notes").unwrap();
        run("methods", &file, &export).unwrap();
        assert_eq!(
            std::fs::read_to_string(authored).unwrap(),
            "My laboratory notes"
        );
        assert!(export.join("reports/methods.generated.md").is_file());
        let package = root.join("package");
        run("package", &file, &package).unwrap();
        assert!(package.join("sequence.fasta").is_file());
        assert!(run("package", &file, &package).is_err());
        std::fs::write(&file, ">example\nAAAA\n").unwrap();
        assert!(run("verify", &file, root).is_err());
    }
}
