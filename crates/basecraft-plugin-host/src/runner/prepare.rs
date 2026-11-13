//! Preparation stages: capture session bindings, freeze input bytes, then allocate ledgers.
use super::{
    Arc, ArtifactRevision, ArtifactRun, BTreeMap, DocumentTarget, Error, ExecutionFamily,
    InputReference, InputView, InvocationContext, InvocationInputEvidence, OperationRequest, Path,
    PathBuf, PreparedOperation, RegisteredOperation, Result, SequenceTarget, SessionPolicy, Value,
    captures, contract, records, sources_from_upstream,
};
use basecraft_protocol::operation::{Alphabet, QueryPreparation};

struct GeneratedInput {
    position: usize,
    bytes: Vec<u8>,
    transformation: &'static str,
    extension: &'static str,
    dependencies: Vec<PathBuf>,
}

/// Refuse a document whose alphabet the input does not accept, or that
/// basecraft can't tell yet (D12): `refusal` says why. Record the alphabet
/// for the plugin. One binding holds one alphabet.
fn check_alphabet(
    operation: &RegisteredOperation,
    name: &str,
    (found, refusal): (Alphabet, Option<String>),
    shown: &str,
    alphabets: &mut BTreeMap<String, Alphabet>,
) -> Result<()> {
    let accepted = &operation.descriptor.inputs[name].alphabets;
    if !accepted.is_empty()
        && let Some(refusal) = refusal
    {
        return Err(Error::Protocol(format!(
            "unsupported_input: {shown}: {refusal}"
        )));
    }
    if !accepted.is_empty() && !accepted.contains(&found) {
        let wanted: Vec<_> = accepted.iter().map(Alphabet::to_string).collect();
        return Err(Error::Protocol(format!(
            "unsupported_input: {} needs {} sequences, and {shown} holds {found} sequences",
            operation.descriptor.title,
            wanted.join(" or "),
        )));
    }
    if *alphabets.entry(name.into()).or_insert(found) != found {
        return Err(Error::Protocol(format!(
            "unsupported_input: {} needs the documents in {name} to share one alphabet",
            operation.descriptor.title
        )));
    }
    Ok(())
}

/// The input view of a session document's current revision.
fn document_view(
    session: &basecraft_ops::Session,
    doc: basecraft_ops::DocId,
    operation: &RegisteredOperation,
    name: &str,
    alphabets: &mut BTreeMap<String, Alphabet>,
) -> Result<(Vec<u8>, &'static str, &'static str)> {
    let mut bytes = Vec::new();
    let view = operation.descriptor.inputs[name].view;
    if session.is_alignment(doc) {
        let alignment = session.alignment(doc)?;
        let alphabet = (alignment.alphabet().value, alignment.alphabet_refusal());
        check_alphabet(operation, name, alphabet, "the open alignment", alphabets)?;
    }
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
    alphabets: BTreeMap<String, Alphabet>,
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
    let mut alphabets = BTreeMap::new();
    let bindings =
        if operation.descriptor.family == ExecutionFamily::SequenceProposal {
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
            let alphabet = (checked.alphabet().value, checked.alphabet_refusal());
            check_alphabet(
                operation,
                name,
                alphabet,
                "the open sequence",
                &mut alphabets,
            )?;
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
                        let id =
                            basecraft_ops::DocId(binding.doc.parse().map_err(|_| {
                                Error::Protocol("invalid session document ID".into())
                            })?);
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
                            let (bytes, transformation, extension) =
                                document_view(session, id, operation, &name, &mut alphabets)?;
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
        alphabets,
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

/// The request as the run folder records it: file inputs relative to the
/// folder, so the folder and its project can move together.
fn normalised_request(
    request: &OperationRequest,
    project: &Path,
    run: &Path,
) -> Result<OperationRequest> {
    let mut recorded = request.clone();
    for reference in recorded
        .inputs
        .values_mut()
        .flatten()
        .chain(&mut recorded.target)
    {
        if let InputReference::File { path } = reference {
            *path = records::relative(run, &project.join(&*path))?;
        }
    }
    Ok(recorded)
}

/// The request a run folder recorded, ready to run again: inputs resolved
/// against the folder's current place, pinned to the package and descriptor
/// the run used. A different installed runtime is refused, naming both locks.
pub fn recorded_request(
    run: &Path,
    registry: &crate::registry::Registry,
) -> Result<OperationRequest> {
    let mut request: OperationRequest = basecraft_protocol::records::decode(
        &basecraft_ops::records::read_record(&run.join("request.json"))?,
    )?;
    let record: basecraft_protocol::records::RunRecord = basecraft_protocol::records::decode(
        &basecraft_ops::records::read_record(&run.join("execution.json"))?,
    )?;
    let (Some(package_digest), Some(descriptor_digest)) =
        (record.package_digest, record.descriptor_digest)
    else {
        return Err(Error::Protocol(
            "the run record names no plugin package to run again".into(),
        ));
    };
    let pins = basecraft_protocol::operation::OperationPins {
        generation: registry.generation.clone(),
        package_digest,
        descriptor_digest,
    };
    let operation = registry.resolve(&request.operation, Some(&pins))?;
    let installed = operation
        .package
        .manifest
        .managed_runtime
        .as_ref()
        .map(|runtime| runtime.lock_digest.as_str());
    let recorded = record
        .environment
        .runtime
        .as_ref()
        .map(|r| r.lock_digest.as_str());
    if installed != recorded {
        return Err(Error::Protocol(format!(
            "plugin_changed: the run used runtime lock {}, and the installed runtime has {}",
            recorded.unwrap_or("none"),
            installed.unwrap_or("none")
        )));
    }
    request.pins = Some(pins);
    for reference in request
        .inputs
        .values_mut()
        .flatten()
        .chain(&mut request.target)
    {
        if let InputReference::File { path } = reference {
            let mut resolved = PathBuf::new();
            for part in run.join(&*path).components() {
                match part {
                    std::path::Component::ParentDir => {
                        resolved.pop();
                    }
                    std::path::Component::CurDir => {}
                    part => resolved.push(part),
                }
            }
            *path = resolved.display().to_string();
        }
    }
    Ok(request)
}

/// Where the host finds the user's contact email for services that ask for one.
pub const CONTACT_EMAIL: &str = "BASECRAFT_CONTACT_EMAIL";

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
            for stage in operation.descriptor.needs.workflow_stages.values() {
                registry.resolve(stage, None)?;
            }
            Some(Arc::new(registry.clone()))
        } else {
            None
        };
        let mut inputs = prepare_inputs(&operation, &request, &policy, &mut session)?;
        let target = prepare_target(&request, &policy, &mut session)?;
        let services = operation.descriptor.host.as_ref();
        let remote = services.and_then(|host| host.remote.as_ref());
        let query_preparation = remote.and_then(|remote| remote.query_preparation);
        let input_diagnostics = lifecycle.verify_retained(
            &inputs.bindings.inputs,
            remote.and_then(|remote| remote.retained_query_transformation.as_deref()),
        )?;
        let mut supplied_parameters = request.parameters.clone();
        if let Some(parameter) = &operation.descriptor.needs.contact_email
            && supplied_parameters.get(parameter).is_none()
        {
            let value = std::env::var(CONTACT_EMAIL).map_err(|_| {
                Error::Protocol(format!("Configure {CONTACT_EMAIL} or supply {parameter}"))
            })?;
            supplied_parameters[parameter] = Value::String(value);
        }
        let parameters =
            contract::normalize_parameters(&operation.descriptor, &supplied_parameters)?;
        operation.package.verify()?;
        let project = session
            .as_deref()
            .and_then(basecraft_ops::Session::project)
            .cloned();
        let mut frozen = freeze_inputs(
            &operation,
            query_preparation,
            &parameters,
            &mut inputs,
            captured,
            project.as_ref(),
        )?;
        let AllocatedRun {
            run,
            ledger_inputs,
            primary_input_owner,
            evidence,
        } = allocate_run(
            &operation,
            &request,
            &policy,
            &inputs,
            &mut frozen,
            captured,
            project.as_ref(),
        )?;
        let run_id = run.id().to_owned();
        let root = run.directory().to_path_buf();
        // Inside the run folder a path is relative to it; outside, absolute.
        let wire = |path: &Path| match path.strip_prefix(&root) {
            Ok(inside) if inside.as_os_str().is_empty() => ".".to_owned(),
            Ok(inside) => inside.display().to_string(),
            Err(_) => path.display().to_string(),
        };
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
            outputs.insert(name.clone(), wire(&path));
        }
        let wire_inputs = frozen
            .inputs
            .into_iter()
            .map(|(name, paths)| {
                let paths = paths.iter().map(|path| wire(Path::new(path))).collect();
                (name, paths)
            })
            .collect();
        let input_evidence = evidence
            .into_iter()
            .map(|(name, records)| {
                let records = records
                    .into_iter()
                    .map(|mut record| {
                        record.original_path =
                            record.original_path.map(|path| wire(Path::new(&path)));
                        record.run_directory = wire(Path::new(&record.run_directory));
                        record
                    })
                    .collect();
                (name, records)
            })
            .collect();
        let context = InvocationContext {
            schema: basecraft_protocol::plugin::PROTOCOL_VERSION,
            operation: operation.id.clone(),
            run_id,
            run_root: root.display().to_string(),
            inputs: wire_inputs,
            outputs,
            cpu_threads: crate::compute::effective_allocation()?,
            cancel_file: wire(&run.artifact("cancel")?),
            progress_file: wire(&run.artifact("progress")?),
            input_evidence,
            alphabets: inputs.alphabets.clone(),
        };
        records::write_json(&run.artifact("descriptor.json")?, &operation.descriptor)?;
        records::write_json(
            &run.artifact("request.json")?,
            &normalised_request(&request, &policy.root, &root)?,
        )?;
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
            tool: std::sync::Mutex::default(),
            parameters,
            context,
            policy,
            ledger_inputs,
            primary_input_owner,
            initiator,
            started: basecraft_platform::now_rfc3339(),
            workflow_registry,
            project,
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
            .outputs
            .get("artifacts")
            .is_some_and(|output| output.bootstrap_exports)
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
    project: Option<&basecraft_ops::project::Project>,
) -> Result<FrozenInputs> {
    let PreparationInputs {
        document,
        input_record,
        generated,
        accepted_hashes,
        bindings,
        alphabets,
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
                project,
            )?;
            if binding.verify {
                let issues = basecraft_ops::records::verify(&source, project)?;
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
                    let shown = source.file_name().map_or_else(
                        || source.display().to_string(),
                        |file| file.to_string_lossy().into_owned(),
                    );
                    let doc = &alignment.doc;
                    let alphabet = (doc.alphabet().value, doc.alphabet_refusal());
                    check_alphabet(operation, &name, alphabet, &shown, alphabets)?;
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
    project: Option<&basecraft_ops::project::Project>,
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
    // The run folder is basecraft's record and always goes under the
    // project's `.basecraft`; `artifact_write` governs the user's files only.
    let anchor = policy
        .root
        .join(exported_name.as_deref().unwrap_or("operation"));
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
    let mutation = records::Mutation::begin_in(project)?;
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
            project,
        )?;
        let mut record = mutation.read_input(ledger.directory())?;
        record.analysis_run = Some(records::relative(ledger.directory(), run.directory())?);
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
            bundle: (records::relative(run.directory(), ledger.directory())?).into(),
            id: (ledger.id()).to_owned(),
            hash: basecraft_io::hash_file(&manifest)?,
        });
        let mut reference = record
            .source
            .clone()
            .ok_or_else(|| Error::Protocol("missing source reference".into()))?;
        reference.path = records::relative(run.directory(), &source)?;
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
        run.write_run(
            basecraft_protocol::records::AnalysisRunStatus::Prepared,
            Some(result),
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
        self.ledger_inputs.push(records::retained_file(
            &self.run,
            role,
            name.into(),
            &path,
            basecraft_io::hash_bytes(bytes),
        )?);
        Ok(path)
    }

    fn document_inputs(
        &mut self,
        mutation: &records::Mutation,
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
        mutation: &records::Mutation,
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
        mutation: &records::Mutation,
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
