//! Small generic installed-operation UI. Jobs belong to the application object,
//! not a tab; closing the picker or switching files never retargets their result.
use basecraft_plugin_host::{jobs::JobManager, policy::SessionPolicy};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};
use std::{path::PathBuf, pin::Pin};

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for AppOperations {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        #[qproperty(QString, message)]
        #[qproperty(QString, outcome)]
        type AppOperations = super::OperationsRust;
        #[qinvokable]
        fn catalog(self: &AppOperations) -> QString;
        #[qinvokable]
        fn blast_contact(self: &AppOperations) -> QString;
        #[qinvokable]
        fn blast_default_alphabet(self: &AppOperations, queries: &QString) -> QString;
        #[qinvokable]
        fn open_browser_window(self: Pin<&mut AppOperations>, url: &QString) -> QString;
        #[qsignal]
        #[cxx_name = "browserFinished"]
        fn browser_finished(self: Pin<&mut AppOperations>, url: QString, status: QString);
        #[qinvokable]
        fn preview_blast(self: &AppOperations, queries: &QString, parameters: &QString) -> QString;
        #[qinvokable]
        /// `folder` is where the search's records go: the session root of
        /// the document the queries came from.
        fn submit_blast(
            self: Pin<&mut AppOperations>,
            queries: &QString,
            parameters: &QString,
            approval: &QString,
            truncate: bool,
            folder: &QString,
        ) -> bool;
        #[qinvokable]
        fn submit(self: Pin<&mut AppOperations>, project: &QString, request: &QString) -> bool;
        #[qinvokable]
        fn submit_zenodo(
            self: Pin<&mut AppOperations>,
            project: &QString,
            request: &QString,
            token: &QString,
        ) -> bool;
        #[qinvokable]
        fn cancel(self: Pin<&mut AppOperations>);
    }
}
#[derive(Default)]
pub struct OperationsRust {
    busy: bool,
    message: QString,
    outcome: QString,
    work: Option<basecraft_ops::work::Work>,
    browser_pending: bool,
}
impl Drop for OperationsRust {
    fn drop(&mut self) {
        if let Some(work) = &self.work {
            work.cancel();
        }
    }
}
fn blast_parameters(parameters: &str) -> basecraft_plugin_host::Result<Value> {
    let registry = crate::plugins::registry()?;
    let operation = registry.resolve("basecraft.blast/submit@1", None)?;
    basecraft_plugin_host::contract::normalize_parameters(
        &operation.descriptor,
        &serde_json::from_str(parameters)?,
    )
}

impl qobject::AppOperations {
    pub fn blast_default_alphabet(&self, queries: &QString) -> QString {
        let queries: Vec<basecraft_ops::blast::Query> =
            serde_json::from_str(&queries.to_string()).unwrap_or_default();
        QString::from(basecraft_ops::blast::suggested_alphabet(&queries).unwrap_or(""))
    }
    pub fn open_browser_window(mut self: Pin<&mut Self>, url: &QString) -> QString {
        if self.browser_pending {
            return QString::from("busy");
        }
        let url = url.to_string();
        self.as_mut().rust_mut().browser_pending = true;
        let spawn = crate::jobs::start(
            self.qt_thread(),
            move |_| {
                Ok(match basecraft_platform::browser::open_new_window(&url) {
                    Ok(true) => (url, "opened"),
                    Ok(false) => (url, "unsupported"),
                    Err(_) => (url, "failed"),
                })
            },
            |mut state, result| {
                state.as_mut().rust_mut().browser_pending = false;
                let (url, status) = result.unwrap_or_default();
                state.browser_finished(QString::from(&url), QString::from(status));
            },
        );
        if spawn.is_err() {
            self.as_mut().rust_mut().browser_pending = false;
            QString::from("failed")
        } else {
            QString::from("pending")
        }
    }
    /// The email NCBI BLAST asks for: the BLAST contact in the contact
    /// details, else the submitter's email.
    pub fn blast_contact(&self) -> QString {
        let email = basecraft_platform::paths::AppPaths::discover()
            .ok()
            .and_then(|paths| {
                basecraft_platform::settings::SettingsStore::new(paths.preferences())
                    .load()
                    .ok()
            })
            .map(|prefs| prefs.service_email("blast").to_owned())
            .unwrap_or_default();
        QString::from(&email)
    }
    pub fn preview_blast(&self, queries: &QString, parameters: &QString) -> QString {
        let result = (|| -> basecraft_plugin_host::Result<_> {
            let queries: Vec<basecraft_ops::blast::Query> =
                serde_json::from_str(&queries.to_string())?;
            let parameters = blast_parameters(&parameters.to_string())?;
            let mut plan =
                serde_json::to_value(basecraft_ops::blast::prepare(&queries, &parameters)?)?;
            plan.as_object_mut().map(|plan| plan.remove("fasta"));
            Ok(plan)
        })();
        QString::from(
            &match result {
                Ok(plan) => plan,
                Err(error) => json!({"error":error.to_string()}),
            }
            .to_string(),
        )
    }
    pub fn submit_blast(
        self: Pin<&mut Self>,
        queries: &QString,
        parameters: &QString,
        approval: &QString,
        truncate: bool,
        folder: &QString,
    ) -> bool {
        let folder = folder.to_string();
        if self.rust().busy {
            return false;
        }
        let queries = queries.to_string();
        let parameters = parameters.to_string();
        let approval = approval.to_string();
        self.start_request("BLAST search", move || {
            let queries: Vec<basecraft_ops::blast::Query> =
                serde_json::from_str(&queries)?;
            let mut parameters = blast_parameters(&parameters)?;
            parameters["approval_hash"] = json!(approval);
            parameters["truncate"] = json!(truncate);
            let plan = basecraft_ops::blast::prepare(&queries, &parameters)?;
            basecraft_ops::blast::authorize(&plan, &parameters)?;
            // The search's records stay with the data it came from (`folder`,
            // the controller's session root).
            if folder.is_empty() {
                return Err(basecraft_plugin_host::Error::Protocol(
                    "The searched document has no folder for the search's records".into(),
                ));
            }
            let root = PathBuf::from(folder);
            let input = basecraft_ops::blast::record_queries(&root, &queries).map_err(|error| {
                basecraft_plugin_host::Error::Protocol(match error {
                    basecraft_ops::Error::ReadOnlyFolder { .. } => "Search refused: the folder is \
                        read-only, so basecraft cannot keep the search's records there."
                        .into(),
                    error => error.to_string(),
                })
            })?;
            Ok((
                root,
                json!({"operation":"basecraft.blast/submit@1","inputs":{"queries":[{"kind":"file","path":input}]},"parameters":parameters}),
            ))
        })
    }

    pub fn catalog(&self) -> QString {
        QString::from(
            &crate::plugins::catalog()
                .unwrap_or_else(|error| json!({"operations":[],"error":error.to_string()}))
                .to_string(),
        )
    }
    pub fn submit(self: Pin<&mut Self>, project: &QString, request: &QString) -> bool {
        let root = PathBuf::from(project.to_string());
        let request = request.to_string();
        let operation = serde_json::from_str::<Value>(&request)
            .ok()
            .and_then(|value| value["operation"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "Operation".into());
        self.start_request(&operation, move || {
            if request.len() > basecraft_protocol::operation::MAX_FRAME_BYTES {
                return Err(basecraft_plugin_host::Error::Protocol(
                    "operation request exceeds byte limit".into(),
                ));
            }
            Ok((root, serde_json::from_str(&request)?))
        })
    }

    pub fn submit_zenodo(
        self: Pin<&mut Self>,
        project: &QString,
        request: &QString,
        token: &QString,
    ) -> bool {
        let root = PathBuf::from(project.to_string());
        let request = request.to_string();
        let token = token.to_string();
        self.start_request_with_credential(
            "Zenodo draft",
            move || {
                if request.len() > basecraft_protocol::operation::MAX_FRAME_BYTES {
                    return Err(basecraft_plugin_host::Error::Protocol(
                        "operation request exceeds byte limit".into(),
                    ));
                }
                let value: Value = serde_json::from_str(&request)?;
                if value["operation"] != "basecraft.zenodo/draft@1" {
                    return Err(basecraft_plugin_host::Error::Protocol(
                        "unexpected credential recipient".into(),
                    ));
                }
                Ok((root, value))
            },
            Some(token),
        )
    }

    fn start_request(
        self: Pin<&mut Self>,
        operation: &str,
        prepare: impl FnOnce() -> basecraft_plugin_host::Result<(PathBuf, Value)> + Send + 'static,
    ) -> bool {
        self.start_request_with_credential(operation, prepare, None)
    }

    fn start_request_with_credential(
        mut self: Pin<&mut Self>,
        operation: &str,
        prepare: impl FnOnce() -> basecraft_plugin_host::Result<(PathBuf, Value)> + Send + 'static,
        credential: Option<String>,
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        self.as_mut().set_outcome(QString::default());
        self.as_mut()
            .set_message(QString::from("Preparing operation"));
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        let started = crate::jobs::start_listed(
            self.qt_thread(),
            crate::jobs::Listing {
                operation: operation.into(),
                source: String::new(),
            },
            move |work| {
                run_request(prepare, credential, work, &qt_thread).map_err(|e| e.to_string())
            },
            qobject::AppOperations::finish,
        );
        match started {
            Ok(work) => {
                self.as_mut().rust_mut().work = Some(work);
                true
            }
            Err(error) => {
                self.as_mut().set_busy(false);
                self.set_message(QString::from(format!("fail: {error}")));
                false
            }
        }
    }

    /// Show a finished run: its outcome first, then the message, then busy
    /// clears, so a caller reacting to `busy` reads the final outcome.
    fn finish(
        mut self: Pin<&mut Self>,
        result: Result<basecraft_protocol::operation::OperationOutcome, String>,
    ) {
        self.as_mut().rust_mut().work = None;
        match result {
            Ok(outcome) => {
                let message =
                    if outcome.state == basecraft_protocol::operation::RuntimeState::Completed {
                        "done: Operation completed".into()
                    } else {
                        outcome.errors.first().map_or_else(
                            || "notify: Operation cancelled".into(),
                            |e| format!("fail: {}", e.message),
                        )
                    };
                self.as_mut()
                    .set_outcome(QString::from(json!(outcome).to_string()));
                self.as_mut().set_message(QString::from(message));
            }
            Err(error) => {
                self.as_mut().set_outcome(QString::from(
                    json!({"state":"failed","errors":[{"message":error}]}).to_string(),
                ));
                self.as_mut()
                    .set_message(QString::from(format!("fail: {error}")));
            }
        }
        self.set_busy(false);
    }
    pub fn cancel(self: Pin<&mut Self>) {
        if let Some(work) = &self.rust().work {
            work.cancel();
        }
        self.set_message(QString::from("Cancelling operation"));
    }
}

/// Submit a prepared request and wait for its outcome on the job thread,
/// showing the plugin's progress text as the owner's message.
fn run_request(
    prepare: impl FnOnce() -> basecraft_plugin_host::Result<(PathBuf, Value)>,
    credential: Option<String>,
    work: &basecraft_ops::work::Work,
    owner: &cxx_qt::CxxQtThread<qobject::AppOperations>,
) -> basecraft_plugin_host::Result<basecraft_protocol::operation::OperationOutcome> {
    let (root, request) = prepare()?;
    if work.cancelled() {
        return Err(basecraft_plugin_host::Error::Protocol(
            "Operation cancelled before submission".into(),
        ));
    }
    let registry = crate::plugins::registry()?;
    let mut policy = SessionPolicy::new(&root, true)?;
    if let Some(token) = credential {
        let recipient = registry.resolve("basecraft.zenodo/draft@1", None)?;
        if recipient.package.origin != basecraft_plugin_host::registry::Origin::Bundled {
            return Err(basecraft_plugin_host::Error::Protocol(
                "unexpected credential package".into(),
            ));
        }
        policy = policy.with_runtime_credential(recipient, "zenodo_access", token)?;
    }
    let mut jobs = JobManager::default();
    let accepted = jobs.submit(&registry, serde_json::from_value(request)?, policy, "GUI")?;
    let text = json!(accepted).to_string();
    let _ = owner.queue(move |mut state| {
        state.as_mut().set_outcome(QString::from(text));
        state.set_message(QString::from("Running operation"));
    });
    // The run belongs to no open document; a detached session lets the
    // shared wait drive it.
    let mut session = basecraft_ops::Session::detached(&root);
    let shown = std::cell::RefCell::new(String::new());
    jobs.wait_in_session(&accepted.run_id, &mut session, &|| {
        if let Ok(outcome) = jobs.get(&accepted.run_id)
            && let Some(evidence) = &outcome.evidence
            && let Ok(file) = std::fs::File::open(PathBuf::from(evidence).join("progress"))
        {
            use std::io::Read;
            let mut stage = String::new();
            let mut shown = shown.borrow_mut();
            if file.take(4096).read_to_string(&mut stage).is_ok()
                && !stage.trim().is_empty()
                && *shown != stage
            {
                shown.clone_from(&stage);
                work.progress(stage.trim());
                let _ = owner.queue(move |state| {
                    state.set_message(QString::from(stage.trim()));
                });
            }
        }
        work.cancelled()
    })
}
