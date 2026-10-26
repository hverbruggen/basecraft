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
        } else if properties.values().any(|field| {
            !matches!(
                field["type"].as_str(),
                Some("boolean" | "integer" | "string")
            ) && !field["enum"].is_array()
        }) {
            Some("This settings schema is available through CLI/Python/MCP")
        } else {
            None
        };
        let fields: Vec<_> = properties.iter().map(|(name,schema)| json!({"name":name,"schema":schema,"required":descriptor.parameters["required"].as_array().is_some_and(|required| required.iter().any(|value| value == name))})).collect();
        rows.push(json!({"operation":operation.id,"title":descriptor.title,"description":descriptor.description,
            "supported":reason.is_none(),"limitation":reason,"fields":fields,"inputs":descriptor.inputs,"outputs":descriptor.outputs,"pins":operation.pins(&registry.generation)}));
    }
    Ok(json!({"operations":rows,"diagnostics":registry.diagnostics}))
}
impl qobject::AppOperations {
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
