//! Registered-operation preparation, execution and validated finalization: the
//! one route every interface uses to run an analysis. Host workflows run their
//! stages through registered child jobs with the same lifecycle as other runs.
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

/// A session document an analysis read. Its owning session records the
/// completed run in that document's history.
pub struct DocumentTarget {
    pub doc: basecraft_ops::DocId,
    pub session: String,
    pub generation: u64,
    pub revision: u64,
    pub record: basecraft_ops::document_record::DocumentRecord,
    /// History key and submitted hash of the document's input.
    pub input: (String, String),
    /// The saved file the run read, or `None` for an unsaved snapshot.
    pub saved: Option<PathBuf>,
    /// The document's location, saved or not, for links back to it.
    pub path: PathBuf,
    /// Named by the request as its target rather than read as an input: a
    /// species workspace receives the run's hypotheses.
    pub receives_results: bool,
    /// Named by file path: the session opened it for this run and saves it
    /// afterwards. A target that is already open is left for its owner to save.
    pub save: bool,
}

/// An unsaved document's current contents, submitted as a generated input.
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
                .alignment(doc)
                .validate_aligned_input()
                .map_err(|error| Error::Protocol(error.to_string()))?;
            basecraft_ops::files::write_alignment(session.alignment(doc), &mut bytes)?;
            Ok((bytes, "canonical-fasta-v1", "afa"))
        }
        InputView::UngappedFasta if session.is_alignment(doc) => {
            basecraft_ops::files::write_ungapped(session.alignment(doc), &mut bytes)?;
            Ok((bytes, "ungapped-fasta-v1", "afa"))
        }
        InputView::Newick if session.is_tree(doc) => Ok((
            session.tree(doc).to_newick_with_rooting().into_bytes(),
            "canonical-newick-v1",
            "nwk",
        )),
        _ => Err(Error::Protocol(
            "unsupported_input: document kind does not match the input view".into(),
        )),
    }
}

pub struct PreparedOperation {
    pub(crate) lifecycle: crate::lifecycle::Lifecycle,
    pub(crate) input_diagnostics: Vec<String>,
    pub sequence: Option<SequenceTarget>,
    pub document: Option<DocumentTarget>,
    pub(crate) input_record: basecraft_ops::document_record::DocumentRecord,
    pub operation: RegisteredOperation,
    pub run: ArtifactRun,
    pub request: OperationRequest,
    pub parameters: Value,
    granted_parameters: std::sync::Mutex<Value>,
    pub context: InvocationContext,
    pub policy: SessionPolicy,
    pub ledger_inputs: Vec<ArtifactRevision>,
    pub initiator: String,
    started: String,
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
        mut session: Option<&mut basecraft_ops::Session>,
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
        let mut document = None;
        let mut input_record = basecraft_ops::document_record::DocumentRecord::empty("Inputs");
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
            let mut bindings = policy.resolve(&operation, &request)?;
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
                            || session.generation(id) != binding.generation
                            || session.revision(id) != binding.revision
                        {
                            return Err(Error::Protocol(
                                "stale_revision: input document changed".into(),
                            ));
                        }
                        if view == InputView::AlignedFasta && session.is_alignment(id) {
                            session
                                .alignment(id)
                                .validate_aligned_input()
                                .map_err(|error| Error::Protocol(error.to_string()))?;
                        }
                        input_record.include_input(&session.document_record(id)?);
                        let saved = session.saved_source(id)?;
                        if document.is_none() {
                            let key = session.doc_key(id);
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
                                path: session.root().join(session.doc_key(id)),
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
                                document_view(session, id, view)?;
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
                        let id =
                            basecraft_ops::DocId(doc.parse().map_err(|_| {
                                Error::Protocol("invalid session document ID".into())
                            })?);
                        if !session.is_open(id)
                            || session.generation(id) != *generation
                            || session.revision(id) != *revision
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
                if name == "cache" && operation.package.manifest.id == "basecraft.distance" {
                    let issues = basecraft_ops::provenance::verify(&source)?;
                    if !issues.is_empty() {
                        return Err(Error::Protocol(format!(
                            "distance cache evidence is not reusable: {}",
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
                if lifecycle.external() {
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
                if !accepted_hashes.contains_key(&source)
                    && let Ok(record) =
                        basecraft_ops::document_record::DocumentRecord::from_file(&source)
                {
                    input_record.include_input(&record);
                    if basecraft_io::hash_file(&source).map_err(basecraft_ops::Error::from)?
                        != original_hash
                    {
                        return Err(Error::Protocol(
                            "source changed while capturing its history".into(),
                        ));
                    }
                }
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
            for input in generated.get(&name).into_iter().flatten() {
                let path = scratch
                    .path()
                    .join(format!("{name}-document.{}", input.extension));
                std::fs::write(&path, &input.bytes)?;
                values.insert(input.position.min(values.len()), path.display().to_string());
            }
            frozen_inputs.insert(name.clone(), values);
        }
        if let Some(target) = &mut document {
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
        let submission_plan = if lifecycle.external() {
            let plan = basecraft_ops::blast::prepare(&submission_queries, &parameters)?;
            basecraft_ops::blast::authorize(&plan, &parameters)?;
            Some(plan)
        } else {
            None
        };
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
            record.extra.insert(
                "analysis_run".into(),
                json!(input_evidence::relative(
                    ledger.directory(),
                    run.directory()
                )?),
            );
            record.submitted_hash = submitted_hash;
            record.submitted_size = std::fs::metadata(&frozen)?.len();
            record.transformation = transformation.into();
            if sequence.is_some() {
                // The run reads this file and proposes edits to the same
                // document; saving them must not count as replacing an
                // analysis input.
                record
                    .extra
                    .insert(input_evidence::PROPOSAL_TARGET.into(), json!(true));
            }
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
            // The save guard names a registered run by its run.json and treats a
            // missing one as damage, as for every other registered run. A run
            // without exports is named by its operation.
            let result = exported_name
                .clone()
                .unwrap_or_else(|| operation.id.clone());
            input_evidence::write_json(
                &run.artifact("run.json")?,
                &json!({"schema_version":1,"id":run.id(),"result":result,"status":"prepared"}),
            )?;
            let _guard = input_evidence::lock()?;
            input_evidence::register(&run)?;
        }
        for (name, inputs) in &generated {
            for input in inputs {
                let mut dependencies = sources_from_upstream(run.directory())?;
                dependencies.extend(input.dependencies.iter().cloned());
                input_evidence::link_generated_input(
                    &run,
                    &dependencies,
                    &input.bytes,
                    input.transformation,
                    "Document snapshot",
                )?;
                // The store's format-neutral name for retained generated bytes,
                // which links, workspaces, Save As and replay look up.
                let retained = "submitted.fasta".to_owned();
                basecraft_io::write_atomic(&run.artifact(&retained)?, |w| {
                    std::io::Write::write_all(w, &input.bytes)
                })
                .map_err(basecraft_ops::Error::from)?;
                let submitted_hash = basecraft_io::hash_bytes(&input.bytes);
                evidence
                    .entry(name.clone())
                    .or_default()
                    .push(InvocationInputEvidence {
                        original_path: None,
                        source_hash: None,
                        submitted_hash: submitted_hash.clone(),
                        transformation: input.transformation.into(),
                        run_directory: run.directory().display().to_string(),
                    });
                ledger_inputs.push(ArtifactRevision {
                    id: format!("{}:document", run.id()),
                    logical_name: retained.clone(),
                    hash: submitted_hash,
                    size: input.bytes.len() as u64,
                    retained: Some(retained),
                    reference: None,
                });
            }
        }
        if let Some(target) = &sequence {
            let payload = target.snapshot.payload()?;
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
        let mut prepared = Self {
            lifecycle,
            input_diagnostics,
            input_record,
            sequence,
            document,
            operation,
            run,
            request,
            granted_parameters: std::sync::Mutex::new(parameters.clone()),
            parameters,
            context,
            policy,
            ledger_inputs,
            initiator,
            started: basecraft_platform::now_rfc3339(),
            workflow_registry,
            _scratch: scratch,
        };
        if let Some((target, save)) = target {
            let session = session.as_deref().ok_or_else(|| {
                Error::Protocol("a target document needs an owning session".into())
            })?;
            prepared.bind_target(session, target, save)?;
        }
        if prepared.operation.package.manifest.id == "basecraft.distance"
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

    /// Keep path-bearing results independent of temporary execution inputs.
    /// Use the existing input ledgers and preserve-copy policy for ancestry;
    /// submitted (possibly transformed) bytes remain in the normal run store.
    fn retain_inputs(&mut self) -> Result<()> {
        let retained_bindings: BTreeSet<_> = self
            .context
            .inputs
            .keys()
            .filter(|name| {
                self.operation
                    .descriptor
                    .inputs
                    .get(*name)
                    .is_some_and(|binding| binding.retain)
            })
            .cloned()
            .collect();
        for (binding, records) in &self.context.input_evidence {
            if !retained_bindings.contains(binding) {
                continue;
            }
            for record in records {
                if record.original_path.is_some() {
                    input_evidence::preserve_input(Path::new(&record.run_directory))?;
                }
            }
        }
        // The logical run's primary reference delegates to the preserved leaf,
        // just as Save As/preserve-copy does. Otherwise primary_matches would
        // incorrectly prefer the now-removed temporary original.
        let input_path = self.run.artifact("input.json")?;
        if input_path.is_file() {
            let mut input = input_evidence::read_input(self.run.directory())?;
            if let Some(source) = &input.source
                && let Some(owner) = self
                    .context
                    .input_evidence
                    .iter()
                    .filter(|(binding, _)| retained_bindings.contains(*binding))
                    .flat_map(|(_, records)| records)
                    .find(|record| record.source_hash.as_ref() == Some(&source.hash))
            {
                input.extra.insert(
                    "origin_run".into(),
                    json!(input_evidence::relative(
                        self.run.directory(),
                        Path::new(&owner.run_directory)
                    )?),
                );
                input.state = "preserved".into();
                input_evidence::write_json(&input_path, &input)?;
            }
        }
        for (binding, paths) in &mut self.context.inputs {
            if !retained_bindings.contains(binding) {
                continue;
            }
            for (index, path) in paths.iter_mut().enumerate() {
                let name = format!("submitted-{binding}-{index}");
                let retained = self.run.artifact(&name)?;
                if Path::new(path) == retained {
                    continue;
                }
                let hash = file_tool::freeze_file(Path::new(path), &retained)?;
                self.ledger_inputs.push(ArtifactRevision {
                    id: format!("{}:{name}", self.run.id()),
                    logical_name: name.clone(),
                    hash,
                    size: std::fs::metadata(&retained)?.len(),
                    retained: Some(name),
                    reference: None,
                });
                *path = retained.display().to_string();
            }
        }
        Ok(())
    }

    /// Inherit a workflow stage's original ledgers, avoiding a parent/child cycle.
    pub(crate) fn inherit_input_ancestry(&self, parent: &Path) -> Result<()> {
        let _guard = input_evidence::lock()?;
        let dependencies = sources_from_upstream(parent)?;
        let mut ledgers = BTreeSet::from([self.run.directory().to_path_buf()]);
        ledgers.extend(
            self.context
                .input_evidence
                .values()
                .flatten()
                .map(|input| PathBuf::from(&input.run_directory)),
        );
        for directory in ledgers {
            let mut input = input_evidence::read_input(&directory)?;
            input.extra.insert(
                "analysis_run".into(),
                json!(input_evidence::relative(&directory, parent)?),
            );
            let upstream = input
                .extra
                .entry("upstream".into())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| Error::Protocol("invalid stage ancestry".into()))?;
            for dependency in &dependencies {
                let bytes =
                    basecraft_ops::provenance::read_record(&dependency.join("manifest.json"))?;
                let publication: Publication = serde_json::from_slice(&bytes)?;
                if !upstream.iter().any(|entry| entry["id"] == publication.id) {
                    upstream.push(json!({"bundle":input_evidence::relative(&directory, &dependency.canonicalize()?)?,
                        "id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)}));
                }
            }
            input_evidence::write_json(&directory.join("input.json"), &input)?;
        }
        Ok(())
    }

    fn bootstrap_destination(&self, member: &Path) -> String {
        let directory = self
            .request
            .destinations
            .get("artifacts/tree.nwk")
            .map(Path::new)
            .and_then(Path::parent)
            .unwrap_or(Path::new(""));
        directory.join(member).display().to_string()
    }

    /// Merge child histories through the same submitted-record abstraction used
    /// for ordinary inputs. Stage publications remain separate immutable origins.
    pub(crate) fn publication_input_record(
        &self,
    ) -> Result<basecraft_ops::document_record::DocumentRecord> {
        let mut record = self.input_record.clone();
        let stages = self.run.artifact("stages.json")?;
        if stages.is_file() {
            let outcomes: Vec<OperationOutcome> =
                serde_json::from_slice(&basecraft_ops::provenance::read_record(&stages)?)?;
            for stage in outcomes {
                if stage.state != basecraft_protocol::operation::RuntimeState::Completed {
                    continue;
                }
                if let Some(artifact) = stage.artifacts.first() {
                    let publication =
                        basecraft_ops::provenance::inspect(Path::new(&artifact.path))?;
                    record.include_input(
                        &basecraft_ops::document_record::DocumentRecord::from_publication(
                            &publication,
                        ),
                    );
                }
            }
        }
        Ok(record)
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
                "tool":result.as_ref().ok().and_then(|r| r.tool.as_ref()),
                "execution":result.as_ref().ok().and_then(|r| r.execution.as_ref()),
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
        // Add the producer's support semantics before sealing the collection so
        // retained results, explicit exports, verification and repair agree.
        if let Some((name, kind)) = file_tool::producer_support(
            &self.operation.package.manifest.id,
            &self.operation.descriptor.implementation,
            &self.parameters,
        ) {
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
                            "support sidecar already exists in staged output".into(),
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
        // Visible copies are publication outputs too, so verifying one checks
        // its bytes; they come first, as the run's primary results.
        let mut exports = Vec::new();
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
            for (key, destination) in &self.request.destinations {
                let Some((member_role, Some(member))) =
                    crate::policy::destination_role(&self.operation, key)
                else {
                    continue;
                };
                if member_role != role {
                    continue;
                }
                let hash = expected.get(member).ok_or_else(|| {
                    Error::Protocol(format!(
                        "invalid_arguments: {role} has no member {}",
                        member.display()
                    ))
                })?;
                self.publish_member(role, &retained.join(member), hash, destination, outcome)?;
                exports.push((destination.clone(), retained.join(member), hash.clone()));
                // Component trees of a forest travel with their primary tree.
                if member == Path::new("tree.nwk") {
                    let stem = Path::new(destination)
                        .file_stem()
                        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                    for index in 1.. {
                        let component = PathBuf::from(format!("tree.component-{index}.nwk"));
                        let Some(hash) = expected.get(&component) else {
                            break;
                        };
                        let target = Path::new(destination)
                            .with_file_name(format!("{stem}.component-{index}.nwk"))
                            .display()
                            .to_string();
                        self.publish_member(
                            role,
                            &retained.join(&component),
                            hash,
                            &target,
                            outcome,
                        )?;
                        exports.push((target, retained.join(&component), hash.clone()));
                    }
                }
            }
            if role == "artifacts"
                && self.operation.package.manifest.id == "basecraft.distance"
                && self.parameters["bootstrap_destination"] == "working"
            {
                for (member, hash) in &expected {
                    let name = member.to_string_lossy();
                    if name.starts_with(&format!("bootstrap-{}", self.run.id()))
                        && name.ends_with(".nex")
                    {
                        let destination = self.bootstrap_destination(member);
                        self.publish_member(
                            role,
                            &retained.join(member),
                            hash,
                            &destination,
                            outcome,
                        )?;
                        exports.push((destination, retained.join(member), hash.clone()));
                    }
                }
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
        if let Some(destination) = self
            .request
            .destinations
            .get(crate::policy::WORKSPACE_DESTINATION)
            .filter(|_| {
                crate::policy::is_workspace_destination(
                    &self.operation,
                    crate::policy::WORKSPACE_DESTINATION,
                )
            })
        {
            let path = self.policy.root.join(destination);
            if let Some(artifact) = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.path == path.display().to_string())
            {
                if basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?
                    != artifact.digest
                {
                    return Err(Error::Protocol(
                        "published workspace changed; repair refused".into(),
                    ));
                }
            } else {
                let published = self.publish_workspace(destination)?;
                outcome.artifacts.push(PublishedArtifact {
                    role: crate::policy::WORKSPACE_DESTINATION.into(),
                    path: published.display().to_string(),
                    digest: basecraft_io::hash_file(&published)
                        .map_err(basecraft_ops::Error::from)?,
                });
                self.persist(outcome)?;
            }
            let artifact = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.path == path.display().to_string())
                .ok_or_else(|| Error::Protocol("missing workspace publication receipt".into()))?;
            let name = path
                .file_name()
                .ok_or_else(|| Error::Protocol("workspace has no name".into()))?;
            exports.push((
                destination.clone(),
                self.run.artifact(&name.to_string_lossy())?,
                artifact.digest.clone(),
            ));
        }
        let mut exported = Vec::new();
        for (destination, retained, hash) in &exports {
            let path = self.policy.root.join(destination);
            exported.push(ArtifactRevision {
                id: format!("{}:export-{}", self.run.id(), exported.len()),
                logical_name: input_evidence::relative(&self.policy.root, &path)?,
                hash: hash.clone(),
                size: std::fs::metadata(&path)?.len(),
                retained: Some(input_evidence::relative(self.run.directory(), retained)?),
                reference: None,
            });
            // An exported tree's support semantics and alignment link travel
            // with it, so a complete export keeps them.
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
            {
                for companion in [
                    basecraft_ops::files::tree_sidecar(&path),
                    basecraft_ops::links::sidecar(&path),
                ] {
                    if companion.is_file() && companion.starts_with(self.run.directory()) {
                        exported.push(ArtifactRevision {
                            id: format!("{}:export-{}", self.run.id(), exported.len()),
                            logical_name: input_evidence::relative(&self.policy.root, &companion)?,
                            hash: basecraft_io::hash_file(&companion)
                                .map_err(basecraft_ops::Error::from)?,
                            size: std::fs::metadata(&companion)?.len(),
                            retained: Some(input_evidence::relative(
                                self.run.directory(),
                                &companion,
                            )?),
                            reference: None,
                        });
                    }
                }
            }
        }
        publication_outputs.splice(0..0, exported);
        outcome.data = result.data.as_deref().cloned();
        let previous = std::fs::read(self.run.directory().join("manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Publication>(&bytes).ok());
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
            events: previous
                .as_ref()
                .map_or_else(Vec::new, |p| p.events.clone()),
            effective_edits: previous
                .as_ref()
                .map_or_else(Vec::new, |p| p.effective_edits.clone()),
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        basecraft_ops::provenance::publish_operation(self.run.directory(), &publication)?;
        if self.document.is_none() && self.sequence.is_none() {
            let (actor, params) = self.tool_activity(outcome)?;
            let activity = basecraft_protocol::LogEntry {
                v: basecraft_protocol::ops::LOG_VERSION,
                id: self.run.id().into(),
                t: self.started.clone(),
                host: String::new(),
                session: publication.session.clone(),
                actor,
                doc: self.operation.id.clone(),
                op: basecraft_protocol::Op::ToolRun {
                    plugin: self.operation.package.manifest.id.clone(),
                    operation: self.operation.descriptor.implementation.clone(),
                    params,
                    inputs: self
                        .ledger_inputs
                        .iter()
                        .map(|input| input.logical_name.clone())
                        .collect(),
                    outputs: publication
                        .outputs
                        .iter()
                        .map(|output| output.logical_name.clone())
                        .collect(),
                },
                result: basecraft_protocol::OpResult {
                    hashes: self
                        .ledger_inputs
                        .iter()
                        .chain(&publication.outputs)
                        .map(|artifact| (artifact.logical_name.clone(), artifact.hash.clone()))
                        .collect(),
                    feature: None,
                },
                state: basecraft_protocol::DocState {
                    len: 0,
                    features: 0,
                    rows: None,
                },
            };
            self.publication_input_record()?
                .publish_operation_activity(&self.run, &activity)?;
        }
        for artifact in &outcome.artifacts {
            self.run.bind(Path::new(&artifact.path))?;
        }
        Ok(())
    }
    /// Export one member of a published collection to a new file in the
    /// project, recording it in the outcome. Idempotent for a recorded export.
    fn publish_member(
        &self,
        role: &str,
        retained: &Path,
        hash: &str,
        destination: &str,
        outcome: &mut OperationOutcome,
    ) -> Result<()> {
        let destination_path = self.policy.root.join(destination);
        let path = destination_path.display().to_string();
        let intent = self.run.artifact(&format!(
            "member-{}-export.json",
            basecraft_io::hash_bytes(destination_path.as_os_str().as_encoded_bytes())
                .trim_start_matches("b3:")
        ))?;
        if outcome
            .artifacts
            .iter()
            .any(|a| a.path == path && a.digest == hash)
        {
            verify_member_receipt(&destination_path, hash, &intent)?;
        } else {
            if destination_path.exists() {
                verify_member_receipt(&destination_path, hash, &intent)?;
            } else {
                let destination = self.policy.path(destination, true)?;
                export_member(retained, &destination, hash, &intent)?;
            }
            outcome.artifacts.push(PublishedArtifact {
                role: role.into(),
                path,
                digest: hash.into(),
            });
            self.persist(outcome)?;
        }
        if destination_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
        {
            self.finalise_exported_tree(&destination_path)?;
        }
        Ok(())
    }

    /// Record this run in `doc`'s history although `doc` is not one of its
    /// inputs, as for a species workspace analysing a file it references.
    fn bind_target(
        &mut self,
        session: &basecraft_ops::Session,
        doc: basecraft_ops::DocId,
        save: bool,
    ) -> Result<()> {
        if self.document.is_some() || self.sequence.is_some() {
            return Err(Error::Protocol(
                "run already records into its input document".into(),
            ));
        }
        if !session.is_open(doc) {
            return Err(Error::Protocol("unknown target document".into()));
        }
        let evidence = self
            .context
            .input_evidence
            .values()
            .flatten()
            .next()
            .ok_or_else(|| Error::Protocol("target run has no input".into()))?;
        let key = evidence
            .original_path
            .clone()
            .unwrap_or_else(|| "generated input".into());
        let path = session.root().join(session.doc_key(doc));
        self.document = Some(DocumentTarget {
            doc,
            session: session.id().into(),
            generation: session.generation(doc),
            revision: session.revision(doc),
            record: session.document_record(doc)?,
            input: (key, evidence.submitted_hash.clone()),
            saved: session.saved_source(doc)?.map(|(path, _, _)| path),
            path,
            receives_results: true,
            save,
        });
        Ok(())
    }

    /// Publish the species workspace in a validated run's result as a new
    /// SPART file in the project.
    fn publish_workspace(&self, destination: &str) -> Result<PathBuf> {
        let validated: Vec<(String, PathBuf, String)> =
            serde_json::from_slice(&basecraft_ops::provenance::read_record(
                &self.run.artifact("validated-artifacts.json")?,
            )?)?;
        let name = self.species_result_name();
        let (role, staged, hash) = validated
            .iter()
            .find(|(_, path, _)| path.file_name().is_some_and(|file| file == name.as_str()))
            .ok_or_else(|| Error::Protocol("run returned no species result".into()))?;
        let retained = if self.operation.descriptor.host_workflow.is_some() {
            staged.clone()
        } else {
            self.run.artifact(role)?.join(
                staged
                    .strip_prefix(
                        self.context
                            .outputs
                            .get(role)
                            .ok_or_else(|| Error::Protocol("undeclared output role".into()))?,
                    )
                    .map_err(|_| Error::Protocol("result escapes allocation".into()))?,
            )
        };
        if basecraft_io::hash_file(&retained).map_err(basecraft_ops::Error::from)? != *hash {
            return Err(Error::Protocol("retained species result changed".into()));
        }
        let value: Value = serde_json::from_slice(&std::fs::read(&retained)?)?;
        if value["workspace"].is_null() {
            return Err(Error::Protocol("result holds no species workspace".into()));
        }
        let work = serde_json::from_value(value["workspace"].clone())?;
        let target = self.policy.path(destination, true)?;
        basecraft_ops::delimitation::publish_workspace_to(&target, &work, &self.run)?;
        Ok(target)
    }

    /// The file holding a delimitation run's result: the descriptor's primary
    /// output, or the fit stage's report for a host alignment workflow.
    pub fn species_result_name(&self) -> String {
        self.operation
            .descriptor
            .legacy_adapter
            .as_ref()
            .and_then(|adapter| adapter.outputs.get("output"))
            .map_or_else(
                || "fit.json".into(),
                |output| {
                    Path::new(&output.path)
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                },
            )
    }

    /// An exported tree gets what the retained member has: its run binding,
    /// producer support semantics and the link to the alignment it came from.
    fn finalise_exported_tree(&self, tree: &Path) -> Result<()> {
        self.run.bind(tree)?;
        if let Some((name, kind)) = file_tool::producer_support(
            &self.operation.package.manifest.id,
            &self.operation.descriptor.implementation,
            &self.parameters,
        ) {
            file_tool::write_support_sidecar(tree, name, kind)?;
        }
        let Some(frozen) = self
            .context
            .inputs
            .get("alignment")
            .and_then(|values| values.first())
        else {
            return Ok(());
        };
        // A target that only receives the run, such as a species workspace,
        // is not the alignment; the tree then links to the alignment input.
        let (source, version) = match self
            .document
            .as_ref()
            .filter(|target| !target.receives_results)
        {
            Some(target) => (target.path.clone(), target.generation),
            None => (
                self.context
                    .input_evidence
                    .get("alignment")
                    .and_then(|values| values.first())
                    .and_then(|evidence| evidence.original_path.as_deref())
                    .map(PathBuf::from)
                    .ok_or_else(|| Error::Protocol("tree alignment source unknown".into()))?,
                0,
            ),
        };
        basecraft_ops::links::AlignmentLink::create_referenced(
            tree,
            &source,
            Path::new(frozen),
            version,
            &self.run,
        )?;
        Ok(())
    }

    pub(crate) fn tool_activity(
        &self,
        outcome: &OperationOutcome,
    ) -> Result<(basecraft_protocol::Actor, Value)> {
        let manifest = &self.operation.package.manifest;
        // The external program and its process evidence, when the plugin reported
        // them, name the software in history and methods text.
        let execution: serde_json::Value = std::fs::read(self.run.artifact("execution.json")?)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let tool = serde_json::from_value::<basecraft_protocol::plugin::ToolInfo>(
            execution["tool"].clone(),
        )
        .ok();
        let actor = tool.as_ref().map_or_else(
            || basecraft_protocol::Actor::Plugin {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
            },
            |tool| basecraft_protocol::Actor::Tool {
                tool: tool.name.clone(),
                version: tool.version.clone(),
            },
        );
        let mut params = json!({"requested":self.request.parameters,"effective":outcome.effective_parameters,
        "operation":self.operation.id,"pins":outcome.pins,"initiator":self.initiator,"run_id":outcome.run_id});
        if !execution["execution"].is_null() {
            params["_execution"] = execution["execution"].clone();
        }
        if !execution["tool"].is_null() {
            params["_observed_tool"] = execution["tool"].clone();
        }
        Ok((actor, params))
    }

    pub fn effective_parameters(&self) -> Value {
        self.granted_parameters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn persist(&self, outcome: &OperationOutcome) -> Result<()> {
        let runtime = self.run.artifact("runtime.json")?;
        input_evidence::write_json(&runtime, outcome)?;
        self.write_task_index(outcome, &runtime)
    }

    /// The Tasks view lists runs from small index records in application
    /// state. This one only points at the run's own `runtime.json`, which
    /// stays the authoritative record.
    fn write_task_index(&self, outcome: &OperationOutcome, runtime: &Path) -> Result<()> {
        use basecraft_protocol::operation::RuntimeState;
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|error| Error::Protocol(error.to_string()))?;
        let directory = paths.state.join("jobs").join(self.run.id());
        std::fs::create_dir_all(&directory)?;
        let status = match outcome.state {
            RuntimeState::Completed => "completed",
            RuntimeState::Failed => "failed",
            RuntimeState::Cancelled => "cancelled",
            RuntimeState::Partial => "partial",
            RuntimeState::Interrupted => "interrupted",
            RuntimeState::Queued => "queued",
            RuntimeState::Running => "running",
            RuntimeState::Cancelling => "cancelling",
            RuntimeState::Finalizing => "finalizing",
        };
        let source = self.document.as_ref().map_or_else(
            || {
                self.context
                    .input_evidence
                    .values()
                    .flatten()
                    .find_map(|evidence| evidence.original_path.clone())
                    .unwrap_or_default()
            },
            |target| target.path.display().to_string(),
        );
        let outputs: serde_json::Map<String, Value> = outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), json!(artifact.digest)))
            .collect();
        let index = json!({
            "schema_version": 1, "id": self.run.id(), "status": status,
            "owner_instance": basecraft_platform::process_instance(),
            "started": self.started,
            "ended": outcome.state.terminal().then(basecraft_platform::now_rfc3339),
            "plugin": {"id": self.operation.package.manifest.id, "version": self.operation.package.manifest.version},
            "operation": self.operation.id, "effective_parameters": outcome.effective_parameters,
            "initiator": self.initiator,
            "source": {"path": source},
            "outputs": outputs,
            "diagnostics": outcome.errors.first().map(|error| error.message.clone()),
            "runtime_record": runtime,
        });
        input_evidence::write_json(&directory.join("manifest.json"), &index)?;
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
/// A pre-rename inode receipt proves which file this run created. Matching
/// bytes alone never authorize adopting somebody else's existing file.
fn verify_member_receipt(destination: &Path, hash: &str, intent: &Path) -> Result<()> {
    let receipt: Value = serde_json::from_slice(&basecraft_ops::provenance::read_record(intent)?)?;
    let metadata = std::fs::symlink_metadata(destination)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || receipt["schema"] != 1
        || receipt["destination"] != json!(destination)
        || receipt["hash"] != hash
        || receipt["device"].as_u64() != Some(metadata.dev())
        || receipt["inode"].as_u64() != Some(metadata.ino())
        || receipt["size"].as_u64() != Some(metadata.len())
        || basecraft_io::hash_file(destination).map_err(basecraft_ops::Error::from)? != hash
    {
        return Err(Error::Protocol(
            "output_conflict: member ownership or bytes changed; repair refused".into(),
        ));
    }
    Ok(())
}

/// Copy one validated collection member to a new file; never replaces one.
fn export_member(source: &Path, destination: &Path, hash: &str, intent: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Protocol("missing destination parent".into()))?;
    let pending = tempfile::Builder::new()
        .prefix(".basecraft-export-")
        .tempfile_in(parent)?;
    std::fs::copy(source, pending.path())?;
    if basecraft_io::hash_file(pending.path()).map_err(basecraft_ops::Error::from)? != hash {
        return Err(Error::Protocol("member changed during export".into()));
    }
    pending.as_file().sync_all()?;
    let metadata = pending.as_file().metadata()?;
    input_evidence::write_json(
        intent,
        &json!({"schema":1,"destination":destination,
        "hash":hash,"device":metadata.dev(),"inode":metadata.ino(),"size":metadata.len()}),
    )?;
    basecraft_platform::atomic::rename_noreplace(pending.path(), destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Protocol("output_conflict: destination exists".into())
        } else {
            error.into()
        }
    })?;
    std::fs::File::open(parent)?.sync_all()?;
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
            &json!({"schema":1,"workflow":method,"initiator":self.initiator,"executor":"basecraft.host","requested_parameters":self.request.parameters,"effective_parameters":self.effective_parameters(),"package_digest":self.operation.package.digest,"descriptor_digest":self.operation.descriptor_digest,"process_instance":basecraft_platform::process_instance()}),
        )?;
        execution
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
