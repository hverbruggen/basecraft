//! Local execution stage. Preparation and publication do not launch processes.
use super::*;

impl PreparedOperation {
    pub fn execute(&self, cancelled: Arc<AtomicBool>) -> Result<InvokeResult> {
        if self.operation.descriptor.host_workflow.is_some() {
            return self.execute_workflow(&cancelled);
        }
        let permit = crate::compute::reserve_cpu(
            self.context.cpu_threads,
            Some(Path::new(&self.context.cancel_file)),
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
        let mut result = process.invoke_context(
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
            let receipt = Path::new(&self.context.outputs["receipt"]);
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
                        serde_json::from_slice(&basecraft_ops::provenance::read_record(receipt)?)?;
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
        let evidence = input_evidence::write_json(
            &self.run.artifact("execution.json")?,
            &serde_json::to_value(ExternalExecution {
                schema: 1,
                operation: self.operation.id.clone(),
                package_version: self.operation.package.manifest.version.clone(),
                package_digest: self.operation.package.digest.clone(),
                descriptor_digest: self.operation.descriptor_digest.clone(),
                runtime: self
                    .operation
                    .package
                    .manifest
                    .managed_runtime
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()?,
                interpreter: self.operation.package.interpreter.clone(),
                initiator: self.initiator.clone(),
                executor: self.operation.package.manifest.id.clone(),
                process_instance: basecraft_platform::process_instance().into(),
                context: serde_json::to_value(&context)?,
                requested_parameters: self.request.parameters.clone(),
                effective_parameters: effective,
                stdout,
                stderr,
                tool: result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.tool.as_ref())
                    .map(serde_json::to_value)
                    .transpose()?,
                execution: result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.execution.as_ref())
                    .map(serde_json::to_value)
                    .transpose()?,
                extra: BTreeMap::new(),
            })?,
        );
        match result {
            Ok(mut result) => {
                if self.lifecycle.external() {
                    let diagnostics: Vec<_> = [
                        teardown.err(),
                        evidence.err().map(Error::from),
                        contact_evidence.err(),
                    ]
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
        let plan: Value = serde_json::from_slice(&basecraft_ops::provenance::read_record(
            &self.run.artifact("query-preparation.json")?,
        )?)?;
        let receipt = Path::new(&self.context.outputs["receipt"]);
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
        input_evidence::write_json(
            &self.run.artifact("execution.json")?,
            &serde_json::to_value(HostWorkflowExecution {
                schema: 1,
                workflow: method.into(),
                initiator: self.initiator.clone(),
                executor: self.operation.package.manifest.id.clone(),
                requested_parameters: self.request.parameters.clone(),
                effective_parameters: self.effective_parameters(),
                package_digest: self.operation.package.digest.clone(),
                descriptor_digest: self.operation.descriptor_digest.clone(),
                process_instance: basecraft_platform::process_instance().into(),
                extra: BTreeMap::new(),
            })?,
        )?;
        execution
    }
}
