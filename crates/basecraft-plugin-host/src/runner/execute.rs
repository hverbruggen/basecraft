//! Local execution stage. Preparation and publication do not launch processes.
use super::{
    Arc, AtomicBool, Error, InvokeResult, Ordering, PluginArtifact, PluginProcess,
    PreparedOperation, Result, Value, contract, link_count, records,
};
use basecraft_protocol::records::{RUN_RECORD_SCHEMA, RunEnvironment, RunRecord, RunRuntime};

impl PreparedOperation {
    pub fn execute(&self, cancelled: Arc<AtomicBool>) -> Result<InvokeResult> {
        if self.operation.descriptor.host_workflow.is_some() {
            return self.execute_workflow(&cancelled);
        }
        let permit = crate::compute::reserve_cpu(
            self.context.cpu_threads,
            Some(&self.run_path(&self.context.cancel_file)),
        )?;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Protocol("cancelled before spawn".into()));
        }
        self.operation.package.verify()?;
        let contact = self.lifecycle.reserve(&cancelled)?;
        let mut process = PluginProcess::spawn_with_credentials(
            &self.operation.package.manifest,
            self.operation
                .package
                .interpreter
                .as_deref()
                .ok_or_else(|| Error::Protocol("plugin runtime is unavailable".into()))?,
            permit.threads(),
            &self.policy.credentials_for(&self.operation),
        )?;
        self.lifecycle.configure(&mut process, cancelled);
        let interpreter = process.info.interpreter.clone();
        let mut context = self.context.clone();
        context.cpu_threads = permit.threads();
        let effective = Value::Object(crate::compute::parameters_for(
            &self.operation.descriptor,
            self.parameters
                .as_object()
                .cloned()
                .ok_or_else(|| Error::Protocol("parameters are not an object".into()))?,
            permit.threads(),
        ));
        contract::BoundedSchema::compile(&self.operation.descriptor.parameters)?
            .validate(&effective)?;
        *self
            .granted_parameters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = effective.clone();
        let mut result = process.invoke(
            &self.operation.descriptor.implementation,
            effective.clone(),
            context.clone(),
            self.sequence.as_ref().map(|target| &target.snapshot),
        );
        let teardown = process.finish();
        let contact_evidence = contact
            .as_ref()
            .map_or(Ok(()), crate::lifecycle::ContactPermit::complete);
        if (self.lifecycle.external() || self.lifecycle.remote_draft()) && result.is_err() {
            let receipt = &self.run_path(&self.context.outputs["receipt"]);
            if receipt.is_file() {
                result = (|| -> Result<InvokeResult> {
                    let metadata = receipt.symlink_metadata()?;
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || link_count(receipt)? != 1
                    {
                        return Err(Error::Protocol(
                            "unsafe retained receipt; submission status unknown".into(),
                        ));
                    }
                    let data =
                        serde_json::from_slice(&basecraft_ops::records::read_record(receipt)?)?;
                    Ok(InvokeResult {
                        data: Some(Box::new(data)),
                        artifacts: vec![PluginArtifact {
                            role: "receipt".into(),
                            path: ".".into(),
                        }],
                        ..InvokeResult::default()
                    })
                })();
            } else {
                result = Err(Error::Protocol(format!(
                    "submission status unknown: {}; no validated receipt; do not resubmit automatically",
                    result.unwrap_err()
                )));
            }
        }
        let (stdout, stderr) = process.captured_output();
        let mut record = self.run_record(effective, context.cpu_threads)?;
        let manifest = &self.operation.package.manifest;
        record.environment = RunEnvironment {
            interpreter,
            runtime: manifest
                .managed_runtime
                .as_ref()
                .zip(manifest.runtime.as_ref())
                .map(|(runtime, spec)| RunRuntime {
                    channels: spec.channels.clone(),
                    dependencies: spec.dependencies.clone(),
                    pypi_dependencies: spec.pypi_dependencies.clone(),
                    lock_digest: runtime.0.lock_digest.clone(),
                    manifest_digest: runtime.0.manifest_digest.clone(),
                    reported_versions: runtime.0.reported_versions.clone(),
                    executable_digests: runtime.0.executable_digests.clone(),
                }),
        };
        record.stdout = stdout;
        record.stderr = stderr;
        if let Ok(result) = &result {
            record.tool.clone_from(&result.tool);
            record.execution.clone_from(&result.execution);
        }
        let evidence = self.write_run_record(&record);
        match result {
            Ok(mut result) => {
                if self.lifecycle.external() {
                    let diagnostics: Vec<_> =
                        [teardown.err(), evidence.err(), contact_evidence.err()]
                            .into_iter()
                            .flatten()
                            .map(|e| e.to_string())
                            .collect();
                    if !diagnostics.is_empty() {
                        result.message = Some(format!(
                            "Local execution evidence incomplete: {}",
                            diagnostics.join("; ")
                        ));
                    }
                } else {
                    teardown?;
                    evidence?;
                }
                Ok(result)
            }
            Err(error) => {
                let error = if self.lifecycle.external() {
                    Error::Protocol(format!(
                        "Submission status unknown unless retained receipt proves acceptance: {error}. Do not automatically resubmit"
                    ))
                } else {
                    error
                };
                match evidence {
                    Ok(()) => Err(error),
                    Err(evidence) => Err(Error::Protocol(format!(
                        "{error}; execution evidence write failed: {evidence}"
                    ))),
                }
            }
        }
    }

    /// Validate the returned identity separately from fallible artifact writes,
    /// so an accepted external side effect remains available in a partial outcome.
    pub(crate) fn submission_identity(&self, result: &InvokeResult) -> Result<()> {
        if !self.lifecycle.external() {
            return Ok(());
        }
        contract::BoundedSchema::compile(&self.operation.descriptor.result)?
            .validate(result.data.as_deref().unwrap_or(&Value::Null))?;
        let plan: Value = serde_json::from_slice(&basecraft_ops::records::read_record(
            &self.run.artifact("query-preparation.json")?,
        )?)?;
        let receipt = &self.run_path(&self.context.outputs["receipt"]);
        if let Ok(metadata) = receipt.symlink_metadata()
            && (!metadata.is_file()
                || metadata.file_type().is_symlink()
                || link_count(receipt)? != 1)
        {
            return Err(Error::Protocol(
                "unsafe allocated receipt; submission status unknown".into(),
            ));
        }
        self.lifecycle.validate_identity(result, &plan)
    }
}

impl PreparedOperation {
    fn execute_workflow(&self, cancelled: &AtomicBool) -> Result<InvokeResult> {
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Protocol("cancelled before workflow".into()));
        }
        let registry = self.workflow_registry.as_ref().ok_or_else(|| {
            Error::Protocol("host workflow is missing accepted stage registry".into())
        })?;
        let effective = Value::Object(crate::compute::parameters_for(
            &self.operation.descriptor,
            self.parameters
                .as_object()
                .cloned()
                .ok_or_else(|| Error::Protocol("workflow parameters must be an object".into()))?,
            self.context.cpu_threads,
        ));
        *self
            .granted_parameters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = effective.clone();
        let method = self
            .operation
            .descriptor
            .host_workflow
            .as_deref()
            .ok_or_else(|| Error::Protocol("missing host workflow".into()))?;
        let mut workflow = crate::delimitation_pipeline::AlignmentWorkflow {
            registry,
            parent: self,
            cancelled,
            stages: vec![],
        };
        let execution = workflow.run(method, effective);
        let record = self.run_record(self.effective_parameters(), self.context.cpu_threads)?;
        self.write_run_record(&record)?;
        execution
    }

    /// The run's own record, without process output. Every path in it is
    /// relative to the run folder.
    fn run_record(&self, effective_parameters: Value, cpu_threads: usize) -> Result<RunRecord> {
        let folder = self.run.directory();
        let relative = |path: &str| records::relative(folder, &self.run_path(path));
        let mut inputs = self.context.input_evidence.clone();
        for record in inputs.values_mut().flatten() {
            if let Some(original) = &record.original_path {
                record.original_path = Some(relative(original)?);
            }
            record.run_directory = relative(&record.run_directory)?;
        }
        Ok(RunRecord {
            schema: RUN_RECORD_SCHEMA,
            operation: self.operation.id.clone(),
            package_version: self.operation.package.manifest.version.clone(),
            package_digest: Some(self.operation.package.digest.clone()),
            descriptor_digest: Some(self.operation.descriptor_digest.clone()),
            environment: RunEnvironment::default(),
            initiator: self.initiator.clone(),
            executor: self.operation.package.manifest.id.clone(),
            process_instance: basecraft_platform::process_instance().into(),
            inputs,
            effective_parameters,
            cpu_threads,
            stdout: String::new(),
            stderr: String::new(),
            tool: None,
            execution: None,
        })
    }

    fn write_run_record(&self, record: &RunRecord) -> Result<()> {
        self.tool
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone_from(&record.tool);
        records::write_json(
            &self.run.artifact("execution.json")?,
            &serde_json::to_value(record)?,
        )?;
        Ok(())
    }
}
