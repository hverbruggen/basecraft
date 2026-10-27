//! Shared registered-operation preparation and validated finalization. Scientific
//! bytes and publications use the same `FileJob`/store/input-evidence primitives.
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use basecraft_ops::{
    analysis_store::{ArtifactDestination, ArtifactRun},
    input_evidence,
};
use basecraft_protocol::{
    operation::{
        ExecutionFamily, InputReference, InputView, InvocationContext, InvocationInputEvidence,
        MAX_ARTIFACTS, MAX_NESTING, OperationOutcome, OperationRequest, PluginArtifact,
        PublishedArtifact,
    },
    plugin::InvokeResult,
    provenance::{ArtifactRevision, Completeness, Publication},
};
use serde_json::{Value, json};

use crate::{
    Error, PluginProcess, Result, contract, file_tool, policy::SessionPolicy,
    registry::RegisteredOperation,
};

/// Explicit, caller-supplied semantic identity for opt-in result reuse. The caller
/// must include external producer identities (e.g. executable hashes) in parameters.
/// This query never automatically changes execution policy for arbitrary operations.
pub struct CompletedResultQuery {
    pub operation: String,
    pub pins: basecraft_protocol::operation::OperationPins,
    pub effective_parameters: Value,
    /// Match role/order/source/submitted hashes and transformation, not mutable paths.
    pub inputs: BTreeMap<String, Vec<InvocationInputEvidence>>,
}

pub struct VerifiedOperationResult {
    pub outcome: OperationOutcome,
    pub directory: PathBuf,
}

#[derive(Default)]
pub struct CompletedResultLookup {
    pub result: Option<VerifiedOperationResult>,
    /// Corrupt, future or bounded-out evidence is a visible miss, not a complete inventory.
    pub diagnostics: Vec<String>,
}

impl CompletedResultQuery {
    fn matches(&self, outcome: &OperationOutcome) -> bool {
        outcome.operation == self.operation
            && outcome.pins.package_digest == self.pins.package_digest
            && outcome.pins.descriptor_digest == self.pins.descriptor_digest
            && outcome.effective_parameters == self.effective_parameters
    }
}

/// Verify a completed public-operation result using its existing execution record,
/// frozen declared input evidence, and shared publication/input/artifact verification.
pub fn verify_completed_result(
    outcome: &OperationOutcome,
    query: &CompletedResultQuery,
) -> Result<VerifiedOperationResult> {
    use basecraft_protocol::operation::RuntimeState;
    if outcome.schema != 1
        || outcome.state != RuntimeState::Completed
        || !outcome.errors.is_empty()
        || !query.matches(outcome)
    {
        return Err(Error::Protocol(
            "completed receipt identity/state does not match".into(),
        ));
    }
    let directory = PathBuf::from(
        outcome
            .evidence
            .as_ref()
            .ok_or_else(|| Error::Protocol("completed receipt has no owned run".into()))?,
    );
    if directory.file_name().and_then(|name| name.to_str()) != Some(outcome.run_id.as_str()) {
        return Err(Error::Protocol(
            "receipt run identity differs from its location".into(),
        ));
    }
    let metadata = directory.symlink_metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Protocol("unsafe completed run directory".into()));
    }
    let execution: Value = serde_json::from_slice(&basecraft_ops::provenance::read_record(
        &directory.join("execution.json"),
    )?)?;
    if execution["schema"] != 1
        || execution["operation"] != query.operation
        || execution["package_digest"] != query.pins.package_digest
        || execution["descriptor_digest"] != query.pins.descriptor_digest
        || execution["effective_parameters"] != query.effective_parameters
    {
        return Err(Error::Protocol(
            "execution specification differs from receipt".into(),
        ));
    }
    let context: InvocationContext = serde_json::from_value(execution["context"].clone())?;
    if context.schema != 1
        || context.operation != query.operation
        || context.run_id != outcome.run_id
        || context.input_evidence.len() != query.inputs.len()
        || query.inputs.iter().any(|(role, expected)| {
            context.input_evidence.get(role).is_none_or(|actual| {
                actual.len() != expected.len()
                    || actual.iter().zip(expected).any(|(a, b)| {
                        a.source_hash != b.source_hash
                            || a.submitted_hash != b.submitted_hash
                            || a.transformation != b.transformation
                    })
            })
        })
    {
        return Err(Error::Protocol(
            "frozen declared inputs do not match reuse query".into(),
        ));
    }
    if outcome.artifacts.is_empty() {
        return Err(Error::Protocol("completed receipt has no artifacts".into()));
    }
    let first = Path::new(&outcome.artifacts[0].path);
    let bundle = basecraft_ops::provenance::bundle_for_result(first);
    if bundle.canonicalize()? != directory.canonicalize()? {
        return Err(Error::Protocol(
            "retained artifact is bound to a different run".into(),
        ));
    }
    let issues = basecraft_ops::provenance::verify(first)?;
    if !issues.is_empty() {
        return Err(Error::Protocol(format!(
            "completed publication is not reusable: {}",
            issues.join("; ")
        )));
    }
    let publication = basecraft_ops::provenance::inspect(first)?;
    for artifact in &outcome.artifacts {
        let path = Path::new(&artifact.path);
        if !path.canonicalize()?.starts_with(directory.canonicalize()?)
            || path.symlink_metadata()?.file_type().is_symlink()
            || basecraft_io::hash_file(path).map_err(basecraft_ops::Error::from)? != artifact.digest
            || !publication
                .outputs
                .iter()
                .any(|output| output.hash == artifact.digest)
        {
            return Err(Error::Protocol(
                "retained artifact differs from completed publication".into(),
            ));
        }
    }
    Ok(VerifiedOperationResult {
        outcome: outcome.clone(),
        directory,
    })
}

/// Search existing owned receipts for an explicitly reusable request. No index,
/// records, directory creation, automatic execution or publication is introduced.
pub fn lookup_completed_result(
    anchor: &Path,
    query: &CompletedResultQuery,
) -> CompletedResultLookup {
    let mut lookup = CompletedResultLookup::default();
    let runs = match ArtifactRun::runs_for_anchor(anchor, 4096) {
        Ok(runs) => runs,
        Err(error) => {
            lookup.diagnostics.push(error.to_string());
            return lookup;
        }
    };
    for directory in runs {
        let receipt = directory.join("runtime.json");
        if !receipt.exists() {
            continue;
        } // Input ledgers and legacy domain runs have no runtime receipt.
        let candidate = (|| -> Result<Option<VerifiedOperationResult>> {
            let value: Value =
                serde_json::from_slice(&basecraft_ops::provenance::read_record(&receipt)?)?;
            if value["schema"] != 1 {
                return Err(Error::Protocol(
                    "unsupported operation receipt version".into(),
                ));
            }
            let outcome: OperationOutcome = serde_json::from_value(value)?;
            if !query.matches(&outcome)
                || outcome.state != basecraft_protocol::operation::RuntimeState::Completed
            {
                return Ok(None);
            }
            if outcome.evidence.as_deref() != directory.to_str() {
                return Err(Error::Protocol(
                    "receipt location changed; reuse unavailable".into(),
                ));
            }
            verify_completed_result(&outcome, query).map(Some)
        })();
        match candidate {
            Ok(Some(result)) => {
                lookup.result = Some(result);
                return lookup;
            }
            Ok(None) => {}
            Err(error) => lookup
                .diagnostics
                .push(format!("{}: {error}", receipt.display())),
        }
    }
    lookup
}

pub struct SequenceTarget {
    pub doc: basecraft_ops::DocId,
    pub session: String,
    pub generation: u64,
    pub revision: u64,
    pub snapshot: crate::Snapshot,
    pub dependencies: Vec<PathBuf>,
}

pub struct PreparedOperation {
    pub(crate) lifecycle: crate::lifecycle::Lifecycle,
    pub(crate) input_diagnostics: Vec<String>,
    pub sequence: Option<SequenceTarget>,
    pub operation: RegisteredOperation,
    pub run: ArtifactRun,
    pub request: OperationRequest,
    pub parameters: Value,
    granted_parameters: std::sync::Mutex<Value>,
    pub context: InvocationContext,
    pub policy: SessionPolicy,
    pub ledger_inputs: Vec<ArtifactRevision>,
    pub initiator: String,
    workflow_registry: Option<Arc<crate::registry::Registry>>,
    _scratch: tempfile::TempDir,
}

impl PreparedOperation {
    /// Validations precede durable records. Only an accepted execution allocates
    /// storage; list/describe/denied calls never arrive here.
    pub fn prepare(
        operation: RegisteredOperation,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: String,
        session: Option<&mut basecraft_ops::Session>,
        registry: &crate::registry::Registry,
    ) -> Result<Self> {
        policy.check(&operation)?;
        let lifecycle = crate::lifecycle::Lifecycle::for_operation(&operation)?;
        let workflow_registry = if operation.descriptor.host_workflow.is_some() {
            registry.resolve("basecraft.delimitation/haplotypes@1", None)?;
            if operation.descriptor.host_workflow.as_deref() != Some("asap_alignment") {
                registry.resolve("basecraft.distance/tree@1", None)?;
            }
            Some(Arc::new(registry.clone()))
        } else {
            None
        };
        let mut sequence = None;
        let bindings = if operation.descriptor.family == ExecutionFamily::SequenceProposal {
            let session = session.ok_or_else(|| {
                Error::Protocol(
                    "sequence operation requires an owning headless/document session".into(),
                )
            })?;
            let name = operation
                .descriptor
                .inputs
                .keys()
                .next()
                .ok_or_else(|| Error::Protocol("missing snapshot binding".into()))?;
            if request.inputs.len() != 1
                || request
                    .inputs
                    .get(name)
                    .is_none_or(|values| values.len() != 1)
            {
                return Err(Error::Protocol(
                    "sequence operation requires exactly its named snapshot input".into(),
                ));
            }
            let (doc, bindings) = match &request.inputs[name][0] {
                InputReference::File { path } => {
                    let bindings = policy.resolve(&operation, &request)?;
                    (session.open(&policy.path(path, false)?)?, bindings)
                }
                InputReference::Document {
                    doc,
                    generation,
                    revision,
                } => {
                    let id = basecraft_ops::DocId(
                        doc.parse()
                            .map_err(|_| Error::Protocol("invalid session document ID".into()))?,
                    );
                    let checked = session.checked_sequence(id)?;
                    if session.generation(id) != *generation || checked.version() != *revision {
                        return Err(Error::Protocol(
                            "stale_revision: snapshot target changed".into(),
                        ));
                    }
                    for (role, path) in &request.destinations {
                        if !operation.descriptor.outputs.contains_key(role) {
                            return Err(Error::Protocol("unknown destination role".into()));
                        }
                        policy.path(path, true)?;
                    }
                    (
                        id,
                        crate::policy::ResolvedBindings {
                            inputs: BTreeMap::new(),
                            destinations: BTreeMap::new(),
                        },
                    )
                }
            };
            let checked = session.checked_sequence(doc)?;
            sequence = Some(SequenceTarget {
                doc,
                session: session.id().into(),
                generation: session.generation(doc),
                revision: checked.version(),
                snapshot: crate::Snapshot::of(session.doc_key(doc), checked)?,
                dependencies: session
                    .document_record(doc)?
                    .origin_bundles()
                    .map(Path::to_path_buf)
                    .collect(),
            });
            bindings
        } else {
            policy.resolve(&operation, &request)?
        };
        let input_diagnostics = lifecycle.verify_retained(&bindings.inputs)?;
        let mut supplied_parameters = request.parameters.clone();
        if lifecycle.external() && supplied_parameters.get("contact_email").is_none() {
            let contact = std::env::var("BASECRAFT_BLAST_CONTACT_EMAIL").map_err(|_| {
                Error::Protocol(
                    "Configure BASECRAFT_BLAST_CONTACT_EMAIL or supply contact_email for NCBI"
                        .into(),
                )
            })?;
            supplied_parameters["contact_email"] = json!(contact);
        }
        let parameters =
            contract::normalize_parameters(&operation.descriptor, &supplied_parameters)?;
        operation.package.verify()?;
        let scratch = tempfile::Builder::new()
            .prefix("basecraft-operation-")
            .tempdir()?;
        let mut frozen_inputs = BTreeMap::new();
        let mut sources = Vec::new();
        let mut submission_queries = Vec::new();
        for (name, paths) in bindings.inputs {
            let binding = &operation.descriptor.inputs[&name];
            let mut values = Vec::new();
            for (index, source) in paths.into_iter().enumerate() {
                let path = scratch.path().join(format!(
                    "{name}-{index}.{}",
                    if matches!(
                        binding.view,
                        InputView::AlignedFasta | InputView::UngappedFasta
                    ) {
                        "afa"
                    } else {
                        source
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("input")
                    }
                ));
                let original_hash = file_tool::freeze_file(&source, &path)?;
                if lifecycle.external() {
                    submission_queries.extend(basecraft_ops::blast::from_file_named(
                        &path,
                        &source.display().to_string(),
                    )?);
                }
                let transformation = match binding.view {
                    InputView::Bytes => "identity-v1",
                    InputView::AlignedFasta | InputView::UngappedFasta => {
                        let basecraft_ops::files::Loaded::Alignment(alignment) =
                            basecraft_ops::files::load(&path)?
                        else {
                            return Err(Error::Protocol("input view requires an alignment".into()));
                        };
                        let mut output = Vec::new();
                        if binding.view == InputView::AlignedFasta {
                            basecraft_ops::files::write_alignment(&alignment.doc, &mut output)?;
                        } else {
                            basecraft_ops::files::write_ungapped(&alignment.doc, &mut output)?;
                        }
                        std::fs::write(&path, output)?;
                        if binding.view == InputView::AlignedFasta {
                            "canonical-fasta-v1"
                        } else {
                            "ungapped-fasta-v1"
                        }
                    }
                    InputView::Newick => {
                        // Validate the declared tree without substituting a fit/preparation workflow.
                        basecraft_ops::files::load_tree(&path)?;
                        "identity-v1"
                    }
                    InputView::SequenceSnapshot => "sequence-file-v1",
                };
                let submitted_hash =
                    basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?;
                sources.push((
                    source,
                    original_hash,
                    submitted_hash,
                    transformation,
                    path.clone(),
                ));
                values.push(path.display().to_string());
            }
            frozen_inputs.insert(name.clone(), values);
        }
        let submission_plan = if lifecycle.external() {
            let plan = basecraft_ops::blast::prepare(&submission_queries, &parameters)?;
            basecraft_ops::blast::authorize(&plan, &parameters)?;
            Some(plan)
        } else {
            None
        };
        let run_id = ulid::Ulid::generate().to_string();
        let anchor = policy.root.join("operation");
        let anchor = if policy.artifact_write {
            anchor
        } else {
            let paths = basecraft_platform::paths::AppPaths::discover()
                .map_err(|error| Error::Protocol(error.to_string()))?;
            let directory = paths.state.join("operation-jobs").join(&run_id);
            std::fs::create_dir_all(&directory)?;
            directory.join("operation")
        };
        let run = ArtifactRun::create(&anchor, &run_id)?;
        let mut ledger_inputs = Vec::new();
        let mut upstream = Vec::new();
        let mut first_input = None;
        let mut evidence: BTreeMap<String, Vec<InvocationInputEvidence>> = BTreeMap::new();
        for (source, original_hash, submitted_hash, transformation, frozen) in sources {
            let ledger = input_evidence::reference_input_with_dependencies(
                &anchor,
                &source,
                &original_hash,
                &[],
                &format!("Input for {}", operation.id),
            )?;
            let mut record = input_evidence::read_input(ledger.directory())?;
            record.submitted_hash = submitted_hash;
            record.submitted_size = std::fs::metadata(&frozen)?.len();
            record.transformation = transformation.into();
            input_evidence::write_json(&ledger.artifact("input.json")?, &record)?;
            if let Some(role) = frozen_inputs
                .iter()
                .find(|(_, paths)| paths.contains(&frozen.display().to_string()))
                .map(|(role, _)| role.clone())
            {
                evidence
                    .entry(role)
                    .or_default()
                    .push(InvocationInputEvidence {
                        original_path: Some(source.display().to_string()),
                        source_hash: Some(original_hash.clone()),
                        submitted_hash: record.submitted_hash.clone(),
                        transformation: record.transformation.clone(),
                        run_directory: ledger.directory().display().to_string(),
                    });
            }
            let manifest = ledger.artifact("manifest.json")?;
            upstream.push(json!({"bundle":input_evidence::relative(run.directory(),ledger.directory())?,"id":ledger.id(),"hash":basecraft_io::hash_file(&manifest).map_err(basecraft_ops::Error::from)?}));
            let mut reference = record
                .source
                .clone()
                .ok_or_else(|| Error::Protocol("missing source reference".into()))?;
            reference.path = input_evidence::relative(run.directory(), &source)?;
            ledger_inputs.push(ArtifactRevision {
                id: format!("{}:source", ledger.id()),
                logical_name: source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash: original_hash,
                size: reference.stamp.size,
                retained: None,
                reference: Some(reference.clone()),
            });
            if first_input.is_none() {
                record.source = Some(reference);
                first_input = Some(record);
            }
        }
        // Reuse a normal primary ledger and its established upstream links. Each
        // contributing original also owns its own normal reference ledger.
        if let Some(mut input) = first_input {
            input.extra.insert("upstream".into(), json!(upstream));
            input_evidence::write_json(&run.artifact("input.json")?, &input)?;
            let _guard = input_evidence::lock()?;
            input_evidence::register(&run)?;
        }
        if let Some(target) = &sequence {
            let mut payload = Vec::new();
            let residues = target.snapshot.residues.map()?;
            let features = target.snapshot.features.map()?;
            payload.extend_from_slice(&(residues.len() as u64).to_le_bytes());
            payload.extend_from_slice(&residues);
            payload.extend_from_slice(&features);
            let mut dependencies = sources_from_upstream(run.directory())?;
            dependencies.extend(target.dependencies.iter().cloned());
            input_evidence::link_generated_input(
                &run,
                &dependencies,
                &payload,
                "sequence-arrow-buffers-v1",
                "Sequence operation snapshot",
            )?;
            let name = "snapshot.arrow-pair";
            basecraft_io::write_atomic(&run.artifact(name)?, |w| {
                std::io::Write::write_all(w, &payload)
            })
            .map_err(basecraft_ops::Error::from)?;
            ledger_inputs.push(ArtifactRevision {
                id: format!("{}:snapshot", run.id()),
                logical_name: name.into(),
                hash: basecraft_io::hash_bytes(&payload),
                size: payload.len() as u64,
                retained: Some(name.into()),
                reference: None,
            });
        }
        if let Some(plan) = &submission_plan {
            let payload = serde_json::to_vec(plan)?;
            let path = run.artifact("query-preparation.json")?;
            basecraft_io::write_atomic(&path, |w| std::io::Write::write_all(w, &payload))
                .map_err(basecraft_ops::Error::from)?;
            let dependencies = sources_from_upstream(run.directory())?;
            input_evidence::link_generated_input(
                &run,
                &dependencies,
                &payload,
                "blast-query-preparation-v1",
                "Frozen BLAST submission",
            )?;
            ledger_inputs.push(ArtifactRevision {
                id: format!("{}:queries", run.id()),
                logical_name: "query-preparation.json".into(),
                hash: basecraft_io::hash_bytes(&payload),
                size: payload.len() as u64,
                retained: Some("query-preparation.json".into()),
                reference: None,
            });
            frozen_inputs.insert("queries".into(), vec![path.display().to_string()]);
        }
        let mut outputs = BTreeMap::new();
        for (name, binding) in &operation.descriptor.outputs {
            let path = if operation.descriptor.host_workflow.is_some() {
                run.artifact("workflow")?
            } else {
                run.staging(name)?
            };
            if binding.collection {
                std::fs::create_dir(&path)?;
            }
            outputs.insert(name.clone(), path.display().to_string());
        }
        let context = InvocationContext {
            schema: 1,
            operation: operation.id.clone(),
            run_id,
            inputs: frozen_inputs,
            outputs,
            cpu_threads: crate::compute::effective_allocation()?,
            cancel_file: run.artifact("cancel")?.display().to_string(),
            progress_file: run.artifact("progress")?.display().to_string(),
            input_evidence: evidence,
            legacy_parameters: BTreeMap::new(),
        };
        input_evidence::write_json(&run.artifact("descriptor.json")?, &operation.descriptor)?;
        input_evidence::write_json(&run.artifact("request.json")?, &request)?;
        Ok(Self {
            lifecycle,
            input_diagnostics,
            sequence,
            operation,
            run,
            request,
            granted_parameters: std::sync::Mutex::new(parameters.clone()),
            parameters,
            context,
            policy,
            ledger_inputs,
            initiator,
            workflow_registry,
            _scratch: scratch,
        })
    }

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
        let mut process = PluginProcess::spawn_accepted(
            &self.operation.package.manifest,
            self.operation
                .package
                .interpreter
                .as_deref()
                .ok_or_else(|| Error::Protocol("plugin runtime is unavailable".into()))?,
            permit.threads(),
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
        if self.lifecycle.external() && result.is_err() {
            let receipt = Path::new(&self.context.outputs["receipt"]);
            if receipt.is_file() {
                result = (|| -> Result<InvokeResult> {
                    let metadata = receipt.symlink_metadata()?;
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || metadata.nlink() != 1
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
            &json!({
                "schema":1,"operation":self.operation.id,"package_version":self.operation.package.manifest.version,
                "package_digest":self.operation.package.digest,"descriptor_digest":self.operation.descriptor_digest,
                "runtime":self.operation.package.manifest.managed_runtime,"interpreter":self.operation.package.interpreter,
                "initiator":self.initiator,"executor":self.operation.package.manifest.id,
                "process_instance":basecraft_platform::process_instance(),"context":context,
                "requested_parameters":self.request.parameters,"effective_parameters":effective,"stdout":stdout,"stderr":stderr,
            }),
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
            && (!metadata.is_file() || metadata.file_type().is_symlink() || metadata.nlink() != 1)
        {
            return Err(Error::Protocol(
                "unsafe allocated receipt; submission status unknown".into(),
            ));
        }
        self.lifecycle.validate_identity(result, &plan)
    }

    /// Validate every member before crossing the publication boundary.
    pub fn validate_result(&self, result: &InvokeResult) -> Result<Vec<(String, PathBuf, String)>> {
        contract::BoundedSchema::compile(&self.operation.descriptor.result)?
            .validate(result.data.as_deref().unwrap_or(&Value::Null))?;
        if (!result.ops.is_empty()
            && (self.sequence.is_none() || !self.operation.descriptor.effects.document_write))
            || !result.outputs.is_empty()
        {
            return Err(Error::Protocol(
                "file operation returned proposals or legacy output paths".into(),
            ));
        }
        if result.ops.len() > 4096 {
            return Err(Error::Protocol("proposal count exceeds 4096".into()));
        }
        if result.artifacts.len() > MAX_ARTIFACTS {
            return Err(Error::Protocol("artifact count limit exceeded".into()));
        }
        if self.lifecycle.external() {
            self.submission_identity(result)?;
            let receipt = Path::new(&self.context.outputs["receipt"]);
            if !receipt.try_exists()? {
                input_evidence::write_json(
                    receipt,
                    result.data.as_deref().unwrap_or(&Value::Null),
                )?;
            }
        }
        let mut paths = BTreeSet::new();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut validated = Vec::new();
        for artifact in &result.artifacts {
            let binding = self
                .operation
                .descriptor
                .outputs
                .get(&artifact.role)
                .ok_or_else(|| Error::Protocol("undeclared artifact role".into()))?;
            let root = PathBuf::from(&self.context.outputs[&artifact.role]);
            let path = if binding.collection {
                let relative = Path::new(&artifact.path);
                if artifact.path.is_empty()
                    || relative
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_)))
                {
                    return Err(Error::Protocol("artifact member escapes allocation".into()));
                }
                let mut path = root.clone();
                for component in relative.components() {
                    path.push(component);
                    if path.symlink_metadata()?.file_type().is_symlink() {
                        return Err(Error::Protocol("symlink artifact".into()));
                    }
                }
                path
            } else {
                if artifact.path != "." {
                    return Err(Error::Protocol(
                        "single artifact must name its allocated role with '.'".into(),
                    ));
                }
                root
            };
            let metadata = path.symlink_metadata()?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || !paths.insert(path.clone())
            {
                return Err(Error::Protocol(
                    "artifact is not a unique ordinary staging file".into(),
                ));
            }
            *counts.entry(artifact.role.clone()).or_default() += 1;
            let hash = basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?;
            validated.push((artifact.role.clone(), path, hash));
        }
        // The registered IQ-TREE route publishes a collection rather than a
        // FileJob tree. Add the same sidecar before sealing that collection so
        // retained results, explicit exports, verification and repair agree.
        if self.operation.id == "basecraft.iqtree/tree@1"
            && let Some((name, kind)) = file_tool::iqtree_support(self.parameters.get("support"))
        {
            let trees: Vec<_> = validated
                .iter()
                .filter(|(role, path, _)| {
                    role == "artifacts"
                        && path
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
                })
                .map(|(_, path, _)| path.clone())
                .collect();
            for tree in trees {
                let sidecar = basecraft_ops::files::tree_sidecar(&tree);
                match std::fs::symlink_metadata(&sidecar) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(Error::Protocol(
                            "IQ-TREE support sidecar already exists in staged output".into(),
                        ));
                    }
                    Err(error) => return Err(error.into()),
                }
                if let Some(sidecar) = file_tool::write_support_sidecar(&tree, name, kind)? {
                    if !paths.insert(sidecar.clone()) {
                        return Err(Error::Protocol("duplicate support sidecar".into()));
                    }
                    *counts.entry("artifacts".into()).or_default() += 1;
                    let hash =
                        basecraft_io::hash_file(&sidecar).map_err(basecraft_ops::Error::from)?;
                    // Bound tree reads use the run-root sidecar; the collection
                    // member carries the same initial metadata when exported.
                    let hidden = self.run.artifact(
                        sidecar
                            .file_name()
                            .and_then(|file| file.to_str())
                            .ok_or_else(|| Error::Protocol("support sidecar filename".into()))?,
                    )?;
                    basecraft_io::write_atomic(&hidden, |writer| {
                        let mut input = std::fs::File::open(&sidecar)?;
                        std::io::copy(&mut input, writer)?;
                        Ok(())
                    })
                    .map_err(basecraft_ops::Error::from)?;
                    validated.push(("artifacts".into(), sidecar, hash));
                }
            }
        }
        for (role, binding) in &self.operation.descriptor.outputs {
            let count = counts.get(role).copied().unwrap_or(0);
            if count < binding.min || count > binding.max {
                return Err(Error::Protocol(format!("artifact cardinality for {role}")));
            }
        }
        let mut actual = BTreeSet::new();
        let scan_root = if self.operation.descriptor.host_workflow.is_some() {
            self.run.artifact("workflow")?
        } else {
            self.run.directory().join("pending")
        };
        scan_files(&scan_root, &mut actual, 0)?;
        if actual != paths {
            return Err(Error::Protocol("undeclared extra staged outputs".into()));
        }
        if self.lifecycle.external() {
            let plan: Value = serde_json::from_slice(&basecraft_ops::provenance::read_record(
                &self.run.artifact("query-preparation.json")?,
            )?)?;
            self.lifecycle.validate_receipt(
                result,
                Path::new(&self.context.outputs["receipt"]),
                &plan,
            )?;
        }
        input_evidence::write_json(&self.run.artifact("validated.json")?, result)?;
        input_evidence::write_json(&self.run.artifact("validated-artifacts.json")?, &validated)?;
        Ok(validated)
    }

    /// Record a receipt after each write; later failure remains partial and all
    /// pending/retained bytes remain available for explicit repair, never retry execution.
    pub fn finalize(
        &self,
        result: &InvokeResult,
        validated: &[(String, PathBuf, String)],
        outcome: &mut OperationOutcome,
        repair: bool,
    ) -> Result<()> {
        if self.lifecycle.external() && !self.run.artifact("execution.json")?.is_file() {
            return Err(Error::Protocol("execution evidence unavailable; retain RID and receipt; publication-only repair cannot invent missing execution evidence".into()));
        }
        let mut collections = BTreeMap::new();
        for (role, binding) in &self.operation.descriptor.outputs {
            if !binding.collection {
                continue;
            }
            let members: Vec<_> = validated
                .iter()
                .filter(|(member, _, _)| member == role)
                .collect();
            if members.is_empty() {
                continue;
            }
            let staging = PathBuf::from(&self.context.outputs[role]);
            let retained = if self.operation.descriptor.host_workflow.is_some() {
                staging.clone()
            } else {
                self.run.artifact(role)?
            };
            let intent = self
                .run
                .artifact(&format!("collection-{role}-intent.json"))?;
            let mut expected = BTreeMap::new();
            for (_, path, hash) in &members {
                let relative = path.strip_prefix(&staging).map_err(|_| {
                    Error::Protocol("collection member lies outside allocation".into())
                })?;
                expected.insert(relative.to_path_buf(), (*hash).clone());
            }
            if retained == staging {
                verify_collection(&retained, &expected)?;
            } else if retained.exists() {
                if !repair || !intent.is_file() {
                    return Err(Error::Protocol(
                        "output_conflict: retained collection already exists".into(),
                    ));
                }
                verify_collection(&retained, &expected)?;
            } else {
                verify_collection(&staging, &expected)?;
                input_evidence::write_json(&intent, &json!({"schema":1,"members":expected}))?;
                for path in expected.keys() {
                    std::fs::File::open(staging.join(path))?.sync_all()?;
                }
                self.run.publish_collection(role)?;
            }
            for (relative, hash) in &expected {
                let path = retained.join(relative).display().to_string();
                if !outcome
                    .artifacts
                    .iter()
                    .any(|a| a.path == path && &a.digest == hash)
                {
                    outcome.artifacts.push(PublishedArtifact {
                        role: role.clone(),
                        path,
                        digest: hash.clone(),
                    });
                }
            }
            self.persist(outcome)?;
            if let Some(destination) = self.request.destinations.get(role) {
                let destination_path = self.policy.root.join(destination);
                let intent = self
                    .run
                    .artifact(&format!("collection-{role}-export.json"))?;
                if destination_path.exists() {
                    if !repair || !intent.is_file() {
                        return Err(Error::Protocol(
                            "output_conflict: collection destination exists".into(),
                        ));
                    }
                    verify_collection(&destination_path, &expected)?;
                } else {
                    let destination = self.policy.path(destination, true)?;
                    input_evidence::write_json(
                        &intent,
                        &json!({"schema":1,"destination":destination,"members":expected}),
                    )?;
                    export_collection(&retained, &destination, &expected)?;
                }
                for (relative, hash) in &expected {
                    let path = destination_path.join(relative).display().to_string();
                    if !outcome
                        .artifacts
                        .iter()
                        .any(|a| a.path == path && &a.digest == hash)
                    {
                        outcome.artifacts.push(PublishedArtifact {
                            role: role.clone(),
                            path,
                            digest: hash.clone(),
                        });
                    }
                }
                self.persist(outcome)?;
            }
            collections.insert(role.clone(), retained);
        }
        let mut publication_outputs = Vec::new();
        for (index, (role, path, digest)) in validated.iter().enumerate() {
            let name = role.clone();
            if let Some(retained) = collections.get(role) {
                let relative = path
                    .strip_prefix(Path::new(&self.context.outputs[role]))
                    .map_err(|_| Error::Protocol("collection member escapes allocation".into()))?;
                let retained = retained.join(relative);
                publication_outputs.push(ArtifactRevision {
                    id: format!("{}:output-{index}", self.run.id()),
                    logical_name: input_evidence::relative(&self.policy.root, &retained)?,
                    hash: digest.clone(),
                    size: std::fs::metadata(&retained)?.len(),
                    retained: Some(input_evidence::relative(self.run.directory(), &retained)?),
                    reference: None,
                });
                continue;
            }
            let retained = self.run.artifact(&name)?;
            if outcome
                .artifacts
                .iter()
                .any(|a| a.path == retained.display().to_string() && &a.digest == digest)
            {
                if basecraft_io::hash_file(&retained).map_err(basecraft_ops::Error::from)?
                    != *digest
                {
                    return Err(Error::Protocol(
                        "retained artifact changed; repair refused".into(),
                    ));
                }
            } else {
                if basecraft_io::hash_file(path).map_err(basecraft_ops::Error::from)? != *digest {
                    return Err(Error::Protocol("validated staging artifact changed".into()));
                }
                let staging = self.run.staging(&name)?;
                if &staging != path {
                    std::fs::rename(path, &staging)?;
                }
                file_tool::publish_artifact(&self.run, &name, ArtifactDestination::Metadata)?;
                outcome.artifacts.push(PublishedArtifact {
                    role: role.clone(),
                    path: retained.display().to_string(),
                    digest: digest.clone(),
                });
                self.persist(outcome)?;
            }
            if let Some(destination) = self.request.destinations.get(role) {
                let expected = self.policy.root.join(destination);
                if outcome
                    .artifacts
                    .iter()
                    .any(|a| a.path == expected.display().to_string() && &a.digest == digest)
                {
                    if basecraft_io::hash_file(&expected).map_err(basecraft_ops::Error::from)?
                        != *digest
                    {
                        return Err(Error::Protocol(
                            "published destination changed; repair refused".into(),
                        ));
                    }
                } else {
                    let destination = self.policy.path(destination, true)?;
                    self.run.publish_secondary(&name, &destination)?;
                    outcome.artifacts.push(PublishedArtifact {
                        role: role.clone(),
                        path: destination.display().to_string(),
                        digest: digest.clone(),
                    });
                    self.persist(outcome)?;
                }
            }
            publication_outputs.push(ArtifactRevision {
                id: format!("{}:output-{index}", self.run.id()),
                logical_name: input_evidence::relative(
                    self.run
                        .directory()
                        .parent()
                        .and_then(Path::parent)
                        .and_then(Path::parent)
                        .and_then(Path::parent)
                        .ok_or_else(|| Error::Protocol("invalid run location".into()))?,
                    &retained,
                )?,
                hash: digest.clone(),
                size: std::fs::metadata(&retained)?.len(),
                retained: Some(input_evidence::relative(self.run.directory(), &retained)?),
                reference: None,
            });
        }
        outcome.data = result.data.as_deref().cloned();
        let publication = Publication {
            schema_version: 1,
            id: self.run.id().into(),
            session: basecraft_platform::process_instance().into(),
            parents: vec![],
            parent_hashes: BTreeMap::new(),
            created: basecraft_platform::now_rfc3339(),
            origin: format!(
                "{} executed by {}; initiated by {}",
                self.operation.id, self.operation.package.manifest.id, self.initiator
            ),
            inputs: self.ledger_inputs.clone(),
            outputs: publication_outputs,
            events: vec![],
            effective_edits: vec![],
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        basecraft_ops::provenance::publish_operation(self.run.directory(), &publication)?;
        for artifact in &outcome.artifacts {
            self.run.bind(Path::new(&artifact.path))?;
        }
        Ok(())
    }
    pub fn effective_parameters(&self) -> Value {
        self.granted_parameters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn persist(&self, outcome: &OperationOutcome) -> Result<()> {
        input_evidence::write_json(&self.run.artifact("runtime.json")?, outcome)?;
        Ok(())
    }
}

fn scan_files(directory: &Path, paths: &mut BTreeSet<PathBuf>, depth: usize) -> Result<()> {
    let metadata = directory.symlink_metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Protocol(
            "artifact root is not an ordinary directory".into(),
        ));
    }
    if depth > MAX_NESTING {
        return Err(Error::Protocol("artifact nesting limit exceeded".into()));
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.path().symlink_metadata()?;
        if metadata.is_dir() {
            scan_files(&entry.path(), paths, depth + 1)?;
        } else if metadata.is_file() {
            paths.insert(entry.path());
            if paths.len() > MAX_ARTIFACTS {
                return Err(Error::Protocol("artifact count limit exceeded".into()));
            }
        } else {
            return Err(Error::Protocol("symlink/special artifact".into()));
        }
    }
    Ok(())
}

fn sources_from_upstream(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.join("input.json").is_file() {
        return Ok(Vec::new());
    }
    let record = input_evidence::read_input(directory)?;
    Ok(record
        .extra
        .get("upstream")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value["bundle"].as_str().map(|path| directory.join(path)))
        .collect())
}

fn verify_collection(root: &Path, expected: &BTreeMap<PathBuf, String>) -> Result<()> {
    let metadata = root.symlink_metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Protocol("collection root is unsafe".into()));
    }
    let mut actual = BTreeSet::new();
    scan_files(root, &mut actual, 0)?;
    if actual != expected.keys().map(|path| root.join(path)).collect() {
        return Err(Error::Protocol(
            "collection members differ from validated output".into(),
        ));
    }
    for (relative, hash) in expected {
        let path = root.join(relative);
        if path.symlink_metadata()?.nlink() != 1
            || basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)? != *hash
        {
            return Err(Error::Protocol(
                "collection bytes changed after validation".into(),
            ));
        }
    }
    Ok(())
}
fn export_collection(
    source: &Path,
    destination: &Path,
    expected: &BTreeMap<PathBuf, String>,
) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Protocol("missing collection destination parent".into()))?;
    let pending = tempfile::Builder::new()
        .prefix(".basecraft-export-")
        .tempdir_in(parent)?;
    for (relative, hash) in expected {
        let target = pending.path().join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source.join(relative), &target)?;
        if basecraft_io::hash_file(&target).map_err(basecraft_ops::Error::from)? != *hash {
            return Err(Error::Protocol("collection changed during export".into()));
        }
        std::fs::File::open(target)?.sync_all()?;
    }
    basecraft_platform::atomic::rename_noreplace(pending.path(), destination)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

impl PreparedOperation {
    fn execute_workflow(&self, cancelled: &AtomicBool) -> Result<InvokeResult> {
        if self.operation.package.manifest.id != "basecraft.host" {
            return Err(Error::Protocol(
                "host workflows require the reserved application package".into(),
            ));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Protocol("cancelled before workflow".into()));
        }
        let registry = self.workflow_registry.as_ref().ok_or_else(|| {
            Error::Protocol("host workflow is missing accepted stage registry".into())
        })?;
        let preparation_plugin = registry
            .resolve("basecraft.delimitation/haplotypes@1", None)?
            .package
            .manifest
            .dir
            .clone();
        let evidence = self
            .context
            .input_evidence
            .get("alignment")
            .and_then(|values| values.first())
            .ok_or_else(|| Error::Protocol("workflow requires named alignment evidence".into()))?;
        let source = PathBuf::from(
            evidence
                .original_path
                .as_deref()
                .ok_or_else(|| Error::Protocol("workflow needs a file-backed alignment".into()))?,
        );
        if Some(basecraft_io::hash_file(&source).map_err(basecraft_ops::Error::from)?)
            != evidence.source_hash
        {
            return Err(Error::Protocol(
                "alignment changed before workflow preparation".into(),
            ));
        }
        let root = PathBuf::from(&self.context.outputs["artifacts"]);
        let mut session = basecraft_ops::Session::unlogged(&self.policy.root);
        let doc = session.open(&source)?;
        let mut preparation = crate::FileJob::from_alignment(
            &session,
            doc,
            (&preparation_plugin, "haplotypes"),
            crate::file_tool::AlignmentInput::Aligned,
            root.join("preparation.json"),
        )?
        .with_cpu_budget(self.context.cpu_threads)
        .with_registry(registry.clone());
        let submitted = basecraft_io::hash_file(Path::new(&self.context.inputs["alignment"][0]))
            .map_err(basecraft_ops::Error::from)?;
        if preparation.input_key.1 != submitted {
            return Err(Error::Protocol(
                "workflow preparation differs from accepted alignment".into(),
            ));
        }
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
        preparation.params.extend(
            effective
                .as_object()
                .cloned()
                .ok_or_else(|| Error::Protocol("workflow parameters must be an object".into()))?,
        );
        preparation
            .params
            .insert("cancel_file".into(), json!(self.context.cancel_file));
        preparation
            .params
            .insert("progress".into(), json!(self.context.progress_file));
        let method = self
            .operation
            .descriptor
            .host_workflow
            .as_deref()
            .ok_or_else(|| Error::Protocol("missing host workflow".into()))?;
        let fit_method = crate::delimitation_pipeline::workflow_fit(method)
            .ok_or_else(|| Error::Protocol("unknown host workflow".into()))?;
        let unique = self.parameters["unique_haplotypes"]
            .as_bool()
            .unwrap_or(fit_method != "asap");
        let workflow = crate::delimitation_pipeline::AlignmentJob {
            preparation,
            unique_haplotypes: unique,
            fit_method: fit_method.into(),
            output: root.join("analysis.spart"),
        };
        let execution = workflow.run().record(&mut session, doc);
        input_evidence::write_json(
            &self.run.artifact("execution.json")?,
            &json!({"schema":1,"workflow":method,"initiator":self.initiator,"executor":"basecraft.host","requested_parameters":self.request.parameters,"effective_parameters":self.effective_parameters(),"package_digest":self.operation.package.digest,"descriptor_digest":self.operation.descriptor_digest,"process_instance":basecraft_platform::process_instance()}),
        )?;
        let mut result = execution?;
        let mut files = BTreeSet::new();
        scan_files(&root, &mut files, 0)?;
        result.artifacts = files
            .into_iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .map(|relative| PluginArtifact {
                        role: "artifacts".into(),
                        path: relative.to_string_lossy().into_owned(),
                    })
                    .map_err(|_| Error::Protocol("workflow artifact escapes allocation".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        result.outputs.clear();
        result.data = None;
        Ok(result)
    }
}

#[cfg(test)]
mod completed_result_tests {
    use super::*;
    #[test]
    fn lookup_reports_corrupt_future_and_incomplete_inventory_without_creating_a_store() {
        let tmp = tempfile::tempdir().unwrap();
        let anchor = tmp.path().join("operation");
        let query = CompletedResultQuery {
            operation: "test.operation/run@1".into(),
            pins: basecraft_protocol::operation::OperationPins {
                generation: "g".into(),
                package_digest: "p".into(),
                descriptor_digest: "d".into(),
            },
            effective_parameters: json!({}),
            inputs: std::collections::BTreeMap::default(),
        };
        assert!(lookup_completed_result(&anchor, &query).result.is_none());
        assert!(!tmp.path().join(".basecraft").exists());
        let run = ArtifactRun::create(&anchor, "corrupt").unwrap();
        input_evidence::write_json(
            &run.artifact("runtime.json").unwrap(),
            &json!({"schema":99}),
        )
        .unwrap();
        let result = lookup_completed_result(&anchor, &query);
        assert!(result.result.is_none());
        assert_eq!(result.diagnostics.len(), 1);
        std::fs::write(run.artifact("runtime.json").unwrap(), b"broken JSON").unwrap();
        assert_eq!(
            lookup_completed_result(&anchor, &query).diagnostics.len(),
            1
        );
        assert!(
            ArtifactRun::runs_for_anchor(&anchor, 0)
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
    }
}
