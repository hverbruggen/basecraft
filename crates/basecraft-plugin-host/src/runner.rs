//! Shared registered-operation preparation and validated finalization. Scientific
//! bytes and publications use the same FileJob/store/input-evidence primitives.
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
    operation::*,
    plugin::InvokeResult,
    provenance::{ArtifactRevision, Completeness, Publication},
};
use serde_json::{Value, json};

use crate::{
    Error, PluginProcess, Result, contract, file_tool, policy::SessionPolicy,
    registry::RegisteredOperation,
};

pub struct SequenceTarget {
    pub doc: basecraft_ops::DocId,
    pub session: String,
    pub generation: u64,
    pub revision: u64,
    pub snapshot: crate::Snapshot,
    pub dependencies: Vec<PathBuf>,
}

pub struct PreparedOperation {
    pub sequence: Option<SequenceTarget>,
    pub operation: RegisteredOperation,
    pub run: ArtifactRun,
    pub request: OperationRequest,
    pub parameters: Value,
    pub context: InvocationContext,
    pub policy: SessionPolicy,
    pub ledger_inputs: Vec<ArtifactRevision>,
    pub initiator: String,
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
    ) -> Result<Self> {
        policy.check(&operation)?;
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
                    .origins
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect(),
            });
            bindings
        } else {
            policy.resolve(&operation, &request)?
        };
        let parameters =
            contract::normalize_parameters(&operation.descriptor, &request.parameters)?;
        operation.package.verify()?;
        let scratch = tempfile::Builder::new()
            .prefix("basecraft-operation-")
            .tempdir()?;
        let mut frozen = BTreeMap::new();
        let mut sources = Vec::new();
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
                        "identity-newick-v1"
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
            frozen.insert(name.clone(), values);
        }
        let run_id = ulid::Ulid::generate().to_string();
        let anchor = request
            .destinations
            .values()
            .next()
            .map_or_else(|| policy.root.join("operation"), |p| policy.root.join(p));
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
        let mut outputs = BTreeMap::new();
        for (name, binding) in &operation.descriptor.outputs {
            let path = run.staging(name)?;
            if binding.collection {
                std::fs::create_dir(&path)?;
            }
            outputs.insert(name.clone(), path.display().to_string());
        }
        let context = InvocationContext {
            schema: 1,
            operation: operation.id.clone(),
            run_id,
            inputs: frozen,
            outputs,
            cpu_threads: crate::compute::effective_allocation()?,
            cancel_file: run.artifact("cancel")?.display().to_string(),
            progress_file: run.artifact("progress")?.display().to_string(),
        };
        input_evidence::write_json(&run.artifact("descriptor.json")?, &operation.descriptor)?;
        input_evidence::write_json(&run.artifact("request.json")?, &request)?;
        Ok(Self {
            sequence,
            operation,
            run,
            request,
            parameters,
            context,
            policy,
            ledger_inputs,
            initiator,
            _scratch: scratch,
        })
    }

    pub fn execute(&self, cancelled: Arc<AtomicBool>) -> Result<InvokeResult> {
        let permit = crate::compute::reserve_cpu(
            self.context.cpu_threads,
            Some(Path::new(&self.context.cancel_file)),
        )?;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Protocol("cancelled before spawn".into()));
        }
        self.operation.package.verify()?;
        let mut process = PluginProcess::spawn_allocated(
            &self.operation.package.manifest,
            self.operation
                .package
                .interpreter
                .as_deref()
                .ok_or_else(|| Error::Protocol("plugin runtime is unavailable".into()))?,
            permit.threads(),
        )?;
        process.cancellation(cancelled);
        let mut context = self.context.clone();
        context.cpu_threads = permit.threads();
        let mut effective = self.parameters.clone();
        if let Some(parameter) = &self.operation.descriptor.cpu_parameter {
            effective[parameter] = json!(permit.threads());
        }
        contract::BoundedSchema::compile(&self.operation.descriptor.parameters)?
            .validate(&effective)?;
        let result = process.invoke_context(
            &self.operation.descriptor.implementation,
            effective.clone(),
            context.clone(),
            self.sequence.as_ref().map(|target| &target.snapshot),
        );
        let teardown = process.finish();
        let (stdout, stderr) = process.captured_output();
        input_evidence::write_json(
            &self.run.artifact("execution.json")?,
            &json!({
                "schema":1,"operation":self.operation.id,"package_version":self.operation.package.manifest.version,
                "package_digest":self.operation.package.digest,"descriptor_digest":self.operation.descriptor_digest,
                "initiator":self.initiator,"executor":self.operation.package.manifest.id,
                "process_instance":basecraft_platform::process_instance(),"context":context,
                "requested_parameters":self.request.parameters,"effective_parameters":effective,"stdout":stdout,"stderr":stderr,
            }),
        )?;
        match result {
            Ok(result) => {
                teardown?;
                Ok(result)
            }
            Err(error) => Err(error),
        }
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
        for (role, binding) in &self.operation.descriptor.outputs {
            let count = counts.get(role).copied().unwrap_or(0);
            if count < binding.min || count > binding.max {
                return Err(Error::Protocol(format!("artifact cardinality for {role}")));
            }
        }
        let mut actual = BTreeSet::new();
        scan_files(&self.run.directory().join("pending"), &mut actual, 0)?;
        if actual != paths {
            return Err(Error::Protocol("undeclared extra staged outputs".into()));
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
    ) -> Result<()> {
        let mut publication_outputs = Vec::new();
        for (index, (role, path, digest)) in validated.iter().enumerate() {
            let name = if self.operation.descriptor.outputs[role].collection {
                format!("{role}-{index}")
            } else {
                role.clone()
            };
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
                retained: None,
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
        for destination in self.request.destinations.values() {
            self.run.bind(&self.policy.root.join(destination))?;
        }
        Ok(())
    }
    pub fn persist(&self, outcome: &OperationOutcome) -> Result<()> {
        input_evidence::write_json(&self.run.artifact("runtime.json")?, outcome)?;
        Ok(())
    }
}

fn scan_files(directory: &Path, paths: &mut BTreeSet<PathBuf>, depth: usize) -> Result<()> {
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
