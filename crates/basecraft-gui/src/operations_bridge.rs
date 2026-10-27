//! Small generic installed-operation UI. Jobs belong to the application object,
//! not a tab; closing the picker or switching files never retargets their result.
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use serde_json::{Value, json};
use std::{path::Path, pin::Pin};

#[cxx_qt::bridge]
pub mod qobject {
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
    jobs: JobManager,
    active: Option<String>,
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
            .any(|binding| binding.min != 1 || binding.max != 1)
        {
            Some("File lists and optional inputs are available through CLI/Python/MCP")
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
    pub fn blast_contact(&self) -> QString {
        QString::from(&std::env::var("BASECRAFT_BLAST_CONTACT_EMAIL").unwrap_or_default())
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
        mut self: Pin<&mut Self>,
        queries: &QString,
        parameters: &QString,
        approval: &QString,
        truncate: bool,
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        let result = (|| -> basecraft_plugin_host::Result<_> {
            let queries: Vec<basecraft_ops::blast::Query> =
                serde_json::from_str(&queries.to_string())?;
            let mut parameters = blast_parameters(&parameters.to_string())?;
            parameters["approval_hash"] = json!(approval.to_string());
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
        })();
        match result {
            Ok((root, request)) => self.submit(
                &QString::from(&root.display().to_string()),
                &QString::from(&request.to_string()),
            ),
            Err(error) => {
                self.set_message(QString::from(&format!("fail: {error}")));
                false
            }
        }
    }

    pub fn catalog(&self) -> QString {
        QString::from(
            &catalog()
                .unwrap_or_else(|error| json!({"operations":[],"error":error.to_string()}))
                .to_string(),
        )
    }
    pub fn submit(mut self: Pin<&mut Self>, project: &QString, request: &QString) -> bool {
        if self.rust().busy {
            return false;
        }
        let request = request.to_string();
        let result = (|| -> basecraft_plugin_host::Result<_> {
            if request.len() > basecraft_protocol::operation::MAX_FRAME_BYTES {
                return Err(basecraft_plugin_host::Error::Protocol(
                    "operation request exceeds byte limit".into(),
                ));
            }
            let registry = Registry::load(&RegistryConfig::discover()?)?;
            let policy = SessionPolicy::new(Path::new(&project.to_string()), true)?;
            self.as_mut().rust_mut().jobs.submit(
                &registry,
                serde_json::from_str(&request)?,
                policy,
                "GUI",
            )
        })();
        match result {
            Ok(accepted) => {
                self.as_mut().rust_mut().active = Some(accepted.run_id.clone());
                self.as_mut()
                    .set_outcome(QString::from(&json!(accepted).to_string()));
                self.as_mut()
                    .set_message(QString::from("Running operation"));
                self.set_busy(true);
                true
            }
            Err(error) => {
                self.set_message(QString::from(&format!("fail: {error}")));
                false
            }
        }
    }
    pub fn poll(mut self: Pin<&mut Self>) {
        let Some(id) = self.rust().active.clone() else {
            return;
        };
        match self.rust().jobs.get(&id) {
            Ok(outcome) => {
                let terminal = outcome.state.terminal();
                let message =
                    if outcome.state == basecraft_protocol::operation::RuntimeState::Completed {
                        "done: Operation completed".into()
                    } else if terminal {
                        outcome.errors.first().map_or_else(
                            || "notify: Operation cancelled".into(),
                            |error| format!("fail: {}", error.message),
                        )
                    } else {
                        "Running operation".into()
                    };
                self.as_mut()
                    .set_outcome(QString::from(&json!(outcome).to_string()));
                self.as_mut().set_message(QString::from(&message));
                self.set_busy(!terminal);
            }
            Err(error) => {
                self.as_mut()
                    .set_message(QString::from(&format!("fail: {error}")));
                self.set_busy(false);
            }
        }
    }
    pub fn cancel(mut self: Pin<&mut Self>) {
        if let Some(id) = self.rust().active.clone() {
            if let Err(error) = self.rust().jobs.cancel(&id) {
                self.as_mut()
                    .set_message(QString::from(&format!("fail: {error}")));
            }
        }
        self.poll();
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
