//! Plugin operations that read and write files, such as aligners.
//!
//! A run happens in three steps, so the slow middle one can go to another
//! thread: [`FileJob::from_alignment`] writes the tool's input from an open
//! document, [`FileJob::run`] runs the plugin, and [`FileJob::record`] logs
//! a `tool.run` operation with the hashes of what went in and came out.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use basecraft_ops::files::{HashingWriter, write_alignment, write_ungapped};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::Actor;
use basecraft_protocol::plugin::InvokeResult;
use serde_json::json;

use crate::{Error, Manifest, PluginProcess, Result};

static CANCELLATIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, PathBuf>>,
> = std::sync::OnceLock::new();

fn cancellations() -> &'static std::sync::Mutex<std::collections::HashMap<String, PathBuf>> {
    CANCELLATIONS.get_or_init(std::sync::Mutex::default)
}

pub fn can_cancel_run(id: &str) -> bool {
    cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(id)
}

/// Only an active in-process job can authorize a cancellation file write.
pub fn cancel_run(id: &str) -> Result<()> {
    let path = cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .cloned()
        .ok_or_else(|| Error::Protocol("Cancellation is unavailable for this run".into()))?;
    basecraft_io::write_atomic(&path, |w| w.write_all(b"cancel\n"))
        .map_err(basecraft_ops::Error::from)?;
    Ok(())
}

/// Known producer support semantics, which must win over numeric-range guesses.
/// `package` and `implementation` name the plugin and its operation.
pub(crate) fn producer_support(
    package: &str,
    implementation: &str,
    params: &serde_json::Value,
) -> Option<(&'static str, basecraft_protocol::SupportKind)> {
    if implementation != "tree" {
        return None;
    }
    match package {
        "basecraft.distance"
            if params
                .get("bootstrap")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0 =>
        {
            Some(("Bootstrap", basecraft_protocol::SupportKind::Bootstrap))
        }
        "basecraft.fasttree" => Some((
            "SH-like local support",
            basecraft_protocol::SupportKind::Probability,
        )),
        "basecraft.iqtree" => iqtree_support(params.get("support")),
        _ => None,
    }
}

/// The built-in IQ-TREE form permits one support method per run. Both IQ-TREE
/// bootstrap variants write percentages; their names preserve the distinction.
fn iqtree_support(
    selected: Option<&serde_json::Value>,
) -> Option<(&'static str, basecraft_protocol::SupportKind)> {
    match selected.and_then(serde_json::Value::as_str) {
        Some("ufboot") => Some((
            "Ultrafast bootstrap (UFBoot)",
            basecraft_protocol::SupportKind::Bootstrap,
        )),
        Some("bootstrap") => Some((
            "Standard bootstrap",
            basecraft_protocol::SupportKind::Bootstrap,
        )),
        _ => None,
    }
}

/// Persist producer semantics only when the parsed tree has exactly one support
/// field. Slash-separated external/combined values need their own verified order.
pub(crate) fn write_support_sidecar(
    tree: &Path,
    name: &str,
    kind: basecraft_protocol::SupportKind,
) -> Result<Option<PathBuf>> {
    let mut doc = basecraft_ops::files::load_tree(tree)?.doc;
    if doc.supports().len() != 1 {
        return Ok(None);
    }
    let old = doc.supports()[0].name.clone();
    doc.rename_support(&old, name);
    doc.set_support_kind(name, kind);
    let metadata = basecraft_ops::files::tree_sidecar(tree);
    basecraft_io::write_atomic(&metadata, |w| {
        basecraft_ops::files::write_tree_sidecar(&doc, w)
    })
    .map_err(basecraft_ops::Error::from)?;
    Ok(Some(metadata))
}

struct CancellationRegistration(String);
impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        cancellations()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// What a tool gets from an alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignmentInput {
    /// The rows without gaps, for an aligner.
    Ungapped,
    /// The alignment as it is, for tree building.
    Aligned,
}

pub struct FileJob {
    /// The plugin's directory and the operation to call.
    pub plugin: PathBuf,
    pub operation: String,
    pub input: PathBuf,
    /// What the input was made from, with the hash of the input itself.
    pub input_key: (String, String),
    pub output: PathBuf,
    /// Public operation parameters, retained in the tool-run log.
    pub params: serde_json::Map<String, serde_json::Value>,
    run_id: String,
    source_record: basecraft_ops::document_record::DocumentRecord,
    cpu_budget: usize,
    accepted_registry: Option<std::sync::Arc<crate::registry::Registry>>,
    granted_budget: std::sync::atomic::AtomicUsize,
    source_path: PathBuf,
    source_version: u64,
    source_hash: String,
    artifacts: Option<basecraft_ops::analysis_store::ArtifactRun>,
    species: Option<SpeciesStorage>,
    allocation_started: std::sync::atomic::AtomicBool,
    scratch_read_bytes: std::sync::atomic::AtomicU64,
    submitted_copy_bytes: std::sync::atomic::AtomicU64,
    scratch_dir: tempfile::TempDir,
}

struct SpeciesStorage {
    anchor: PathBuf,
    primary: Option<PathBuf>,
    standalone: bool,
    dependencies: Vec<PathBuf>,
}

impl FileJob {
    fn prepare_species(
        &mut self,
        document: Option<(&Session, DocId)>,
        file: Option<(&Path, &basecraft_protocol::provenance::FileStamp)>,
        transformation: &str,
    ) -> Result<()> {
        let anchor = self.output.clone();
        let standalone = document.is_none_or(|(s, d)| !s.is_delimitation(d));
        self.prepare_species_at(document, file, transformation, &anchor, standalone)
    }

    fn prepare_species_at(
        &mut self,
        document: Option<(&Session, DocId)>,
        file: Option<(&Path, &basecraft_protocol::provenance::FileStamp)>,
        transformation: &str,
        anchor: &Path,
        standalone: bool,
    ) -> Result<()> {
        use basecraft_ops::{analysis_store::ArtifactRun, input_evidence};
        let plugin = Manifest::load(&self.plugin)?.id;
        if plugin != "basecraft.delimitation"
            && !(document.is_none() && plugin == "basecraft.distance" && self.operation == "tree")
        {
            return Ok(());
        }
        let _lock = input_evidence::lock()?;
        let run = ArtifactRun::create(anchor, &self.run_id)?;
        let source = if let Some((file, accepted_stamp)) = file {
            let path = file.canonicalize()?;
            let stamp = input_evidence::stamp(&path)?;
            if &stamp != accepted_stamp || stamp.size != std::fs::metadata(&self.input)?.len() {
                return Err(Error::Protocol(
                    "Analysis source changed during submission".into(),
                ));
            }
            Some(basecraft_protocol::provenance::InputReference {
                path: input_evidence::relative(run.directory(), &path)?,
                stamp,
                hash: self.input_key.1.clone(),
                extra: std::collections::BTreeMap::new(),
            })
        } else if let Some((session, doc)) = document {
            session.referenced_source(doc, run.directory())?
        } else {
            None
        };
        let primary = (standalone
            && matches!(
                self.operation.as_str(),
                "workspace_tree"
                    | "workspace_alignment"
                    | "manual_cut"
                    | "gmyc_single"
                    | "gmyc_multiple"
                    | "gmyc_exhaustive"
                    | "ptp"
                    | "mptp"
                    | "asap"
                    | "asap_import"
            ))
        .then(|| {
            if basecraft_ops::files::Format::from_path(anchor).ok()
                == Some(basecraft_ops::files::Format::Delimitation)
            {
                anchor.to_path_buf()
            } else {
                anchor.with_extension("spart")
            }
        });
        let internal_name = format!(
            "{}-result.{}",
            self.run_id,
            if self.operation == "tree" {
                "nwk"
            } else {
                "json"
            }
        );
        let record = input_evidence::SubmittedInput {
            schema_version: 1,
            state: if source.is_some() {
                "referenced"
            } else {
                "generated"
            }
            .into(),
            source,
            submitted_hash: self.input_key.1.clone(),
            submitted_size: std::fs::metadata(&self.input)?.len(),
            transformation: transformation.into(),
            extra: [("species_result".into(), json!(internal_name))].into(),
        };
        input_evidence::write_json(&run.directory().join("input.json"), &record)?;
        input_evidence::write_json(
            &run.directory().join("run.json"),
            &json!({"schema_version":1,"id":self.run_id,"result":anchor.file_name().and_then(|n| n.to_str()),"status":"prepared"}),
        )?;
        input_evidence::register(&run)?;
        self.output = run.artifact(&internal_name)?;
        self.source_hash = record
            .source
            .as_ref()
            .map_or_else(String::new, |r| r.hash.clone());
        self.species = Some(SpeciesStorage {
            anchor: anchor.to_path_buf(),
            primary,
            standalone,
            dependencies: Vec::new(),
        });
        self.artifacts = Some(run);
        Ok(())
    }

    /// Run stages from this accepted registry: the resolved package is verified
    /// before spawning and CPU settings bind against its exact descriptor.
    #[must_use]
    pub fn with_registry(mut self, registry: std::sync::Arc<crate::registry::Registry>) -> Self {
        self.accepted_registry = Some(registry);
        self
    }

    pub(crate) fn stage_registry(&self) -> Result<crate::registry::Registry> {
        match &self.accepted_registry {
            Some(registry) => Ok(registry.as_ref().clone()),
            None => crate::registry::Registry::load(&crate::registry::RegistryConfig::discover()?),
        }
    }

    #[must_use]
    pub fn with_cpu_budget(mut self, budget: usize) -> Self {
        self.cpu_budget = budget.clamp(1, crate::compute::available());
        self
    }

    pub fn species_input_evidence(&self) -> Result<serde_json::Value> {
        let run = self
            .artifacts
            .as_ref()
            .ok_or_else(|| Error::Protocol("No species run".into()))?;
        let input = basecraft_ops::input_evidence::read_input(run.directory())?;
        let path = input.source.as_ref().map_or_else(
            || run.directory().join("submitted.fasta"),
            |r| run.directory().join(&r.path),
        );
        Ok(
            json!({"run_directory":run.directory(),"path":path.canonicalize()?,"source_hash":input.source.as_ref().map(|r| &r.hash),"submitted_hash":input.submitted_hash,"transformation":input.transformation,"state":input.state}),
        )
    }

    pub fn species_workspace_target(&self, output: &Path) -> Option<PathBuf> {
        self.species.as_ref().filter(|s| s.standalone).map(|_| {
            if basecraft_ops::files::Format::from_path(output).ok()
                == Some(basecraft_ops::files::Format::Delimitation)
            {
                output.to_path_buf()
            } else {
                output.with_extension("spart")
            }
        })
    }

    pub fn publish_species_workspace_at(&mut self, target: Option<PathBuf>) {
        if let Some(storage) = &mut self.species {
            storage.primary = target;
        }
    }

    /// Progress and cancellation live with execution scratch, never as durable
    /// companions of the scientific result.
    pub fn control_paths(&self) -> (PathBuf, PathBuf) {
        (
            self.scratch_dir.path().join("progress.json"),
            self.scratch_dir.path().join("cancel"),
        )
    }

    /// Supporting species stages stay in their reserved run, never beside the SPART.
    pub fn species_artifact(&self, name: &str) -> Result<PathBuf> {
        self.artifacts
            .as_ref()
            .ok_or_else(|| Error::Protocol("No species artifact run".into()))?
            .artifact(name)
            .map_err(Into::into)
    }

    /// A job whose input is the alignment as FASTA, with or without gaps.
    pub fn from_alignment(
        session: &Session,
        doc: DocId,
        (plugin, operation): (&Path, &str),
        kind: AlignmentInput,
        output: PathBuf,
    ) -> Result<Self> {
        if !session.is_alignment(doc) {
            return Err(Error::Protocol(
                "file analysis requires an alignment document".into(),
            ));
        }
        let source_path = session.root().join(session.doc_key(doc));
        if output.exists() && output.canonicalize()? == source_path.canonicalize()? {
            return Err(Error::Protocol(
                "tool output must differ from the source alignment".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-tool")
            .tempdir()?;
        let path = dir.path().join("input.fasta");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut hw = HashingWriter::new(&mut file);
        let key = match kind {
            AlignmentInput::Ungapped => {
                write_ungapped(session.alignment(doc), &mut hw)?;
                format!("{} (ungapped)", session.doc_key(doc))
            }
            AlignmentInput::Aligned => {
                write_alignment(session.alignment(doc), &mut hw)?;
                session.doc_key(doc).to_string()
            }
        };
        let hash = hw.hash();
        file.flush()?;
        let run_id = ulid::Ulid::generate().to_string();
        let manifest = Manifest::load(plugin)?;
        let referenced = operation == "tree"
            || matches!(
                (manifest.id.as_str(), operation),
                ("basecraft.mafft", "align")
                    | ("basecraft.trimal", "trim")
                    | ("basecraft.distance", "matrix")
            );
        let artifacts = if referenced {
            let _lock = basecraft_ops::input_evidence::lock()?;
            let run = basecraft_ops::analysis_store::ArtifactRun::create(&output, &run_id)?;
            let source = session.referenced_source(doc, run.directory())?;
            let input_record = basecraft_ops::input_evidence::SubmittedInput {
                schema_version: 1,
                state: if source.is_some() {
                    "referenced"
                } else {
                    "generated"
                }
                .into(),
                source,
                submitted_hash: hash.clone(),
                submitted_size: std::fs::metadata(&path)?.len(),
                transformation: match kind {
                    AlignmentInput::Aligned => "canonical-fasta-v1",
                    AlignmentInput::Ungapped => "ungapped-fasta-v1",
                }
                .into(),
                extra: std::collections::BTreeMap::new(),
            };
            basecraft_ops::input_evidence::write_json(
                &run.directory().join("run.json"),
                &json!({"schema_version":1,"id":run_id,"result":output.file_name().and_then(|n| n.to_str()).ok_or_else(|| Error::Protocol("result filename is not UTF-8".into()))?,"status":"prepared"}),
            )?;
            basecraft_ops::input_evidence::write_json(
                &run.directory().join("input.json"),
                &input_record,
            )?;
            basecraft_ops::input_evidence::register(&run)?;
            Some(run)
        } else {
            None
        };
        let source_hash = if let Some(run) = &artifacts {
            basecraft_ops::input_evidence::read_input(run.directory())?
                .source
                .map_or_else(String::new, |r| r.hash)
        } else if manifest.id == "basecraft.delimitation" {
            String::new()
        } else {
            basecraft_io::hash_file(&source_path).map_err(basecraft_ops::Error::from)?
        };
        let mut job = Self {
            plugin: plugin.to_path_buf(),
            operation: operation.to_string(),
            input: path,
            input_key: (key, hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: session.root().join(session.doc_key(doc)),
            source_version: session.generation(doc),
            source_record: session.document_record(doc)?,
            source_hash,
            cpu_budget: crate::compute::effective_allocation()?,
            accepted_registry: None,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            artifacts,
            species: None,
            allocation_started: std::sync::atomic::AtomicBool::new(false),
            scratch_read_bytes: std::sync::atomic::AtomicU64::new(0),
            submitted_copy_bytes: std::sync::atomic::AtomicU64::new(0),
            scratch_dir: dir,
        };
        job.prepare_species(
            Some((session, doc)),
            None,
            match kind {
                AlignmentInput::Aligned => "canonical-fasta-v1",
                AlignmentInput::Ungapped => "ungapped-fasta-v1",
            },
        )?;
        Ok(job)
    }

    /// Run the plugin. Touches no session, so it can run on any thread.
    pub fn from_tree(
        session: &Session,
        doc: DocId,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        if !session.is_tree(doc) {
            return Err(Error::Protocol("a tree input is required".into()));
        }
        let source_path = session.root().join(session.doc_key(doc));
        if output.exists() && output.canonicalize()? == source_path.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from source tree".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-tree-job-")
            .tempdir()?;
        let input = dir.path().join("input.nwk");
        std::fs::write(&input, session.tree(doc).to_newick())?;
        let hash = basecraft_io::hash_file(&input).map_err(basecraft_ops::Error::from)?;
        let run_id = ulid::Ulid::generate().to_string();
        let mut job = Self {
            plugin: plugin.to_path_buf(),
            operation: operation.into(),
            input,
            input_key: (session.doc_key(doc).into(), hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: source_path.clone(),
            source_version: session.generation(doc),
            source_record: session.document_record(doc)?,
            source_hash: if Manifest::load(plugin)?.id == "basecraft.delimitation" {
                String::new()
            } else {
                basecraft_io::hash_file(&source_path).map_err(basecraft_ops::Error::from)?
            },
            cpu_budget: crate::compute::effective_allocation()?,
            accepted_registry: None,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            artifacts: None,
            species: None,
            allocation_started: std::sync::atomic::AtomicBool::new(false),
            scratch_read_bytes: std::sync::atomic::AtomicU64::new(0),
            submitted_copy_bytes: std::sync::atomic::AtomicU64::new(0),
            scratch_dir: dir,
        };
        job.prepare_species(Some((session, doc)), None, "canonical-newick-v1")?;
        Ok(job)
    }

    /// Public immutable file input for analyses coordinated by another document.
    pub fn from_file(
        session: &Session,
        doc: DocId,
        input: &Path,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        let source_path = input.canonicalize()?;
        if output.exists() && output.canonicalize()? == input.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from analysis input".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-file-job-")
            .tempdir()?;
        let snapshot = dir.path().join(
            input
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("input")),
        );
        let input_stamp = basecraft_ops::input_evidence::stamp(input)?;
        let hash = freeze_file(input, &snapshot)?;
        let run_id = ulid::Ulid::generate().to_string();
        let mut job = Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash.clone()),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            source_version: session.generation(doc),
            source_record: file_input_record(input, plugin, operation)?,
            source_hash: hash.clone(),
            source_path,
            run_id,
            cpu_budget: crate::compute::effective_allocation()?,
            accepted_registry: None,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            artifacts: None,
            species: None,
            allocation_started: std::sync::atomic::AtomicBool::new(false),
            scratch_read_bytes: std::sync::atomic::AtomicU64::new(0),
            submitted_copy_bytes: std::sync::atomic::AtomicU64::new(0),
            scratch_dir: dir,
        };
        job.prepare_species(
            Some((session, doc)),
            Some((input, &input_stamp)),
            "identity-v1",
        )?;
        if session.is_delimitation(doc) && operation == "tree" {
            job.prepare_species_display(input, &input_stamp)?;
        }
        Ok(job)
    }

    fn prepare_species_display(
        &mut self,
        input: &Path,
        accepted: &basecraft_protocol::provenance::FileStamp,
    ) -> Result<()> {
        use basecraft_ops::{analysis_store::ArtifactRun, input_evidence};
        if !matches!(
            Manifest::load(&self.plugin)?.id.as_str(),
            "basecraft.distance" | "basecraft.iqtree"
        ) {
            return Ok(());
        }
        let _lock = input_evidence::lock()?;
        let source = input.canonicalize()?;
        let stamp = input_evidence::stamp(&source)?;
        if &stamp != accepted {
            return Err(Error::Protocol(
                "Display alignment changed during submission".into(),
            ));
        }
        let run = ArtifactRun::create(&self.output, &self.run_id)?;
        let input = input_evidence::SubmittedInput {
            schema_version: 1,
            source: Some(basecraft_protocol::provenance::InputReference {
                path: input_evidence::relative(run.directory(), &source)?,
                hash: self.input_key.1.clone(),
                stamp,
                extra: std::collections::BTreeMap::new(),
            }),
            submitted_hash: self.input_key.1.clone(),
            submitted_size: accepted.size,
            transformation: "identity-v1".into(),
            state: "referenced".into(),
            extra: std::collections::BTreeMap::new(),
        };
        input_evidence::write_json(&run.artifact("input.json")?, &input)?;
        input_evidence::write_json(
            &run.artifact("run.json")?,
            &json!({"schema_version":1,"id":run.id(),"result":self.output.file_name().and_then(|n|n.to_str()),"status":"prepared"}),
        )?;
        input_evidence::register(&run)?;
        self.artifacts = Some(run);
        Ok(())
    }

    /// Compose a dependent public file operation without accessing a document on
    /// the worker thread. The next input is copied before the stage starts.
    pub fn following(
        &self,
        input: &Path,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        if output.exists() && output.canonicalize()? == input.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from analysis input".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-following-job-")
            .tempdir()?;
        let snapshot = dir.path().join(
            input
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("input")),
        );
        let input_stamp = basecraft_ops::input_evidence::stamp(input)?;
        let hash = freeze_file(input, &snapshot)?;
        let run_id = ulid::Ulid::generate().to_string();
        let mut job = Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: self.source_path.clone(),
            source_version: self.source_version,
            source_record: self.following_record(input)?,
            source_hash: self.source_hash.clone(),
            cpu_budget: self.cpu_budget,
            accepted_registry: self.accepted_registry.clone(),
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            artifacts: None,
            species: None,
            allocation_started: std::sync::atomic::AtomicBool::new(false),
            scratch_read_bytes: std::sync::atomic::AtomicU64::new(0),
            submitted_copy_bytes: std::sync::atomic::AtomicU64::new(0),
            scratch_dir: dir,
        };
        if let Some(storage) = &self.species {
            let temporary = input.starts_with(self.scratch_dir.path());
            job.prepare_species_at(
                None,
                (!temporary).then_some((input, &input_stamp)),
                if temporary {
                    "canonical-fasta-v1"
                } else {
                    "identity-v1"
                },
                &storage.anchor,
                false,
            )?;
            if let (Some(parent), Some(run), Some(next)) =
                (&self.artifacts, &job.artifacts, &mut job.species)
            {
                next.dependencies.clone_from(&storage.dependencies);
                next.dependencies.push(parent.directory().to_path_buf());
                if temporary {
                    let mut inherited =
                        basecraft_ops::input_evidence::read_input(parent.directory())?;
                    if let Some(source) = &mut inherited.source {
                        source.path = basecraft_ops::input_evidence::relative(
                            run.directory(),
                            &parent.directory().join(&source.path).canonicalize()?,
                        )?;
                    }
                    inherited.extra.insert(
                        "species_result".into(),
                        json!(job.output.file_name().and_then(|n| n.to_str())),
                    );
                    basecraft_ops::input_evidence::write_json(
                        &run.directory().join("input.json"),
                        &inherited,
                    )?;
                }
            }
        }
        Ok(job)
    }

    fn following_record(
        &self,
        input: &Path,
    ) -> Result<basecraft_ops::document_record::DocumentRecord> {
        use basecraft_protocol::{LogEntry, Op, OpResult, provenance::EditActivity};
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let manifest_path = paths
            .state
            .join("jobs")
            .join(&self.run_id)
            .join("manifest.json");
        let file = std::fs::File::open(&manifest_path)?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(file, 16 * 1024 * 1024 + 1),
            &mut bytes,
        )?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(Error::Protocol(
                "Retained execution record exceeds 16 MiB".into(),
            ));
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| Error::Protocol(e.to_string()))?;
        let hash = basecraft_io::hash_file(input).map_err(basecraft_ops::Error::from)?;
        let key = input.display().to_string();
        if manifest["status"] != "completed"
            || manifest["outputs"][&key].as_str() != Some(hash.as_str())
        {
            // An intermediate conversion without matching execution evidence stays an unknown boundary.
            return Ok(basecraft_ops::document_record::DocumentRecord::from_file(
                input,
            )?);
        }
        let mut record = self.source_record.clone();
        let mut params = manifest["effective_parameters"].clone();
        if !params.is_object() {
            return Err(Error::Protocol(
                "Retained execution parameters must be an object".into(),
            ));
        }
        if !manifest["execution"].is_null() {
            params["_execution"] = manifest["execution"].clone();
        }
        let actor = if let Ok(tool) =
            serde_json::from_value::<basecraft_protocol::plugin::ToolInfo>(manifest["tool"].clone())
        {
            Actor::Tool {
                tool: tool.name,
                version: tool.version,
            }
        } else {
            Actor::Plugin {
                id: manifest["plugin"]["id"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                version: manifest["plugin"]["version"]
                    .as_str()
                    .unwrap_or("not recorded")
                    .into(),
            }
        };
        let mut input_session = Session::recoverable(
            input
                .parent()
                .ok_or_else(|| Error::Protocol("input has no parent".into()))?,
        )?;
        let input_doc = input_session.open(input)?;
        let op = Op::ToolRun {
            plugin: manifest["plugin"]["id"]
                .as_str()
                .unwrap_or("unknown")
                .into(),
            operation: self.operation.clone(),
            params,
            inputs: vec![self.input_key.0.clone()],
            outputs: vec![key.clone()],
        };
        record.events.push(LogEntry {
            v: 1,
            id: self.run_id.clone(),
            t: manifest["ended"].as_str().unwrap_or("").into(),
            host: String::new(),
            session: self.run_id.clone(),
            actor,
            doc: key.clone(),
            op: op.clone(),
            result: OpResult {
                hashes: vec![self.input_key.clone(), (key, hash)],
                feature: None,
            },
            state: input_session.state(input_doc),
        });
        record.effective.push(EditActivity {
            id: self.run_id.clone(),
            operation: op,
        });
        Ok(record)
    }

    /// Host-controlled I/O after the document's existing verified open. These
    /// counters exclude tool reads, mmap page faults and the initial document load.
    pub fn input_io(&self) -> Option<serde_json::Value> {
        self.artifacts.as_ref().map(|_| json!({
            "source_identity_hash_bytes": 0, "persistent_original_copy_bytes": 0,
            "execution_scratch_read_bytes": self.scratch_read_bytes.load(std::sync::atomic::Ordering::Relaxed),
            "persistent_submitted_copy_bytes": self.submitted_copy_bytes.load(std::sync::atomic::Ordering::Relaxed),
        }))
    }

    fn execution_budget(&self) -> usize {
        let granted = self
            .granted_budget
            .load(std::sync::atomic::Ordering::Relaxed);
        if granted == 0 {
            self.cpu_budget
        } else {
            granted
        }
    }

    fn retained_input(&self) -> Result<PathBuf> {
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let id = &self.run_id;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(Error::Protocol("invalid run identity".into()));
        }
        let retained = paths.state.join("jobs").join(id).join("input.snapshot");
        let path = if retained.is_file() {
            retained
        } else {
            self.input.clone()
        };
        if basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)? != self.input_key.1 {
            return Err(Error::Protocol("submitted input snapshot changed".into()));
        }
        Ok(path)
    }

    /// Record a terminal outcome even if the plugin reports an RPC error.
    /// Input snapshots outlive the temporary worker directory through the store.
    pub fn record_terminal(
        &self,
        session: &mut Session,
        doc: DocId,
        outcome: &Result<InvokeResult>,
        elapsed_seconds: f64,
    ) -> Result<()> {
        use basecraft_protocol::delimitation::{JobRecord, JobStatus};
        if self.artifacts.is_none() {
            session.snapshot_file(&self.retained_input()?)?;
        }
        if let Some(run) = &self.artifacts {
            let cancelled = self
                .params
                .get("cancel_file")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|p| Path::new(p).exists());
            return session
                .log_referenced_job_finished(
                    doc,
                    JobRecord {
                        id: self.run_id.clone(),
                        generation: self.source_version,
                        status: if outcome.is_ok() {
                            JobStatus::Completed
                        } else if cancelled {
                            JobStatus::Cancelled
                        } else {
                            JobStatus::Failed
                        },
                        method: self.operation.clone(),
                        inputs: std::iter::once(self.input_key.clone()).collect(),
                        outputs: self.retained_outputs(
                            run,
                            outcome.as_ref().ok(),
                            outcome.as_ref().err(),
                        )?,
                        diagnostics: outcome
                            .as_ref()
                            .err()
                            .map_or_else(Vec::new, |e| vec![e.to_string()]),
                        elapsed_seconds,
                    },
                )
                .map_err(Into::into);
        }
        let cancelled = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|p| Path::new(p).exists());
        let diagnostics = outcome
            .as_ref()
            .err()
            .map_or_else(Vec::new, |e| vec![e.to_string()]);
        let mut report = JobRecord {
            id: self.run_id.clone(),
            generation: self.source_version,
            status: if outcome.is_ok() {
                JobStatus::Completed
            } else if cancelled {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            },
            method: self.operation.clone(),
            inputs: std::iter::once(self.input_key.clone()).collect(),
            outputs: std::collections::BTreeMap::new(),
            diagnostics,
            elapsed_seconds,
        };
        let path = PathBuf::from(format!("{}.run.json", self.output.display()));
        if path.exists() {
            report
                .outputs
                .insert(path.display().to_string(), session.snapshot_file(&path)?);
            if let Ok(value) = std::fs::read(&path).and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(std::io::Error::other)
            }) {
                if let Ok(native) = serde_json::from_value::<JobRecord>(value["job"].clone()) {
                    if native.id == self.run_id && native.method == self.operation {
                        report.status = native.status;
                        report.diagnostics.extend(native.diagnostics);
                        for path in native.outputs.keys() {
                            let path = Path::new(path);
                            if path.is_file() {
                                report.outputs.insert(
                                    path.display().to_string(),
                                    session.snapshot_file(path)?,
                                );
                            }
                        }
                    }
                }
            }
        }
        if self.artifacts.is_some() {
            session.log_referenced_job_finished(doc, report)?;
        } else {
            session.log_job_finished(doc, report)?;
        }
        Ok(())
    }

    pub fn run(&self) -> Result<InvokeResult> {
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let id = &self.run_id;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(Error::Protocol("invalid run identity".into()));
        }
        let run_dir = paths.state.join("jobs").join(id);
        // Unique ownership prevents reusing a run identity to overwrite evidence.
        std::fs::create_dir_all(paths.state.join("jobs"))?;
        std::fs::create_dir(&run_dir)?;
        let input = std::fs::read(&self.input)?;
        self.scratch_read_bytes
            .fetch_add(input.len() as u64, std::sync::atomic::Ordering::Relaxed);
        if basecraft_io::hash_bytes(&input) != self.input_key.1 {
            return Err(Error::Protocol("job input changed before launch".into()));
        }
        if let Some(run) = &self.artifacts {
            let recorded = basecraft_ops::input_evidence::read_input(run.directory())?;
            if recorded.submitted_hash != self.input_key.1
                || recorded.submitted_size != input.len() as u64
            {
                return Err(Error::Protocol(
                    "submitted input record changed before launch".into(),
                ));
            }
            if recorded.state == "generated" {
                basecraft_io::write_atomic(&run.directory().join("submitted.fasta"), |w| {
                    w.write_all(&input)
                })
                .map_err(basecraft_ops::Error::from)?;
                self.submitted_copy_bytes
                    .fetch_add(input.len() as u64, std::sync::atomic::Ordering::Relaxed);
            }
        } else {
            basecraft_io::write_atomic(&run_dir.join("input.snapshot"), |w| w.write_all(&input))
                .map_err(basecraft_ops::Error::from)?;
        }
        if let Some(run) = &self.artifacts {
            if self.params.contains_key("analysis_artifacts")
                || self.params.contains_key("bootstrap_trees")
            {
                return Err(Error::Protocol("host-managed allocation cannot be combined with caller allocation or bootstrap_trees".into()));
            }
            if self
                .params
                .get("bootstrap")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            {
                for partial in [false, true] {
                    if std::fs::symlink_metadata(self.expected_bootstrap_path(run, partial)?)
                        .is_ok()
                    {
                        return Err(Error::Protocol(
                            "bootstrap artifact destination already exists; nothing was adopted"
                                .into(),
                        ));
                    }
                }
            }
            self.allocation_started
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let started = basecraft_platform::now_rfc3339();
        self.write_run_manifest(&run_dir, &started, "running", None)?;
        let _registration = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .map(|path| {
                cancellations()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(self.run_id.clone(), PathBuf::from(path));
                CancellationRegistration(self.run_id.clone())
            });
        let frozen_input = if self.artifacts.is_some() {
            self.input.clone()
        } else {
            run_dir.join("input.snapshot")
        };
        let result = self.execute_run(&frozen_input);
        let cancelled = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|p| Path::new(p).exists());
        let status = if result.is_ok() {
            "completed"
        } else if cancelled {
            "cancelled"
        } else {
            "failed"
        };
        self.write_run_manifest(&run_dir, &started, status, Some(&result))?;
        result
    }

    fn expected_bootstrap_path(
        &self,
        run: &basecraft_ops::analysis_store::ArtifactRun,
        partial: bool,
    ) -> Result<PathBuf> {
        let suffix = if partial { ".partial.nex" } else { ".nex" };
        let name = format!("bootstrap-{}{suffix}", run.id());
        let location = if self
            .params
            .get("bootstrap_destination")
            .and_then(serde_json::Value::as_str)
            == Some("working")
        {
            basecraft_ops::analysis_store::ArtifactDestination::WorkingDirectory
        } else {
            basecraft_ops::analysis_store::ArtifactDestination::Metadata
        };
        Ok(run.destination(location, &name)?)
    }

    fn diagnostic_artifact(failure: Option<&Error>) -> Option<serde_json::Value> {
        let message = failure?.to_string();
        let (_, json) = message.split_once("; retained bootstrap artifacts: ")?;
        serde_json::Deserializer::from_str(json)
            .into_iter::<serde_json::Value>()
            .next()?
            .ok()
    }

    fn valid_bootstrap_artifact(
        &self,
        run: &basecraft_ops::analysis_store::ArtifactRun,
        collection: &serde_json::Value,
    ) -> Result<bool> {
        if !self
            .allocation_started
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Ok(false);
        }
        let requested = self
            .params
            .get("bootstrap")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let status = collection["status"].as_str();
        let expected = self.expected_bootstrap_path(run, status != Some("complete"))?;
        let count = collection["trees"].as_u64();
        Ok(requested > 0
            && collection["format"] == "nexus"
            && collection["path"]
                .as_str()
                .is_some_and(|p| Path::new(p) == expected)
            && matches!(
                status,
                Some("complete" | "partial" | "cancelled" | "original_failed")
            )
            && count
                .is_some_and(|n| n <= requested && (status != Some("complete") || n == requested)))
    }

    fn retained_outputs(
        &self,
        run: &basecraft_ops::analysis_store::ArtifactRun,
        result: Option<&InvokeResult>,
        failure: Option<&Error>,
    ) -> Result<std::collections::BTreeMap<String, String>> {
        let mut paths =
            result.map_or_else(Vec::new, |r| r.outputs.iter().map(PathBuf::from).collect());
        // Failed execution paths are registered only from structured reports
        // written in this run's allocated locations, never from path existence.
        if self
            .allocation_started
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let report_name = format!(
                "{}.run.json",
                self.output
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            for report in [
                run.directory().join(&report_name),
                run.directory().join("pending").join(&report_name),
            ] {
                if !report.is_file() {
                    continue;
                }
                let value: serde_json::Value =
                    serde_json::from_slice(&basecraft_ops::provenance::read_record(&report)?)?;
                if self.species.is_some()
                    && value["job"]["id"] == self.run_id
                    && value["job"]["method"] == self.operation
                {
                    paths.push(report.clone());
                    if let Some(outputs) = value["job"]["outputs"].as_object() {
                        if outputs.len() > 4096 {
                            return Err(Error::Protocol(
                                "Species recovery artifact limit exceeded".into(),
                            ));
                        }
                        for (path, expected) in outputs {
                            let path = PathBuf::from(path);
                            if !path.canonicalize()?.starts_with(run.directory())
                                || expected.as_str()
                                    != Some(
                                        basecraft_io::hash_file(&path)
                                            .map_err(basecraft_ops::Error::from)?
                                            .as_str(),
                                    )
                            {
                                return Err(Error::Protocol(
                                    "Species recovery artifact escaped its run or changed".into(),
                                ));
                            }
                            paths.push(path);
                        }
                    }
                }
                if let Some(collection) = value.pointer("/bootstrap/replicate_trees") {
                    let path = collection
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .map(PathBuf::from);
                    let status = collection.get("status").and_then(serde_json::Value::as_str);
                    let expected = self.expected_bootstrap_path(run, status != Some("complete"))?;
                    if self.valid_bootstrap_artifact(run, collection)?
                        && path.as_ref() == Some(&expected)
                        && collection["format"] == "nexus"
                        && collection["trees"].as_u64()
                            == value
                                .pointer("/bootstrap/completed")
                                .and_then(serde_json::Value::as_u64)
                        && matches!(
                            status,
                            Some("complete" | "partial" | "cancelled" | "original_failed")
                        )
                    {
                        paths.push(expected);
                        paths.push(report.clone());
                    }
                }
            }
        }
        if let Some(collection) = Self::diagnostic_artifact(failure) {
            if self.valid_bootstrap_artifact(run, &collection)? {
                if let Some(path) = collection["path"].as_str() {
                    paths.push(PathBuf::from(path));
                }
            }
        }
        if Manifest::load(&self.plugin)?.id == "basecraft.distance" {
            let declaration = run.artifact("cache-publication.json")?;
            if declaration.is_file() {
                let record: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&declaration)?)?;
                paths.push(declaration);
                if let (Some(name), Some(hash)) =
                    (record["cache"].as_str(), record["hash"].as_str())
                {
                    let path = run.artifact(name)?;
                    if basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)? != hash {
                        return Err(Error::Protocol("retained cache evidence changed".into()));
                    }
                    paths.push(path);
                    let report = run.artifact(&format!(
                        "{}.run.json",
                        self.output
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ))?;
                    if report.is_file() {
                        paths.push(report);
                    }
                }
            }
        }
        let mut outputs = std::collections::BTreeMap::new();
        for path in paths {
            if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(Error::Protocol("retained artifact is a symlink".into()));
            }
            outputs.insert(
                path.display().to_string(),
                basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?,
            );
        }
        Ok(outputs)
    }

    fn write_run_manifest(
        &self,
        directory: &Path,
        started: &str,
        status: &str,
        outcome: Option<&Result<InvokeResult>>,
    ) -> Result<()> {
        let manifest = Manifest::load(&self.plugin)?;
        let mut parameters =
            crate::compute::parameters(&manifest.id, self.params.clone(), self.execution_budget());
        parameters.insert("job_id".into(), json!(self.run_id));
        let mut outputs = serde_json::Map::new();
        if let Some(run) = &self.artifacts {
            for (path, hash) in self.retained_outputs(
                run,
                outcome.and_then(|o| o.as_ref().ok()),
                outcome.and_then(|o| o.as_ref().err()),
            )? {
                outputs.insert(path, json!(hash));
            }
        } else if outcome.is_some_and(Result::is_ok) {
            let hash = basecraft_io::hash_file(&self.output).map_err(basecraft_ops::Error::from)?;
            outputs.insert(self.output.display().to_string(), json!(hash));
        }
        let source_path =
            serde_json::to_value(&self.source_path).map_err(|e| Error::Protocol(e.to_string()))?;
        let value = json!({
            "schema_version": 1, "id": self.run_id, "status": status, "input_io": self.input_io(),
            "primary_output": if outcome.is_some_and(Result::is_ok) { Some(&self.output) } else { None },
            "recovery_artifact": Self::diagnostic_artifact(outcome.and_then(|o| o.as_ref().err())),
            "owner_instance": basecraft_platform::process_instance(),
            "source_record": self.source_record,
            "started": started, "ended": outcome.map(|_| basecraft_platform::now_rfc3339()),
            "plugin": { "id": manifest.id, "version": manifest.version },
            "operation": self.operation, "effective_parameters": parameters,
            "source": { "path": source_path, "generation": self.source_version, "disk_hash": self.source_hash },
            "input": { "logical_name": self.input_key.0, "hash": self.input_key.1, "retained": if self.artifacts.is_none() { Some("input.snapshot") } else { None }, "artifact_run": self.artifacts.as_ref().map(basecraft_ops::analysis_store::ArtifactRun::directory), "bytes": std::fs::metadata(&self.input)?.len() },
            "outputs": outputs, "requested_cpu_budget": self.cpu_budget, "cpu_budget": self.execution_budget(),
            "diagnostics": outcome.and_then(|r| r.as_ref().err()).map(ToString::to_string),
            "tool": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.tool.as_ref()),
            "execution": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.execution.as_ref()),
            "completeness": { "executable_argv_recorded": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.execution.as_ref()).is_some_and(|e| !e.argv.is_empty()), "environment_lock_recorded": false },
            "interruption_policy": "a running record left by a terminated owner is interrupted, never completed"
        });
        let mut bytes =
            serde_json::to_vec_pretty(&value).map_err(|e| Error::Protocol(e.to_string()))?;
        bytes.push(b'\n');
        if let Some(run) = &self.artifacts {
            basecraft_io::write_atomic(&run.directory().join("execution.json"), |w| {
                w.write_all(&bytes)
            })
            .map_err(basecraft_ops::Error::from)?;
            // Success is recorded when the result is published; a terminal
            // failure must not leave the run looking merely prepared.
            if matches!(status, "failed" | "cancelled") {
                let path = run.directory().join("run.json");
                if let Some(mut record) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                {
                    record["status"] = json!(status);
                    basecraft_ops::input_evidence::write_json(&path, &record)?;
                }
            }
        }
        basecraft_io::write_atomic(&directory.join("manifest.json"), |w| w.write_all(&bytes))
            .map_err(basecraft_ops::Error::from)?;
        Ok(())
    }

    fn execute_run(&self, frozen_input: &Path) -> Result<InvokeResult> {
        if self
            .species
            .as_ref()
            .and_then(|s| s.primary.as_ref())
            .is_some_and(|p| std::fs::symlink_metadata(p).is_ok())
        {
            return Err(Error::Protocol("Species workspace already exists; choose a new filename or open it to add hypotheses".into()));
        }
        let cancel = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .map(Path::new);
        if self.artifacts.is_some() && std::fs::symlink_metadata(&self.output).is_ok() {
            return Err(Error::Protocol(
                "analysis output already exists; choose a new result filename".into(),
            ));
        }
        let permit = crate::compute::reserve_cpu(self.cpu_budget, cancel)?;
        let granted_budget = permit.threads();
        self.granted_budget
            .store(granted_budget, std::sync::atomic::Ordering::Relaxed);
        let manifest = Manifest::load(&self.plugin)?;
        let (mut process, mut params) = if let Some(registry) = &self.accepted_registry {
            let operation = registry
                .operations
                .values()
                .find(|op| {
                    op.package.manifest.id == manifest.id
                        && op.descriptor.implementation == self.operation
                })
                .ok_or_else(|| Error::Protocol("accepted workflow stage is unavailable".into()))?;
            operation.package.verify()?;
            if operation.package.manifest.dir != manifest.dir {
                return Err(Error::Protocol(
                    "workflow stage package differs from accepted package".into(),
                ));
            }
            let python =
                operation.package.interpreter.as_deref().ok_or_else(|| {
                    Error::Protocol("workflow stage runtime is unavailable".into())
                })?;
            (
                PluginProcess::spawn_accepted(&manifest, python, granted_budget)?,
                crate::compute::parameters_for(
                    &operation.descriptor,
                    self.params.clone(),
                    granted_budget,
                ),
            )
        } else {
            (
                PluginProcess::spawn_with_allocation(&manifest, granted_budget)?,
                crate::compute::parameters(&manifest.id, self.params.clone(), granted_budget),
            )
        };
        params.insert("job_id".into(), json!(self.run_id));
        params.entry("original_source").or_insert(
            serde_json::to_value(&self.source_path).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        if let Some(run) = &self.artifacts {
            if params.contains_key("analysis_artifacts") || params.contains_key("bootstrap_trees") {
                return Err(Error::Protocol("host-managed allocation cannot be combined with caller analysis_artifacts or bootstrap_trees; use bootstrap_destination".into()));
            }
            if let Some(destination) = params.get("bootstrap_destination") {
                if !matches!(destination.as_str(), Some("metadata" | "working")) {
                    return Err(Error::Protocol(
                        "bootstrap_destination must be metadata or working".into(),
                    ));
                }
            }
            params.insert("analysis_artifacts".into(), json!({
                "metadata_directory": run.directory(),
                "working_directory": self.output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).canonicalize()?,
                "run_id": run.id(),
            }));
            if self.species.is_some() {
                let evidence = basecraft_ops::input_evidence::read_input(run.directory())?;
                let stable = evidence.source.as_ref().map_or_else(
                    || run.directory().join("submitted.fasta"),
                    |source| run.directory().join(&source.path),
                );
                if evidence.source.is_none() {
                    params.insert("original_source".into(), json!(stable.canonicalize()?));
                }
                params.insert(
                    "input_evidence".into(),
                    json!({
                        "run_directory": run.directory(), "path": stable.canonicalize()?,
                        "source_hash": evidence.source.as_ref().map(|s| &s.hash),
                        "submitted_hash": evidence.submitted_hash,
                        "transformation": evidence.transformation, "state": evidence.state,
                    }),
                );
            }
        }
        params.insert(
            "input".into(),
            serde_json::to_value(frozen_input).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        let tool_output = if self.species.is_some() {
            self.output.clone()
        } else if let Some(run) = &self.artifacts {
            run.staging(
                self.output
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| Error::Protocol("output filename is not UTF-8".into()))?,
            )?
        } else {
            self.output.clone()
        };
        params.insert(
            "output".into(),
            serde_json::to_value(&tool_output).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        let cache = if manifest.id == "basecraft.distance" {
            if let Some(run) = &self.artifacts {
                let mut protected = vec![
                    self.source_path.clone(),
                    frozen_input.into(),
                    self.output.clone(),
                    tool_output.clone(),
                    PathBuf::from(format!("{}.run.json", self.output.display())),
                ];
                protected.extend([
                    self.expected_bootstrap_path(run, false)?,
                    self.expected_bootstrap_path(run, true)?,
                ]);
                protected.extend(
                    ["progress", "cancel_file", "groups"]
                        .into_iter()
                        .filter_map(|key| {
                            params
                                .get(key)
                                .and_then(serde_json::Value::as_str)
                                .map(PathBuf::from)
                        }),
                );
                crate::distance_cache::DistanceCache::prepare(
                    run,
                    &mut params,
                    &tool_output,
                    &protected,
                )?
            } else {
                None
            }
        } else {
            None
        };
        let mut result =
            process.invoke(&self.operation, serde_json::Value::Object(params), None)?;
        if !tool_output.is_file() {
            return Err(Error::Protocol(format!(
                "{} wrote no {}",
                manifest.id,
                tool_output.display()
            )));
        }
        if let Some(run) = &self.artifacts {
            // The engine already published bootstrap streams. Only outputs in
            // this run's pending directory are promoted by the host.
            let pending = run.directory().join("pending");
            let mut paths = vec![tool_output.clone()];
            paths.extend(
                result
                    .outputs
                    .iter()
                    .map(PathBuf::from)
                    .filter(|p| *p != tool_output),
            );
            let mut allocated = Vec::new();
            for path in paths {
                if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                    return Err(Error::Protocol("plugin returned a symlink artifact".into()));
                }
                let path = path.canonicalize()?;
                if path.parent() == Some(pending.as_path()) {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| Error::Protocol("artifact filename is not UTF-8".into()))?;
                    let visible = path == tool_output
                        || self.operation == "tree"
                            && path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("nwk"));
                    allocated.push((path.clone(), Some((name.to_owned(), visible))));
                } else if path.starts_with(run.directory())
                    || path == self.expected_bootstrap_path(run, false)?
                    || path == self.expected_bootstrap_path(run, true)?
                {
                    allocated.push((path, None)); // Register; never republish/copy.
                } else {
                    return Err(Error::Protocol(format!(
                        "plugin returned unallocated artifact {}; publication incomplete; staged data retained",
                        path.display()
                    )));
                }
            }
            // Validate the entire return before publishing anything. Retain
            // support files and finish declared secondary exports before making
            // primary/component results visible. These steps are not a transaction.
            for (_, allocation) in &allocated {
                if let Some((name, true)) = allocation {
                    let destination = run.destination(
                        basecraft_ops::analysis_store::ArtifactDestination::WorkingDirectory,
                        name,
                    )?;
                    if cache.as_ref().is_some_and(|c| c.is_secondary(&destination)) {
                        return Err(Error::Protocol(
                            "secondary cache collides with a scientific result".into(),
                        ));
                    }
                }
            }
            let mut published = std::collections::HashMap::new();
            for (path, allocation) in &allocated {
                if let Some((name, false)) = allocation {
                    let destination = publish_artifact(
                        run,
                        name,
                        basecraft_ops::analysis_store::ArtifactDestination::Metadata,
                    )?;
                    published.insert(path.clone(), destination);
                }
            }
            let secondary = if let Some(cache) = &cache {
                cache
                    .finish(run)
                    .map_err(|error| self.publication_failure(run, error))?
            } else {
                Vec::new()
            };
            for (path, allocation) in &allocated {
                if let Some((name, true)) = allocation {
                    let destination = publish_artifact(
                        run,
                        name,
                        basecraft_ops::analysis_store::ArtifactDestination::WorkingDirectory,
                    )?;
                    published.insert(path.clone(), destination);
                }
            }
            result.outputs = allocated
                .into_iter()
                .map(|(path, _)| {
                    published
                        .remove(&path)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect();
            result.outputs.extend(secondary);
            if self.operation == "tree" {
                run.bind(&self.output)?;
            }
        }
        if let Some(storage) = &self.species {
            if let Some(primary) = &storage.primary {
                let value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&self.output)?)?;
                if !value["workspace"].is_null() {
                    let work = serde_json::from_value(value["workspace"].clone())?;
                    basecraft_ops::delimitation::publish_workspace(
                        primary,
                        &work,
                        self.artifacts
                            .as_ref()
                            .ok_or_else(|| Error::Protocol("Missing species run".into()))?,
                    )?;
                    result.outputs.insert(0, primary.display().to_string());
                    result.outputs.push(
                        basecraft_ops::files::tree_sidecar(primary)
                            .display()
                            .to_string(),
                    );
                }
            }
        }
        if self.operation == "tree" {
            let mut trees = vec![self.output.canonicalize()?];
            for path in &result.outputs {
                let path = PathBuf::from(path);
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nwk"))
                    && !cache.as_ref().is_some_and(|c| c.is_secondary(&path))
                {
                    let path = path.canonicalize()?;
                    if !trees.contains(&path) {
                        trees.push(path);
                    }
                }
            }
            let support = producer_support(
                &manifest.id,
                "tree",
                &serde_json::Value::Object(self.params.clone()),
            );
            if let Some((name, kind)) = support {
                for tree in &trees {
                    if let Some(run) = &self.artifacts {
                        run.bind(tree)?;
                    }
                    if let Some(metadata) = write_support_sidecar(tree, name, kind)? {
                        result.outputs.push(metadata.to_string_lossy().into_owned());
                    }
                }
            }
            let artifacts = if let Some(run) = &self.artifacts {
                basecraft_ops::links::AlignmentLink::create_referenced(
                    &self.output,
                    &self.source_path,
                    frozen_input,
                    self.source_version,
                    run,
                )?
            } else {
                basecraft_ops::links::AlignmentLink::create(
                    &self.output,
                    &self.source_path,
                    frozen_input,
                    self.source_version,
                    true,
                    Some(&self.source_hash),
                )?
            };
            result
                .outputs
                .extend(artifacts.iter().map(|p| p.to_string_lossy().into_owned()));
            let link = basecraft_ops::links::AlignmentLink::read(&self.output)?;
            for tree in trees.into_iter().skip(1) {
                let metadata = link.share_to(&self.output, &tree)?;
                result.outputs.push(metadata.to_string_lossy().into_owned());
            }
        }
        Ok(result)
    }

    fn publication_failure(
        &self,
        run: &basecraft_ops::analysis_store::ArtifactRun,
        failure: Error,
    ) -> Error {
        let report = run.directory().join(format!(
            "{}.run.json",
            self.output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        if let Ok(bytes) = std::fs::read(&report) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(collection) = value.pointer("/bootstrap/replicate_trees") {
                    if self
                        .valid_bootstrap_artifact(run, collection)
                        .unwrap_or(false)
                    {
                        return Error::Protocol(format!(
                            "{failure}; retained bootstrap artifacts: {collection}; report: {}",
                            report.display()
                        ));
                    }
                }
            }
        }
        failure
    }

    /// Log the run against the document its input came from.
    pub fn record(&self, session: &mut Session, doc: DocId, result: &InvokeResult) -> Result<()> {
        let manifest = Manifest::load(&self.plugin)?;
        let actor = result.tool.as_ref().map_or_else(
            || Actor::Plugin {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
            },
            |t| Actor::Tool {
                tool: t.name.clone(),
                version: t.version.clone(),
            },
        );
        let primary = self
            .species
            .as_ref()
            .and_then(|s| s.primary.as_ref())
            .filter(|p| p.is_file());
        let mut outputs = if let Some(primary) = primary {
            vec![primary.canonicalize()?, self.output.canonicalize()?]
        } else {
            vec![self.output.canonicalize()?]
        };
        for path in &result.outputs {
            let path = PathBuf::from(path).canonicalize()?;
            if !outputs.contains(&path) {
                outputs.push(path);
            }
        }
        let mut recorded_parameters =
            crate::compute::parameters(&manifest.id, self.params.clone(), self.execution_budget());
        recorded_parameters.insert("job_id".into(), json!(self.run_id));
        if let Some(execution) = &result.execution {
            recorded_parameters.insert(
                "_execution".into(),
                serde_json::to_value(execution).map_err(|e| Error::Protocol(e.to_string()))?,
            );
        }
        if let Some(tool) = &result.tool {
            recorded_parameters.insert(
                "_observed_tool".into(),
                serde_json::to_value(tool).map_err(|e| Error::Protocol(e.to_string()))?,
            );
        }
        if self.artifacts.is_none() {
            session.snapshot_file(&self.retained_input()?)?;
        }
        if let Some(run) = &self.artifacts {
            if let Some(storage) = &self.species {
                let mut input = basecraft_ops::input_evidence::read_input(run.directory())?;
                let mut upstream = Vec::new();
                for dependency in &storage.dependencies {
                    let bytes =
                        basecraft_ops::provenance::read_record(&dependency.join("manifest.json"))?;
                    let publication: basecraft_protocol::provenance::Publication =
                        serde_json::from_slice(&bytes)?;
                    upstream.push(json!({"bundle":basecraft_ops::input_evidence::relative(run.directory(), dependency)?,"id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)}));
                }
                if !upstream.is_empty() {
                    input.extra.insert("upstream".into(), json!(upstream));
                }
                basecraft_ops::input_evidence::write_json(
                    &run.directory().join("input.json"),
                    &input,
                )?;
            }
            session.log_referenced_tool_run(
                doc,
                actor,
                (&manifest.id, &self.operation),
                serde_json::Value::Object(recorded_parameters),
                std::slice::from_ref(&self.input_key),
                &outputs,
            )?;
            return session
                .publish_referenced_analysis(
                    doc,
                    self.species
                        .as_ref()
                        .and_then(|s| s.primary.as_deref())
                        .filter(|p| p.is_file())
                        .unwrap_or(&self.output),
                    &if manifest.id == "basecraft.distance" {
                        crate::distance_cache::publication_outputs(run, &outputs)?
                    } else {
                        outputs.clone()
                    },
                    &self.source_record,
                    run,
                )
                .map_err(Into::into);
        }
        session.log_tool_run(
            doc,
            actor,
            (&manifest.id, &self.operation),
            serde_json::Value::Object(recorded_parameters),
            std::slice::from_ref(&self.input_key),
            &outputs,
        )?;
        session.publish_analysis_snapshot(
            doc,
            &self.output,
            std::slice::from_ref(&self.input_key),
            &self.source_record,
        )?;
        Ok(())
    }
}

fn file_input_record(
    input: &Path,
    plugin: &Path,
    operation: &str,
) -> Result<basecraft_ops::document_record::DocumentRecord> {
    let format = basecraft_ops::files::Format::from_path(input).ok();
    let tree = matches!(
        operation,
        "workspace_tree"
            | "gmyc_single"
            | "gmyc_multiple"
            | "gmyc_exhaustive"
            | "ptp"
            | "mptp"
            | "manual_cut"
            | "inspect_tree"
            | "validate_time_tree"
            | "relative_time"
    );
    let species = Manifest::load(plugin)?.id == "basecraft.delimitation";
    if (species || operation == "tree")
        && (format.is_none() || tree && input.file_name().is_some_and(|n| n == "submitted.fasta"))
    {
        // Preserved originals and exported content-addressed artifacts need no
        // format-bearing filename. The method validates their actual bytes;
        // absence of an imported document history stays an honest boundary.
        return Ok(basecraft_ops::document_record::DocumentRecord::empty(
            &input.display().to_string(),
        ));
    }
    basecraft_ops::document_record::DocumentRecord::from_file(input).map_err(Into::into)
}

/// `<stem>.<suffix>.<ext>` beside `file`, numbered if that name is taken:
/// `rbcL.fasta` gives `rbcL.mafft.fasta`, then `rbcL.mafft-2.fasta`.
pub fn output_beside(file: &Path, suffix: &str, ext: &str) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // Drop the extension, and an `.aln` before it.
    let mut stem = name.split('.').next().unwrap_or("alignment").to_string();
    if stem.is_empty() {
        stem = "alignment".into();
    }
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(format!("{stem}.{suffix}.{ext}"))
        } else {
            dir.join(format!("{stem}.{suffix}-{n}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Shared immutable-file preparation for legacy and registered jobs.
pub(crate) fn freeze_file(source: &Path, destination: &Path) -> Result<String> {
    let before = basecraft_ops::input_evidence::stamp(source)?;
    std::fs::copy(source, destination)?;
    let hash = basecraft_io::hash_file(destination).map_err(basecraft_ops::Error::from)?;
    if before != basecraft_ops::input_evidence::stamp(source)?
        || basecraft_io::hash_file(source).map_err(basecraft_ops::Error::from)? != hash
    {
        return Err(Error::Protocol("input changed during submission".into()));
    }
    Ok(hash)
}

/// Both execution families promote only validated staging files through the
/// common store's no-clobber primitive.
pub(crate) fn publish_artifact(
    run: &basecraft_ops::analysis_store::ArtifactRun,
    name: &str,
    location: basecraft_ops::analysis_store::ArtifactDestination,
) -> Result<PathBuf> {
    Ok(run.publish(name, location)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_number_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rbcL.aln.fasta");
        let first = output_beside(&file, "mafft", "fasta");
        assert_eq!(first, dir.path().join("rbcL.mafft.fasta"));
        std::fs::write(&first, "").unwrap();
        assert_eq!(
            output_beside(&file, "mafft", "fasta"),
            dir.path().join("rbcL.mafft-2.fasta")
        );
    }
}
