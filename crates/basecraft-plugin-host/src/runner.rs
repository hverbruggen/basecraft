//! Registered-operation preparation, execution and validated finalization: the
//! one route every interface uses to run an analysis. Host workflows run their
//! stages through registered child jobs with the same lifecycle as other runs.
mod captures;
mod execute;
mod finalize;
mod prepare;
pub use prepare::recorded_request;

use basecraft_ops::records::AlignmentLinkExt;
use basecraft_platform::files::link_count;
use basecraft_protocol::publication_records::{CollectionExportIntent, CollectionIntent};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use basecraft_ops::{
    records,
    records::{ArtifactDestination, ArtifactRun},
};
use basecraft_protocol::{
    operation::{
        ExecutionFamily, InputReference, InputView, InvocationContext, InvocationInputEvidence,
        MAX_ARTIFACTS, MAX_NESTING, OperationOutcome, OperationRequest, PluginArtifact,
        PublishedArtifact,
    },
    plugin::InvokeResult,
    provenance::{ArtifactRevision, Publication},
    records::{JobIndexPlugin, JobIndexSource, LocalJobIndex, RUN_RECORD_SCHEMA, RunRecord},
};
use serde_json::Value;

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
    let (execution, writer) = basecraft_protocol::records::decode_written::<RunRecord>(
        &basecraft_ops::records::read_record(&directory.join("execution.json"))?,
    )?;
    if execution.schema != RUN_RECORD_SCHEMA {
        return Err(Error::Protocol(basecraft_protocol::records::other_format(
            &writer,
            execution.schema,
            RUN_RECORD_SCHEMA,
        )));
    }
    if execution.operation != query.operation
        || execution.package_digest.as_ref() != Some(&query.pins.package_digest)
        || execution.descriptor_digest.as_ref() != Some(&query.pins.descriptor_digest)
        || execution.effective_parameters != query.effective_parameters
    {
        return Err(Error::Protocol(
            "execution specification differs from receipt".into(),
        ));
    }
    if execution.inputs.len() != query.inputs.len()
        || query.inputs.iter().any(|(role, expected)| {
            execution.inputs.get(role).is_none_or(|actual| {
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
    let bundle = basecraft_ops::records::bundle_for_result(first, None);
    if bundle.canonicalize()? != directory.canonicalize()? {
        return Err(Error::Protocol(
            "retained artifact is bound to a different run".into(),
        ));
    }
    let issues = basecraft_ops::records::verify(first, None)?;
    if !issues.is_empty() {
        return Err(Error::Protocol(format!(
            "completed publication is not reusable: {}",
            issues.join("; ")
        )));
    }
    let publication = basecraft_ops::records::inspect(first, None)?;
    for artifact in &outcome.artifacts {
        let path = Path::new(&artifact.path);
        if !path.canonicalize()?.starts_with(directory.canonicalize()?)
            || path.symlink_metadata()?.file_type().is_symlink()
            || basecraft_io::hash_file(path)? != artifact.digest
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
        } // Input ledgers and domain runs have no runtime receipt.
        let candidate = (|| -> Result<Option<VerifiedOperationResult>> {
            let (outcome, writer) = basecraft_protocol::records::decode_written::<OperationOutcome>(
                &basecraft_ops::records::read_record(&receipt)?,
            )?;
            if outcome.schema != 1 {
                return Err(Error::Protocol(basecraft_protocol::records::other_format(
                    &writer,
                    outcome.schema,
                    1,
                )));
            }
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
    pub record: basecraft_ops::records::DocumentRecord,
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
pub struct PreparedOperation {
    pub(crate) lifecycle: crate::lifecycle::Lifecycle,
    pub(crate) input_diagnostics: Vec<String>,
    pub sequence: Option<SequenceTarget>,
    pub document: Option<DocumentTarget>,
    pub(crate) input_record: basecraft_ops::records::DocumentRecord,
    pub operation: RegisteredOperation,
    pub run: ArtifactRun,
    pub request: OperationRequest,
    pub parameters: Value,
    granted_parameters: std::sync::Mutex<Value>,
    /// The external tool the run record names, once it is written.
    tool: std::sync::Mutex<Option<basecraft_protocol::plugin::ToolInfo>>,
    pub context: InvocationContext,
    pub policy: SessionPolicy,
    pub ledger_inputs: Vec<ArtifactRevision>,
    /// Exact original ledger from which the primary reference was prepared.
    primary_input_owner: Option<PathBuf>,
    pub initiator: String,
    started: String,
    workflow_registry: Option<Arc<crate::registry::Registry>>,
    /// The session's project: the history of results and the analysis index
    /// live at its root.
    pub(crate) project: Option<basecraft_ops::project::Project>,
    _scratch: tempfile::TempDir,
}

impl PreparedOperation {
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
        // Preserve leaves and delegate the primary ledger under one boundary:
        // a cooperating save must not invalidate the primary before its rewrite.
        let mutation = records::Mutation::begin_in(self.project.as_ref())?;
        for (binding, records) in &self.context.input_evidence {
            if !retained_bindings.contains(binding) {
                continue;
            }
            for record in records {
                if record.original_path.is_some() {
                    mutation.preserve_input(&self.run_path(&record.run_directory))?;
                }
            }
        }
        // The logical run's primary reference delegates to the preserved leaf,
        // just as Save As/preserve-copy does. Otherwise primary_matches would
        // incorrectly prefer the now-removed temporary original.
        let input_path = self.run.artifact("input.json")?;
        if input_path.is_file() {
            let mut input = mutation.read_input(self.run.directory())?;
            if let Some(source) = &input.source
                && let Some(owner) = self
                    .context
                    .input_evidence
                    .iter()
                    .filter(|(binding, _)| retained_bindings.contains(*binding))
                    .flat_map(|(_, records)| records)
                    .find(|record| {
                        self.primary_input_owner.as_deref()
                            == Some(self.run_path(&record.run_directory).as_path())
                            && record.source_hash.as_ref() == Some(&source.hash)
                    })
            {
                input.origin_run = Some(records::relative(
                    self.run.directory(),
                    &self.run_path(&owner.run_directory),
                )?);
                input.state = basecraft_protocol::input_records::EvidenceState::Preserved;
                mutation.write_input(self.run.directory(), &input)?;
            }
        }
        drop(mutation);
        let root = self.run.directory().to_path_buf();
        for (binding, paths) in &mut self.context.inputs {
            if !retained_bindings.contains(binding) {
                continue;
            }
            for (index, path) in paths.iter_mut().enumerate() {
                let name = format!("submitted-{binding}-{index}");
                let retained = self.run.artifact(&name)?;
                if root.join(&*path) == retained {
                    continue;
                }
                let hash = file_tool::freeze_file(&root.join(&*path), &retained)?;
                self.ledger_inputs.push(records::retained_file(
                    &self.run,
                    &name,
                    name.clone(),
                    &retained,
                    hash,
                )?);
                *path = name;
            }
        }
        Ok(())
    }

    /// Inherit a workflow stage's original ledgers, avoiding a parent/child cycle.
    pub(crate) fn inherit_input_ancestry(&self, parent: &Path) -> Result<()> {
        let mutation = records::Mutation::begin_in(self.project.as_ref())?;
        let dependencies = sources_from_upstream(parent)?;
        let mut ledgers = BTreeSet::from([self.run.directory().to_path_buf()]);
        ledgers.extend(
            self.context
                .input_evidence
                .values()
                .flatten()
                .map(|input| self.run_path(&input.run_directory)),
        );
        for directory in ledgers {
            let mut input = mutation.read_input(&directory)?;
            input.analysis_run = Some(records::relative(&directory, parent)?);
            let upstream = &mut input.upstream;
            for dependency in &dependencies {
                let bytes = basecraft_ops::records::read_record(&dependency.join("manifest.json"))?;
                let publication: Publication = basecraft_protocol::records::decode(&bytes)?;
                if !upstream.iter().any(|entry| entry.id == publication.id) {
                    upstream.push(basecraft_protocol::input_records::UpstreamLink {
                        bundle: (records::relative(&directory, &dependency.canonicalize()?)?)
                            .into(),
                        id: publication.id.clone(),
                        hash: basecraft_io::hash_bytes(&bytes),
                    });
                }
            }
            mutation.write_input(&directory, &input)?;
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
    ) -> Result<basecraft_ops::records::DocumentRecord> {
        let mut record = self.input_record.clone();
        let stages = self.run.artifact("stages.json")?;
        if stages.is_file() {
            let outcomes: Vec<OperationOutcome> =
                serde_json::from_slice(&basecraft_ops::records::read_record(&stages)?)?;
            for stage in outcomes {
                if stage.state != basecraft_protocol::operation::RuntimeState::Completed {
                    continue;
                }
                if let Some(artifact) = stage.artifacts.first() {
                    let publication = basecraft_ops::records::inspect(
                        Path::new(&artifact.path),
                        self.project.as_ref(),
                    )?;
                    record.include_input(
                        &basecraft_ops::records::DocumentRecord::from_publication(&publication),
                    );
                }
            }
        }
        Ok(record)
    }

    pub fn validate_result(&self, result: &InvokeResult) -> Result<Vec<(String, PathBuf, String)>> {
        contract::BoundedSchema::compile(&self.operation.descriptor.result)?
            .validate(result.data.as_deref().unwrap_or(&Value::Null))?;
        if !result.ops.is_empty()
            && (self.sequence.is_none() || !self.operation.descriptor.effects.document_write)
        {
            return Err(Error::Protocol("file operation returned proposals".into()));
        }
        if result.ops.len() > 4096 {
            return Err(Error::Protocol("proposal count exceeds 4096".into()));
        }
        if result.artifacts.len() > MAX_ARTIFACTS {
            return Err(Error::Protocol("artifact count limit exceeded".into()));
        }
        if self.lifecycle.external() {
            self.submission_identity(result)?;
            let receipt = &self.run_path(&self.context.outputs["receipt"]);
            if !receipt.try_exists()? {
                records::write_plain_json(receipt, result.data.as_deref().unwrap_or(&Value::Null))?;
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
            let root = self.run_path(&self.context.outputs[&artifact.role]);
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
                || link_count(&path)? != 1
                || !paths.insert(path.clone())
            {
                return Err(Error::Protocol(
                    "artifact is not a unique ordinary staging file".into(),
                ));
            }
            *counts.entry(artifact.role.clone()).or_default() += 1;
            let hash = basecraft_io::hash_file(&path)?;
            validated.push((artifact.role.clone(), path, hash));
        }
        // Add what the producer states about its results before sealing the
        // collection so retained results, explicit exports, verification and
        // repair agree.
        let support = file_tool::producer_support(&self.operation.descriptor, &self.parameters);
        let stated: Vec<_> = validated
            .iter()
            .filter(|(_, path, _)| file_tool::is_tree(path))
            .filter_map(|(role, path, _)| {
                let binding = &self.operation.descriptor.outputs[role];
                let primary = binding
                    .primary
                    .as_ref()
                    .map_or(!binding.collection, |member| {
                        *path == self.run_path(&self.context.outputs[role]).join(member)
                    });
                let lengths = primary
                    .then(|| file_tool::stated_branch_lengths(binding, &self.parameters))
                    .flatten();
                let support = support.filter(|_| role == "artifacts");
                (support.is_some() || lengths.is_some())
                    .then(|| (role.clone(), path.clone(), lengths, support))
            })
            .collect();
        for (role, path, lengths, support) in stated {
            let sidecar = basecraft_ops::files::tree_sidecar(&path);
            match std::fs::symlink_metadata(&sidecar) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(Error::Protocol(
                        "companion already exists in staged output".into(),
                    ));
                }
                Err(error) => return Err(error.into()),
            }
            if let Some(sidecar) = file_tool::write_tree_companion(&path, support, lengths)? {
                if !paths.insert(sidecar.clone()) {
                    return Err(Error::Protocol("duplicate companion".into()));
                }
                *counts.entry(role.clone()).or_default() += 1;
                let hash = basecraft_io::hash_file(&sidecar)?;
                // Bound tree reads use the run-root companion; the collection
                // member carries the same facts when exported.
                let hidden = self.run.artifact(
                    sidecar
                        .file_name()
                        .and_then(|file| file.to_str())
                        .ok_or_else(|| Error::Protocol("companion filename".into()))?,
                )?;
                basecraft_io::write_atomic(&hidden, |writer| {
                    let mut input = std::fs::File::open(&sidecar)?;
                    std::io::copy(&mut input, writer)?;
                    Ok(())
                })?;
                validated.push((role, sidecar, hash));
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
            let plan: Value = serde_json::from_slice(&basecraft_ops::records::read_record(
                &self.run.artifact("query-preparation.json")?,
            )?)?;
            self.lifecycle.validate_receipt(
                result,
                &self.run_path(&self.context.outputs["receipt"]),
                &plan,
            )?;
        }
        records::write_json(&self.run.artifact("validated.json")?, result)?;
        records::write_plain_json(&self.run.artifact("validated-artifacts.json")?, &validated)?;
        Ok(validated)
    }

    /// Record a receipt after each write; later failure remains partial and all
    /// pending/retained bytes remain available for explicit repair, never retry execution.
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
        let key = evidence.original_path.as_deref().map_or_else(
            || "generated input".into(),
            |path| self.run_path(path).display().to_string(),
        );
        let path = session.root().join(session.doc_key(doc)?);
        self.document = Some(DocumentTarget {
            doc,
            session: session.id().into(),
            generation: session.generation(doc)?,
            revision: session.revision(doc)?,
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
    fn publish_workspace(&self, destination: &str) -> Result<(PathBuf, Option<String>)> {
        let validated: Vec<(String, PathBuf, String)> = serde_json::from_slice(
            &basecraft_ops::records::read_record(&self.run.artifact("validated-artifacts.json")?)?,
        )?;
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
                        self.run_path(
                            self.context
                                .outputs
                                .get(role)
                                .ok_or_else(|| Error::Protocol("undeclared output role".into()))?,
                        ),
                    )
                    .map_err(|_| Error::Protocol("result escapes allocation".into()))?,
            )
        };
        if basecraft_io::hash_file(&retained)? != *hash {
            return Err(Error::Protocol("retained species result changed".into()));
        }
        let value: Value = serde_json::from_slice(&std::fs::read(&retained)?)?;
        if value["workspace"].is_null() {
            return Err(Error::Protocol("result holds no species workspace".into()));
        }
        let mut work = serde_json::from_value(value["workspace"].clone())?;
        let roster =
            basecraft_ops::delimitation::link_roster(&self.policy.root, &self.run, &mut work)?;
        let target = self.policy.path(destination, true)?;
        basecraft_ops::delimitation::publish_workspace_to(&target, &work, &self.run)?;
        Ok((target, roster.summary()))
    }

    /// The file holding a delimitation run's result: the descriptor's primary
    /// output, or the fit stage's report for a host alignment workflow.
    pub fn species_result_name(&self) -> String {
        self.operation
            .descriptor
            .outputs
            .values()
            .find_map(|output| output.primary.as_deref())
            .and_then(|member| Path::new(member).file_name())
            .map_or_else(
                || "fit.json".into(),
                |name| name.to_string_lossy().into_owned(),
            )
    }

    /// An exported tree gets what the retained member has: its run binding,
    /// producer support semantics and the link to the alignment it came from.
    fn finalise_exported_tree(&self, role: &str, tree: &Path) -> Result<()> {
        self.run.bind(tree)?;
        file_tool::write_tree_companion(
            tree,
            file_tool::producer_support(&self.operation.descriptor, &self.parameters),
            file_tool::stated_branch_lengths(
                &self.operation.descriptor.outputs[role],
                &self.parameters,
            ),
        )?;
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
                    .map(|path| self.run_path(path))
                    .ok_or_else(|| Error::Protocol("tree alignment source unknown".into()))?,
                0,
            ),
        };
        basecraft_ops::records::AlignmentLink::create_referenced(
            tree,
            &source,
            Path::new(frozen),
            version,
            &self.run,
        )?;
        Ok(())
    }

    /// Who ran this operation, and what its `tool.run` entry records: the
    /// requested parameters and the run folder. Everything else about the run
    /// is in the folder's `execution.json`.
    pub(crate) fn tool_run<'a>(
        &'a self,
        inputs: &'a [(String, String)],
        outputs: &'a [PathBuf],
    ) -> (
        basecraft_protocol::Actor,
        basecraft_ops::session::ToolRunRecord<'a>,
    ) {
        let manifest = &self.operation.package.manifest;
        let plugin = basecraft_protocol::Actor::Plugin {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
        };
        let record = basecraft_ops::session::ToolRunRecord {
            run: Some(&self.run),
            ..basecraft_ops::session::ToolRunRecord::new(
                (&manifest.id, &self.operation.descriptor.implementation),
                self.request.parameters.clone(),
                inputs,
                outputs,
            )
        };
        let tool = self
            .tool
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let actor = tool.map_or(plugin, |tool| basecraft_protocol::Actor::Tool {
            tool: tool.name,
            version: tool.version,
        });
        (actor, record)
    }

    /// A context path: relative to the run folder, or absolute outside it.
    pub fn run_path(&self, path: &str) -> PathBuf {
        self.run.directory().join(path)
    }

    pub fn effective_parameters(&self) -> Value {
        self.granted_parameters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn persist(&self, outcome: &OperationOutcome) -> Result<()> {
        let runtime = self.run.artifact("runtime.json")?;
        records::write_json(&runtime, outcome)?;
        self.write_task_index(outcome, &runtime)
    }

    /// The Tasks view lists runs from small index records in application
    /// state. This one only points at the run's own `runtime.json`, which
    /// stays the authoritative record.
    fn write_task_index(&self, outcome: &OperationOutcome, runtime: &Path) -> Result<()> {
        let source = self.document.as_ref().map_or_else(
            || {
                self.context
                    .input_evidence
                    .values()
                    .flatten()
                    .find_map(|evidence| evidence.original_path.as_deref())
                    .map(|path| self.run_path(path).display().to_string())
                    .unwrap_or_default()
            },
            |target| target.path.display().to_string(),
        );
        let outputs = outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), artifact.digest.clone()))
            .collect();
        let index = serde_json::to_value(LocalJobIndex {
            schema_version: 1,
            id: self.run.id().into(),
            status: outcome.state,
            owner_instance: basecraft_platform::process_instance().into(),
            started: self.started.clone(),
            ended: outcome
                .state
                .terminal()
                .then(basecraft_platform::now_rfc3339),
            plugin: JobIndexPlugin {
                id: self.operation.package.manifest.id.clone(),
                version: self.operation.package.manifest.version.clone(),
            },
            operation: self.operation.id.clone(),
            effective_parameters: outcome.effective_parameters.clone(),
            initiator: self.initiator.clone(),
            source: JobIndexSource { path: source },
            outputs,
            diagnostics: outcome.errors.first().map(|error| error.message.clone()),
            runtime_record: runtime.into(),
        })?;
        let path = records::job_index_path(self.run.id())?;
        std::fs::create_dir_all(path.parent().unwrap_or(&path))?;
        records::write_json(&path, &index)?;
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
    let record = records::read_input(directory)?;
    Ok(record
        .upstream
        .iter()
        .map(|value| directory.join(&value.bundle))
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
        if link_count(&path)? != 1 || basecraft_io::hash_file(&path)? != *hash {
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
        if basecraft_io::hash_file(&target)? != *hash {
            return Err(Error::Protocol("collection changed during export".into()));
        }
        basecraft_platform::files::sync_file(&target)?;
    }
    basecraft_platform::atomic::rename_noreplace(pending.path(), destination)?;
    basecraft_platform::files::sync_directory(parent)?;
    Ok(())
}

#[cfg(test)]
mod record_format_tests;
