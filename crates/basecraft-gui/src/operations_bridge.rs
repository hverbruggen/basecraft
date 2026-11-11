//! Small generic installed-operation UI. Jobs belong to the application object,
//! not a tab; closing the picker or switching files never retargets their result.
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

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
        fn submit_blast(
            self: Pin<&mut AppOperations>,
            queries: &QString,
            parameters: &QString,
            approval: &QString,
            truncate: bool,
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
        fn poll(self: Pin<&mut AppOperations>);
        #[qinvokable]
        fn cancel(self: Pin<&mut AppOperations>);
    }
}
#[derive(Default)]
pub struct OperationsRust {
    busy: bool,
    message: QString,
    outcome: QString,
    cancelled: Option<Arc<AtomicBool>>,
    pending_result: Option<Result<basecraft_protocol::operation::OperationOutcome, String>>,
    browser_pending: bool,
}
impl Drop for OperationsRust {
    fn drop(&mut self) {
        if let Some(cancelled) = &self.cancelled {
            cancelled.store(true, Ordering::Release);
        }
    }
}
// Keep recursion bounded and reject forms we cannot represent faithfully.
fn supported_parameters(schema: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    if schema["enum"].is_array() {
        return true;
    }
    match schema["type"].as_str() {
        Some("boolean" | "integer" | "number" | "string") => true,
        Some("array") => supported_parameters(&schema["items"], depth + 1),
        Some("object") => schema["properties"].as_object().is_some_and(|properties| {
            properties
                .values()
                .all(|field| supported_parameters(field, depth + 1))
                && schema.get("oneOf").is_none_or(|choices| {
                    choices.as_array().is_some_and(|choices| {
                        let mut names = std::collections::BTreeSet::new();
                        choices.iter().all(|choice| {
                            choice.as_object().is_some_and(|fields| {
                                fields
                                    .keys()
                                    .all(|name| matches!(name.as_str(), "required" | "title"))
                                    && choice["required"].as_array().is_some_and(|required| {
                                        !required.is_empty()
                                            && required.iter().all(|name| {
                                                name.as_str().is_some_and(|name| {
                                                    properties.contains_key(name)
                                                        && names.insert(name.to_owned())
                                                })
                                            })
                                    })
                            })
                        })
                    })
                })
        }),
        _ => false,
    }
}
fn catalog() -> basecraft_plugin_host::Result<Value> {
    use basecraft_protocol::operation::ExecutionFamily;
    let registry = Registry::load(&RegistryConfig::discover()?)?;
    let mut rows = Vec::new();
    for operation in registry.operations.values() {
        let descriptor = &operation.descriptor;
        let properties = descriptor.parameters["properties"]
            .as_object()
            .ok_or_else(|| {
                basecraft_plugin_host::Error::Protocol("missing parameter properties".into())
            })?;
        let reason = if !operation.package.enabled {
            Some("Plugin is disabled")
        } else if operation.package.interpreter.is_none() {
            Some("Python runtime is unavailable")
        } else if descriptor.family != ExecutionFamily::FileAnalysis {
            Some("Use the sequence tools or CLI/Python/MCP for snapshot proposals")
        } else if descriptor
            .inputs
            .values()
            .any(|binding| binding.min > 1 || binding.max != 1)
        {
            Some("File lists are available through CLI/Python/MCP")
        } else if !supported_parameters(&descriptor.parameters, 0) {
            Some("This settings schema is available through CLI/Python/MCP")
        } else {
            None
        };
        let fields: Vec<_> = properties.iter().map(|(name,schema)| json!({"name":name,"schema":schema,"required":descriptor.parameters["required"].as_array().is_some_and(|required| required.iter().any(|value| value == name))})).collect();
        rows.push(json!({"operation":operation.id,"title":descriptor.title,"description":descriptor.description,
            "parameters":descriptor.parameters,"supported":reason.is_none(),"limitation":reason,"fields":fields,"inputs":descriptor.inputs,"outputs":descriptor.outputs,"pins":operation.pins(&registry.generation)}));
    }
    Ok(json!({"operations":rows,"diagnostics":registry.diagnostics}))
}
fn blast_parameters(parameters: &str) -> basecraft_plugin_host::Result<Value> {
    let registry = Registry::load(&RegistryConfig::discover()?)?;
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
        QString::from(basecraft_ops::blast::suggested_alphabet(&queries))
    }
    pub fn open_browser_window(mut self: Pin<&mut Self>, url: &QString) -> QString {
        if self.browser_pending {
            return QString::from("busy");
        }
        let url = url.to_string();
        self.as_mut().rust_mut().browser_pending = true;
        let thread = self.qt_thread();
        let spawn = std::thread::Builder::new()
            .name("basecraft-browser".into())
            .spawn(move || {
                let status = match basecraft_platform::browser::open_new_window(&url) {
                    Ok(true) => "opened",
                    Ok(false) => "unsupported",
                    Err(_) => "failed",
                };
                let _ = thread.queue(move |mut state| {
                    state.as_mut().rust_mut().browser_pending = false;
                    state.browser_finished(QString::from(&url), QString::from(status));
                });
            });
        if spawn.is_err() {
            self.as_mut().rust_mut().browser_pending = false;
            QString::from("failed")
        } else {
            QString::from("pending")
        }
    }
    pub fn blast_contact(&self) -> QString {
        QString::from(&std::env::var("BASECRAFT_CONTACT_EMAIL").unwrap_or_default())
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
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        let queries = queries.to_string();
        let parameters = parameters.to_string();
        let approval = approval.to_string();
        self.start_request(move || {
            let queries: Vec<basecraft_ops::blast::Query> =
                serde_json::from_str(&queries)?;
            let mut parameters = blast_parameters(&parameters)?;
            parameters["approval_hash"] = json!(approval);
            parameters["truncate"] = json!(truncate);
            let plan = basecraft_ops::blast::prepare(&queries, &parameters)?;
            basecraft_ops::blast::authorize(&plan, &parameters)?;
            let paths = basecraft_platform::paths::AppPaths::discover()
                .map_err(|e| basecraft_plugin_host::Error::Protocol(e.to_string()))?;
            // The existing operation artifact store retains query evidence and
            // receipt. This application-state root never edits the source file.
            let root = paths
                .state
                .join("operation-jobs")
                .join(ulid::Ulid::generate().to_string());
            std::fs::create_dir_all(&root)?;
            let input = root.join("query-source.json");
            basecraft_ops::input_evidence::write_json(&input, &queries)?;
            Ok((
                root,
                json!({"operation":"basecraft.blast/submit@1","inputs":{"queries":[{"kind":"file","path":"query-source.json"}]},"parameters":parameters}),
            ))
        })
    }

    pub fn catalog(&self) -> QString {
        QString::from(
            &catalog()
                .unwrap_or_else(|error| json!({"operations":[],"error":error.to_string()}))
                .to_string(),
        )
    }
    pub fn submit(self: Pin<&mut Self>, project: &QString, request: &QString) -> bool {
        let root = PathBuf::from(project.to_string());
        let request = request.to_string();
        self.start_request(move || {
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
        prepare: impl FnOnce() -> basecraft_plugin_host::Result<(PathBuf, Value)> + Send + 'static,
    ) -> bool {
        self.start_request_with_credential(prepare, None)
    }

    fn start_request_with_credential(
        mut self: Pin<&mut Self>,
        prepare: impl FnOnce() -> basecraft_plugin_host::Result<(PathBuf, Value)> + Send + 'static,
        credential: Option<String>,
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.as_mut().rust_mut().cancelled = Some(cancelled.clone());
        self.as_mut().set_outcome(QString::default());
        self.as_mut()
            .set_message(QString::from("Preparing operation"));
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        let spawn = std::thread::Builder::new()
            .name("operation-submission".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> basecraft_plugin_host::Result<_> {
                        let (root, request) = prepare()?;
                        if cancelled.load(Ordering::Acquire) {
                            return Err(basecraft_plugin_host::Error::Protocol(
                                "Operation cancelled before submission".into(),
                            ));
                        }
                        let registry = Registry::load(&RegistryConfig::discover()?)?;
                        let mut policy = SessionPolicy::new(&root, true)?;
                        if let Some(token) = credential {
                            let recipient = registry.resolve("basecraft.zenodo/draft@1", None)?;
                            if recipient.package.origin
                                != basecraft_plugin_host::registry::Origin::Bundled
                            {
                                return Err(basecraft_plugin_host::Error::Protocol(
                                    "unexpected credential package".into(),
                                ));
                            }
                            policy = policy.with_runtime_credential(
                                recipient,
                                "zenodo_access",
                                token,
                            )?;
                        }
                        let mut jobs = JobManager::default().with_task_cancellation();
                        let accepted = jobs.submit(
                            &registry,
                            serde_json::from_value(request)?,
                            policy,
                            "GUI",
                        )?;
                        let text = json!(accepted).to_string();
                        let _ = qt_thread.queue(move |mut state| {
                            state.as_mut().set_outcome(QString::from(text));
                            state.set_message(QString::from("Running operation"));
                        });
                        loop {
                            if cancelled.load(Ordering::Acquire) {
                                jobs.cancel(&accepted.run_id)?;
                            }
                            let outcome = jobs.get(&accepted.run_id)?;
                            if outcome.state.terminal() {
                                return Ok(outcome);
                            }
                            if let Some(evidence) = &outcome.evidence {
                                let path = PathBuf::from(evidence).join("progress");
                                if let Ok(file) = std::fs::File::open(path) {
                                    use std::io::Read;
                                    let mut stage = String::new();
                                    if file.take(4096).read_to_string(&mut stage).is_ok()
                                        && !stage.trim().is_empty()
                                    {
                                        let _ = qt_thread.queue(move |state| {
                                            state.set_message(QString::from(stage.trim()));
                                        });
                                    }
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                    },
                ))
                .unwrap_or_else(|_| {
                    Err(basecraft_plugin_host::Error::Protocol(
                        "Operation worker interrupted".into(),
                    ))
                });
                let _ = qt_thread.queue(move |mut state| {
                    state.as_mut().rust_mut().pending_result =
                        Some(result.map_err(|e| e.to_string()));
                });
            });
        if let Err(error) = spawn {
            self.as_mut().rust_mut().cancelled = None;
            self.as_mut().set_busy(false);
            self.set_message(QString::from(format!("fail: {error}")));
            return false;
        }
        true
    }
    pub fn poll(mut self: Pin<&mut Self>) {
        // Keep the existing poll/terminal transition contract: callers can
        // record a terminal outcome immediately after poll clears busy.
        let Some(result) = self.as_mut().rust_mut().pending_result.take() else {
            return;
        };
        self.as_mut().rust_mut().cancelled = None;
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
        if let Some(cancelled) = &self.rust().cancelled {
            cancelled.store(true, Ordering::Release);
        }
        self.set_message(QString::from("Cancelling operation"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn supports_nested_calibrations_and_exclusive_smoothing() {
        use super::*;
        let contract: Value =
            serde_json::from_str(include_str!("../../../plugins/sanderson/operations.json"))
                .unwrap();
        assert!(supported_parameters(
            &contract["operations"][0]["parameters"],
            0
        ));
        assert!(!supported_parameters(&json!({"type":"null"}), 0));
        assert!(!supported_parameters(
            &json!({"type":"object","properties":{"x":{"type":"number"}},"oneOf":[{"required":["x"]},{"required":["x"]}]}),
            0
        ));
    }
}
