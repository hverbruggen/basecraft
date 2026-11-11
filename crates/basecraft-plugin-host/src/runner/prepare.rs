//! Preparation stages: capture session bindings, freeze input bytes, then allocate ledgers.
use super::{
    Arc, ArtifactRevision, ArtifactRun, BTreeMap, DocumentTarget, Error, ExecutionFamily,
    InputReference, InputView, InvocationContext, InvocationInputEvidence, OperationRequest, Path,
    PathBuf, PreparedOperation, RegisteredOperation, Result, SequenceTarget, SessionPolicy, Value,
    captures, contract, input_evidence, sources_from_upstream,
};
use basecraft_protocol::operation::QueryPreparation;

struct GeneratedInput {
    position: usize,
    bytes: Vec<u8>,
    transformation: &'static str,
    extension: &'static str,
    dependencies: Vec<PathBuf>,
}

/// The input view of a session document's current revision.
fn document_view(
    session: &basecraft_ops::Session,
    doc: basecraft_ops::DocId,
    view: InputView,
) -> Result<(Vec<u8>, &'static str, &'static str)> {
    let mut bytes = Vec::new();
    match view {
        InputView::AlignedFasta if session.is_alignment(doc) => {
            session
                .alignment(doc)?
                .validate_aligned_input()
                .map_err(|error| Error::Protocol(error.to_string()))?;
            basecraft_ops::files::write_alignment(session.alignment(doc)?, &mut bytes)?;
            Ok((bytes, "canonical-fasta-v1", "afa"))
        }
        InputView::UngappedFasta if session.is_alignment(doc) => {
            basecraft_ops::files::write_ungapped(session.alignment(doc)?, &mut bytes)?;
            Ok((bytes, "ungapped-fasta-v1", "afa"))
        }
        InputView::Newick if session.is_tree(doc) => Ok((
            session.tree(doc)?.to_newick_with_rooting().into_bytes(),
            "canonical-newick-v1",
            "nwk",
        )),
        _ => Err(Error::Protocol(
            "unsupported_input: document kind does not match the input view".into(),
        )),
    }
}

/// Resolved session inputs, before target binding, freezing or run allocation.
struct PreparationInputs {
    sequence: Option<SequenceTarget>,
    document: Option<DocumentTarget>,
    input_record: basecraft_ops::records::DocumentRecord,
    generated: BTreeMap<String, Vec<GeneratedInput>>,
    accepted_hashes: BTreeMap<PathBuf, String>,
    bindings: crate::policy::ResolvedBindings,
}

/// Validate input bindings against the owning session's current revisions and
/// capture generated views. The caller keeps the session and all later stages.
fn prepare_inputs(
    operation: &RegisteredOperation,
    request: &OperationRequest,
    policy: &SessionPolicy,
    session: &mut Option<&mut basecraft_ops::Session>,
) -> Result<PreparationInputs> {
    let mut sequence = None;
    let mut document = None;
    let mut input_record = basecraft_ops::records::DocumentRecord::empty("Inputs");
    let mut generated: BTreeMap<String, Vec<GeneratedInput>> = BTreeMap::new();
    let mut accepted_hashes: BTreeMap<PathBuf, String> = BTreeMap::new();
    let bindings = if operation.descriptor.family == ExecutionFamily::SequenceProposal {
        let session = session.as_deref_mut().ok_or_else(|| {
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
                let bindings = policy.resolve(operation, request)?;
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
                if session.generation(id)? != *generation || checked.version() != *revision {
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
                        documents: BTreeMap::new(),
                        destinations: BTreeMap::new(),
                    },
                )
            }
        };
        let checked = session.checked_sequence(doc)?;
        sequence = Some(SequenceTarget {
            doc,
            session: session.id().into(),
            generation: session.generation(doc)?,
            revision: checked.version(),
            snapshot: crate::Snapshot::of(session.doc_key(doc)?, checked)?,
            dependencies: session
                .document_record(doc)?
                .origin_bundles()
                .map(Path::to_path_buf)
                .collect(),
        });
        bindings
    } else {
        let mut bindings = policy.resolve(operation, request)?;
        let documents = std::mem::take(&mut bindings.documents);
        if !documents.is_empty() {
            let session = session.as_deref().ok_or_else(|| {
                Error::Protocol("document inputs require an owning session".into())
            })?;
            for (name, bound) in documents {
                let view = operation.descriptor.inputs[&name].view;
                for (position, binding) in bound {
                    let id = basecraft_ops::DocId(
                        binding
                            .doc
                            .parse()
                            .map_err(|_| Error::Protocol("invalid session document ID".into()))?,
                    );
                    if !session.is_open(id)
                        || session.generation(id)? != binding.generation
                        || session.revision(id)? != binding.revision
                    {
                        return Err(Error::Protocol(
                            "stale_revision: input document changed".into(),
                        ));
                    }
                    if view == InputView::AlignedFasta && session.is_alignment(id) {
                        session
                            .alignment(id)?
                            .validate_aligned_input()
                            .map_err(|error| Error::Protocol(error.to_string()))?;
                    }
                    input_record.include_input(&session.document_record(id)?);
                    let saved = session.saved_source(id)?;
                    if document.is_none() {
                        let key = session.doc_key(id)?;
                        document = Some(DocumentTarget {
                            doc: id,
                            session: session.id().into(),
                            generation: binding.generation,
                            revision: binding.revision,
                            record: session.document_record(id)?,
                            input: (
                                if view == InputView::UngappedFasta {
                                    format!("{key} (ungapped)")
                                } else {
                                    key.into()
                                },
                                String::new(),
                            ),
                            saved: saved.as_ref().map(|(path, _, _)| path.clone()),
                            path: session.root().join(session.doc_key(id)?),
                            receives_results: false,
                            save: false,
                        });
                    }
                    // A clean document is its saved file; unsaved edits are
                    // submitted as a generated snapshot of the current revision.
                    if let Some((path, _, hash)) = saved {
                        let files = bindings.inputs.entry(name.clone()).or_default();
                        files.insert(position.min(files.len()), path.clone());
                        accepted_hashes.insert(path, hash);
                    } else {
                        let (bytes, transformation, extension) = document_view(session, id, view)?;
                        generated
                            .entry(name.clone())
                            .or_default()
                            .push(GeneratedInput {
                                position,
                                bytes,
                                transformation,
                                extension,
                                dependencies: session
                                    .document_record(id)?
                                    .origin_bundles()
                                    .map(Path::to_path_buf)
                                    .collect(),
                            });
                    }
                }
            }
            let generated_count: usize = generated.values().map(Vec::len).sum();
            if generated_count > 1 {
                return Err(Error::Protocol(
                    "unsupported_input: save the document before combining it with other inputs"
                        .into(),
                ));
            }
        }
        bindings
    };
    Ok(PreparationInputs {
        sequence,
        document,
        input_record,
        generated,
        accepted_hashes,
        bindings,
    })
}

/// Resolve the explicit target in its owning session before freezing inputs.
fn prepare_target(
    request: &OperationRequest,
    policy: &SessionPolicy,
    session: &mut Option<&mut basecraft_ops::Session>,
) -> Result<Option<(basecraft_ops::DocId, bool)>> {
    let target = match &request.target {
        None => None,
        Some(reference) => {
            let session = session.as_deref_mut().ok_or_else(|| {
                Error::Protocol("a target document needs an owning session".into())
            })?;
            Some(match reference {
                InputReference::File { path } => {
                    if !policy.document_write {
                        return Err(Error::Protocol(
                            "permission_denied: saving a target document needs writes".into(),
                        ));
                    }
                    (session.open(&policy.path(path, false)?)?, true)
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
                    if !session.is_open(id)
                        || session.generation(id)? != *generation
                        || session.revision(id)? != *revision
                    {
                        return Err(Error::Protocol(
                            "stale_revision: target document changed".into(),
                        ));
                    }
                    (id, false)
                }
            })
        }
    };
    Ok(target)
}

struct FrozenInputs {
    scratch: tempfile::TempDir,
    inputs: BTreeMap<String, Vec<String>>,
    sources: Vec<(PathBuf, String, String, &'static str, PathBuf)>,
    submission_plan: Option<basecraft_ops::blast::Plan>,
}

struct AllocatedRun {
    run: ArtifactRun,
    ledger_inputs: Vec<ArtifactRevision>,
    primary_input_owner: Option<PathBuf>,
    evidence: BTreeMap<String, Vec<InvocationInputEvidence>>,
}

impl PreparedOperation {
    /// Validations precede durable records. Only an accepted execution allocates
    /// storage; list/describe/denied calls never arrive here.
    pub fn prepare(
        operation: RegisteredOperation,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: String,
        mut session: Option<&mut basecraft_ops::Session>,
        registry: &crate::registry::Registry,
        captured: &[basecraft_ops::sanger_review::CapturedInput],
    ) -> Result<Self> {
        policy.check(&operation)?;
        let policy = policy.capture_ambient_credential(&operation)?;
        let lifecycle = crate::lifecycle::Lifecycle::for_operation(&operation)?;
        let workflow_registry = if operation.descriptor.host_workflow.is_some() {
            for stage in operation
                .descriptor
                .host
                .as_ref()
                .into_iter()
                .flat_map(|host| host.workflow_operations.values())
            {
                registry.resolve(stage, None)?;
            }
            Some(Arc::new(registry.clone()))
        } else {
            None
        };
        let mut inputs = prepare_inputs(&operation, &request, &policy, &mut session)?;
        let target = prepare_target(&request, &policy, &mut session)?;
        let services = operation.descriptor.host.as_ref();
        let query_preparation = services.and_then(|host| host.query_preparation);
        let input_diagnostics = lifecycle.verify_retained(
            &inputs.bindings.inputs,
            services.and_then(|host| host.retained_query_transformation.as_deref()),
        )?;
        let mut supplied_parameters = request.parameters.clone();
        if operation.package.origin == crate::registry::Origin::Bundled {
            for (parameter, setting) in services
                .into_iter()
                .flat_map(|host| &host.parameter_environment)
            {
                if supplied_parameters.get(parameter).is_none() {
                    let value = std::env::var(setting).map_err(|_| {
                        Error::Protocol(format!("Configure {setting} or supply {parameter}"))
                    })?;
                    supplied_parameters[parameter] = Value::String(value);
                }
            }
        }
        let parameters =
            contract::normalize_parameters(&operation.descriptor, &supplied_parameters)?;
        operation.package.verify()?;
        let mut frozen = freeze_inputs(&operation, query_preparation, &parameters, &mut inputs, captured)?;
        let AllocatedRun {
            run,
            ledger_inputs,
            primary_input_owner,
            evidence,
        } = allocate_run(&operation, &request, &policy, &inputs, &mut frozen, captured)?;
        let run_id = run.id().to_owned();
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
            inputs: frozen.inputs,
            outputs,
            cpu_threads: crate::compute::effective_allocation()?,
            cancel_file: run.artifact("cancel")?.display().to_string(),
            progress_file: run.artifact("progress")?.display().to_string(),
            input_evidence: evidence,
            legacy_parameters: BTreeMap::new(),
        };
        input_evidence::write_json(&run.artifact("descriptor.json")?, &operation.descriptor)?;
        input_evidence::write_json(&run.artifact("request.json")?, &request)?;
        let mut prepared = Self {
            lifecycle,
            input_diagnostics,
            input_record: inputs.input_record,
            sequence: inputs.sequence,
            document: inputs.document,
            operation,
            run,
            request,
            granted_parameters: std::sync::Mutex::new(parameters.clone()),
            parameters,
            context,
            policy,
            ledger_inputs,
            primary_input_owner,
            initiator,
            started: basecraft_platform::now_rfc3339(),
            workflow_registry,
            _scratch: frozen.scratch,
        };
        if let Some((target, save)) = target {
            let session = session.as_deref().ok_or_else(|| {
                Error::Protocol("a target document needs an owning session".into())
            })?;
            prepared.bind_target(session, target, save)?;
        }
        if prepared
            .operation
            .descriptor
            .host
            .as_ref()
            .is_some_and(|host| host.bootstrap_exports)
            && prepared.parameters["bootstrap_destination"] == "working"
            && prepared.parameters["bootstrap"].as_u64().unwrap_or(0) > 0
        {
            for suffix in [".nex", ".partial.nex"] {
                let member = format!("bootstrap-{}{suffix}", prepared.run.id());
                prepared
                    .policy
                    .path(&prepared.bootstrap_destination(Path::new(&member)), true)?;
            }
        }
        if prepared
            .operation
            .descriptor
            .inputs
            .values()
            .any(|binding| binding.retain)
        {
            prepared.retain_inputs()?;
        }
        Ok(prepared)
    }
}

fn freeze_inputs(
    operation: &RegisteredOperation,
    query_preparation: Option<QueryPreparation>,
    parameters: &Value,
    inputs: &mut PreparationInputs,
    captured: &[basecraft_ops::sanger_review::CapturedInput],
) -> Result<FrozenInputs> {
    let PreparationInputs {
        document,
        input_record,
        generated,
        accepted_hashes,
        bindings,
        ..
    } = inputs;
    let scratch = tempfile::Builder::new()
        .prefix("basecraft-operation-")
        .tempdir()?;
    let mut frozen_inputs = BTreeMap::new();
    let mut sources = Vec::new();
    let mut submission_queries = Vec::new();
    for (name, paths) in std::mem::take(&mut bindings.inputs) {
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
            let original_hash = captures::freeze(
                &source,
                &path,
                captured,
                (!accepted_hashes.contains_key(&source)).then_some(&mut *input_record),
            )?;
            if operation
                .descriptor
                .host
                .as_ref()
                .is_some_and(|host| host.verify_inputs.contains(&name))
            {
                let issues = basecraft_ops::records::verify(&source)?;
                if !issues.is_empty() {
                    return Err(Error::Protocol(format!(
                        "input evidence is not reusable: {}",
                        issues.join("; ")
                    )));
                }
            }

            if accepted_hashes
                .get(&source)
                .is_some_and(|accepted| accepted != &original_hash)
            {
                return Err(Error::Protocol(
                    "source changed since opening; reopen it before analysis".into(),
                ));
            }
            if matches!(query_preparation, Some(QueryPreparation::Blast)) {
                submission_queries.extend(basecraft_ops::blast::from_file_named(
                    &path,
                    &source.display().to_string(),
                )?);
            }
            let transformation = match binding.view {
                // A sequence file is frozen unchanged; its sealed snapshot
                // is a separate generated input recorded below.
                InputView::Bytes | InputView::SequenceSnapshot => "identity-v1",
                InputView::AlignedFasta | InputView::UngappedFasta => {
                    let basecraft_ops::files::Loaded::Alignment(alignment) =
                        basecraft_ops::files::load(&path)?
                    else {
                        return Err(Error::Protocol("input view requires an alignment".into()));
                    };
                    let mut output = Vec::new();
                    if binding.view == InputView::AlignedFasta {
                        alignment
                            .doc
                            .validate_aligned_input()
                            .map_err(|error| Error::Protocol(error.to_string()))?;
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
            };
            let submitted_hash = basecraft_io::hash_file(&path)?;
            sources.push((
                source,
                original_hash,
                submitted_hash,
                transformation,
                path.clone(),
            ));
            values.push(path.display().to_string());
        }
        for input in generated.get(&name).into_iter().flatten() {
            let path = scratch
                .path()
                .join(format!("{name}-document.{}", input.extension));
            std::fs::write(&path, &input.bytes)?;
            values.insert(input.position.min(values.len()), path.display().to_string());
        }
        frozen_inputs.insert(name.clone(), values);
    }
    if let Some(target) = document {
        target.input.1 = match &target.saved {
            Some(saved) => sources
                .iter()
                .find(|(source, ..)| source == saved)
                .map(|(_, _, submitted, ..)| submitted.clone()),
            None => generated
                .values()
                .flatten()
                .next()
                .map(|input| basecraft_io::hash_bytes(&input.bytes)),
        }
        .ok_or_else(|| Error::Protocol("document input was not submitted".into()))?;
    }
    let submission_plan = if matches!(query_preparation, Some(QueryPreparation::Blast)) {
        let plan = basecraft_ops::blast::prepare(&submission_queries, parameters)?;
        basecraft_ops::blast::authorize(&plan, parameters)?;
        Some(plan)
    } else {
        None
    };
    Ok(FrozenInputs {
        scratch,
        inputs: frozen_inputs,
        sources,
        submission_plan,
    })
}

fn allocate_run(
    operation: &RegisteredOperation,
    request: &OperationRequest,
    policy: &SessionPolicy,
    inputs: &PreparationInputs,
    frozen: &mut FrozenInputs,
    captured: &[basecraft_ops::sanger_review::CapturedInput],
) -> Result<AllocatedRun> {
    let sequence = &inputs.sequence;
    let generated = &inputs.generated;
    let FrozenInputs {
        inputs: frozen_inputs,
        sources,
        submission_plan,
        ..
    } = frozen;
    let run_id = ulid::Ulid::generate().to_string();
    // Runs and their input ledgers are named by the first exported file,
    // so a save conflict says what depends on the input.
    let exported_name = request
        .destinations
        .values()
        .find_map(|destination| Path::new(destination).file_name())
        .map(|name| name.to_string_lossy().into_owned());
    let anchor = policy
        .root
        .join(exported_name.as_deref().unwrap_or("operation"));
    let anchor = if policy.artifact_write {
        anchor
    } else {
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|error| Error::Protocol(error.to_string()))?;
        let directory = paths.state.join("operation-jobs").join(&run_id);
        std::fs::create_dir_all(&directory)?;
        directory.join("operation")
    };
    // Freeze/serialize before locking. Publication rechecks each exact original
    // hash and keeps ownership, exemptions and primary links under one guard.
    let sequence_payload = sequence
        .as_ref()
        .map(|target| target.snapshot.payload())
        .transpose()?;
    let submission_payload = submission_plan
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()?;
    let mutation = input_evidence::Mutation::begin()?;
    let run = ArtifactRun::create(&anchor, &run_id)?;
    let mut ledger_inputs = Vec::new();
    let mut upstream = Vec::new();
    let mut first_input = None;
    let mut evidence: BTreeMap<String, Vec<InvocationInputEvidence>> = BTreeMap::new();
    for (source, original_hash, submitted_hash, transformation, frozen) in sources.drain(..) {
        let ledger = captures::register(
            &mutation,
            &anchor,
            &source,
            &original_hash,
            captured,
            &format!("Input for {}", operation.id),
        )?;
        let mut record = mutation.read_input(ledger.directory())?;
        record.analysis_run = Some(input_evidence::relative(
            ledger.directory(),
            run.directory(),
        )?);
        record.submitted_hash = submitted_hash;
        record.submitted_size = std::fs::metadata(&frozen)?.len();
        record.transformation = transformation.into();
        if sequence.is_some() {
            // The run reads this file and proposes edits to the same
            // document; saving them must not count as replacing an
            // analysis input.
            record.proposal_target = Some(true);
        }
        mutation.write_input(ledger.directory(), &record)?;
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
        upstream.push(basecraft_protocol::input_records::UpstreamLink {
            bundle: (input_evidence::relative(run.directory(), ledger.directory())?).into(),
            id: (ledger.id()).to_owned(),
            hash: basecraft_io::hash_file(&manifest)?,
        });
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
            first_input = Some((record, ledger.directory().to_path_buf()));
        }
    }
    // Reuse a normal primary ledger and its established upstream links. Each
    // contributing original also owns its own normal reference ledger.
    let primary_input_owner = first_input.as_ref().map(|(_, owner)| owner.clone());
    if let Some((mut input, _)) = first_input {
        input.upstream = upstream;
        mutation.write_input(run.directory(), &input)?;
        // The save guard names a registered run by its run.json and treats a
        // missing one as damage, as for every other registered run. A run
        // without exports is named by its operation.
        let result = exported_name
            .clone()
            .unwrap_or_else(|| operation.id.clone());
        input_evidence::write_json(
            &run.artifact("run.json")?,
            &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
                schema_version: 1,
                id: run.id().into(),
                result: Some(result),
                status: basecraft_protocol::records::AnalysisRunStatus::Prepared,
                outputs: None,
                extra: BTreeMap::new(),
            })?,
        )?;
        mutation.register(&run)?;
    }
    let mut allocated = AllocatedRun {
        run,
        ledger_inputs,
        primary_input_owner,
        evidence,
    };
    allocated.document_inputs(&mutation, generated)?;
    if let (Some(target), Some(payload)) = (sequence, sequence_payload.as_deref()) {
        allocated.sequence_snapshot(&mutation, target, payload)?;
    }
    if let Some(payload) = submission_payload {
        allocated.query_preparation(&mutation, &payload, frozen_inputs)?;
    }
    Ok(allocated)
}

impl AllocatedRun {
    /// Retain exact generated bytes and their immutable publication identity.
    fn retain(&mut self, name: &str, role: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.run.artifact(name)?;
        basecraft_io::write_atomic(&path, |w| std::io::Write::write_all(w, bytes))?;
        self.ledger_inputs.push(ArtifactRevision {
            id: format!("{}:{role}", self.run.id()),
            logical_name: name.into(),
            hash: basecraft_io::hash_bytes(bytes),
            size: bytes.len() as u64,
            retained: Some(name.into()),
            reference: None,
        });
        Ok(path)
    }

    fn document_inputs(
        &mut self,
        mutation: &input_evidence::Mutation,
        generated: &BTreeMap<String, Vec<GeneratedInput>>,
    ) -> Result<()> {
        for (name, inputs) in generated {
            for input in inputs {
                let mut dependencies = sources_from_upstream(self.run.directory())?;
                dependencies.extend(input.dependencies.iter().cloned());
                mutation.link_generated_input(
                    &self.run,
                    &dependencies,
                    &input.bytes,
                    input.transformation,
                    "Document snapshot",
                )?;
                self.retain("submitted.fasta", "document", &input.bytes)?;
                self.evidence
                    .entry(name.clone())
                    .or_default()
                    .push(InvocationInputEvidence {
                        original_path: None,
                        source_hash: None,
                        submitted_hash: basecraft_io::hash_bytes(&input.bytes),
                        transformation: input.transformation.into(),
                        run_directory: self.run.directory().display().to_string(),
                    });
            }
        }
        Ok(())
    }

    fn sequence_snapshot(
        &mut self,
        mutation: &input_evidence::Mutation,
        target: &SequenceTarget,
        payload: &[u8],
    ) -> Result<()> {
        let mut dependencies = sources_from_upstream(self.run.directory())?;
        dependencies.extend(target.dependencies.iter().cloned());
        mutation.link_generated_input(
            &self.run,
            &dependencies,
            payload,
            "sequence-arrow-buffers-v1",
            "Sequence operation snapshot",
        )?;
        self.retain("snapshot.arrow-pair", "snapshot", payload)?;
        Ok(())
    }

    fn query_preparation(
        &mut self,
        mutation: &input_evidence::Mutation,
        payload: &[u8],
        inputs: &mut BTreeMap<String, Vec<String>>,
    ) -> Result<()> {
        // Retain the query before publishing its input ledger, as for the producer.
        let path = self.retain("query-preparation.json", "queries", payload)?;
        let dependencies = sources_from_upstream(self.run.directory())?;
        mutation.link_generated_input(
            &self.run,
            &dependencies,
            payload,
            "blast-query-preparation-v1",
            "Frozen BLAST submission",
        )?;
        inputs.insert("queries".into(), vec![path.display().to_string()]);
        Ok(())
    }
}
