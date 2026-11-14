//! Tree–alignment links: the linked file by path, size, modification time
//! and hash, with no copy (D25), or the exact input of the run that inferred
//! the tree.
use crate::{Result, error::invalid};
use basecraft_docs::TreeDoc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use basecraft_protocol::link_records::{
    AlignmentLinkRecovery, AlignmentLinkUnavailable, PreviousAlignmentMetadata as PreviousMetadata,
};
pub use basecraft_protocol::records::AlignmentLink;

fn hidden_sidecar(run: &Path, tree: &Path) -> PathBuf {
    let digest = basecraft_io::hash_bytes(tree.file_name().unwrap_or_default().as_encoded_bytes());
    run.join(format!(
        "{}.alignment.json",
        digest.trim_start_matches("b3:")
    ))
}

/// The alignment link of a loose tree.
pub fn sidecar(tree: &Path) -> PathBuf {
    sidecar_in(None, tree)
}

/// Where a tree's alignment link lives: in the run a bound result belongs
/// to, else with the tree's other records in the `.basecraft` that holds
/// them (D23), never beside the tree.
pub fn sidecar_in(project: Option<&crate::project::Project>, tree: &Path) -> PathBuf {
    if let Ok(Some(run)) = crate::records::analysis_store::ArtifactRun::for_result(tree) {
        return hidden_sidecar(&run, tree);
    }
    crate::project::Project::document_record_with(project, tree, ".alignment.json")
}

/// A presentation association has its own link and never overwrites inference input.
fn view_tree(tree: &Path) -> PathBuf {
    let mut name = tree.as_os_str().to_os_string();
    name.push(".view");
    PathBuf::from(name)
}

pub fn view_sidecar(tree: &Path) -> PathBuf {
    view_sidecar_in(None, tree)
}

pub fn view_sidecar_in(project: Option<&crate::project::Project>, tree: &Path) -> PathBuf {
    sidecar_in(project, &view_tree(tree))
}

/// Atomic saves replace the named directory entry, not a symlink's referent.
pub(crate) fn document_entry(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let filename = path
        .file_name()
        .ok_or_else(|| invalid("tree filename is missing"))?;
    Ok(parent.canonicalize()?.join(filename))
}

/// A dangling symlink is an explicit entry, not an absent association.
/// Errors inspecting an association must propagate rather than enable fallback.
pub fn metadata_present(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Keep a uniquely named file with the tree's link records, never beside it.
fn retain_file(
    project: Option<&crate::project::Project>,
    tree: &Path,
    suffix: &str,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> Result<PathBuf> {
    let prefix = format!(
        "{}.",
        tree.file_name().unwrap_or_default().to_string_lossy()
    );
    let link = sidecar_in(project, tree);
    crate::files::companion_folder(&link)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(suffix)
        .tempfile_in(link.parent().unwrap_or(Path::new(".")))?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    let (_, path) = temporary.keep().map_err(|error| error.error)?;
    Ok(path)
}

/// Keep filesystem inspection in ops; protocol owns only the wire declaration.
fn previous_metadata(path: &Path) -> Result<PreviousMetadata> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(PreviousMetadata::Symlink(std::fs::read_link(path)?))
        }
        Ok(metadata) if metadata.is_file() => Ok(PreviousMetadata::File(std::fs::read(path)?)),
        Ok(_) => Err(crate::Error::Damaged {
            path: path.to_path_buf(),
            detail: format!("association metadata is not a file: {}", path.display()),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PreviousMetadata::Absent),
        Err(error) => Err(error.into()),
    }
}

struct StagedLink {
    metadata: Vec<u8>,
}

impl StagedLink {
    /// The link as the tree at `to` records it: paths relative to `to`.
    fn copy(link: &AlignmentLink, from: &Path, to: &Path) -> Result<Self> {
        let mut copied = link.clone();
        copied.source = relative_to(&link.source, to)?;
        if link.schema == 2 {
            copied.analysis_run = Some(relative_to(&link.evidence_run(from)?, to)?);
        }
        Ok(Self {
            metadata: basecraft_protocol::records::encode(&copied)?,
        })
    }
}

/// Prepare all source associations before replacing a tree. Publication uses an
/// explicit unavailable view entry until both metadata states are committed.
/// A crash/error leaves extraction disabled and a recovery record beside the tree.
pub(crate) struct PreparedLinks {
    project: Option<crate::project::Project>,
    target: PathBuf,
    origin: Option<StagedLink>,
    view: Option<StagedLink>,
    previous_origin: PreviousMetadata,
    previous_view: PreviousMetadata,
}

impl PreparedLinks {
    pub(crate) fn prepare(
        project: Option<&crate::project::Project>,
        from: &Path,
        to: &Path,
    ) -> Result<Self> {
        let origin = metadata_present(&sidecar_in(project, from))?
            .then(|| AlignmentLink::read_in(project, from))
            .transpose()?;
        Self::with_origin(project, origin.as_ref(), from, to)
    }

    fn with_origin(
        project: Option<&crate::project::Project>,
        origin: Option<&AlignmentLink>,
        from: &Path,
        to: &Path,
    ) -> Result<Self> {
        let view_path = view_tree(from);
        let view = metadata_present(&sidecar_in(project, &view_path))?
            .then(|| AlignmentLink::read_in(project, &view_path))
            .transpose()?;
        // Check every run input before creating destination evidence or
        // touching its tree. A linked file is checked when it is used.
        for link in origin.into_iter().chain(view.iter()) {
            if link.schema == 2 {
                link.verified_run_input(from)?;
            }
        }
        let previous_origin = previous_metadata(&sidecar_in(project, to))?;
        let previous_view = previous_metadata(&view_sidecar_in(project, to))?;
        Ok(Self {
            project: project.cloned(),
            target: to.to_path_buf(),
            origin: origin
                .map(|link| StagedLink::copy(link, from, to))
                .transpose()?,
            view: view
                .as_ref()
                .map(|link| StagedLink::copy(link, from, &view_tree(to)))
                .transpose()?,
            previous_origin,
            previous_view,
        })
    }

    pub(crate) fn publish<T>(
        &self,
        save_data: impl FnOnce() -> Result<T>,
    ) -> Result<(T, Vec<PathBuf>)> {
        // Convert fallibly before retaining the recovery file or installing guards.
        // A prior symlink may contain a non-UTF-8 path that JSON cannot represent.
        let recovery = basecraft_protocol::records::encode(&AlignmentLinkRecovery {
            schema: 1,
            note: "Save As association recovery: previous entries and intended metadata".into(),
            previous_origin: self.previous_origin.clone(),
            previous_view: self.previous_view.clone(),
            intended_origin: self.origin.as_ref().map(|link| link.metadata.clone()),
            intended_view: self.view.as_ref().map(|link| link.metadata.clone()),
        })?;
        let project = self.project.as_ref();
        let recovery_path =
            retain_file(project, &self.target, ".alignment-recovery.json", |out| {
                out.write_all(&recovery)
            })?;
        let unavailable = basecraft_protocol::records::encode(&AlignmentLinkUnavailable {
            schema: 1,
            unavailable: "Save As alignment publication incomplete".into(),
            recovery: Some(relative_to(&recovery_path, &self.target)?.into_os_string()),
        })?;
        // Install the guard before changing data or origin. If this fails, the old
        // tree and all its associations are untouched. Never remove a failed guard.
        let view_link = view_sidecar_in(project, &self.target);
        crate::files::companion_folder(&view_link)?;
        basecraft_io::write_atomic(&view_link, |out| out.write_all(&unavailable))?;
        let finish = || -> Result<(T, Vec<PathBuf>)> {
            // The scientific origin must also become unavailable before replacing
            // data, so partial files cannot claim the overwritten tree's origin.
            basecraft_io::write_atomic(&sidecar_in(project, &self.target), |out| {
                out.write_all(&unavailable)
            })?;
            let result = save_data()?;
            let mut outputs = vec![recovery_path.clone()];
            for (metadata, staged) in [
                (sidecar_in(project, &self.target), &self.origin),
                (view_sidecar_in(project, &self.target), &self.view),
            ] {
                if let Some(link) = staged {
                    basecraft_io::write_atomic(&metadata, |out| out.write_all(&link.metadata))?;
                    outputs.push(metadata);
                } else if metadata_present(&metadata)? {
                    // Intentional absence is part of Save As state. Prior entries
                    // are preserved in recovery.
                    std::fs::remove_file(&metadata)?;
                }
            }
            Ok((result, outputs))
        };
        finish().map_err(|error| crate::Error::Damaged { path: recovery_path.clone(), detail: format!(
            "Save As incomplete; alignment association may be unavailable; recovery metadata at {}: {error}", recovery_path.display()
        ) })
    }
}

/// Store portable references relative to the document's directory where possible.
fn relative_to(path: &Path, tree: &Path) -> Result<PathBuf> {
    let parent = tree.parent().unwrap_or(Path::new(".")).canonicalize()?;
    // A relative path names a file beside the document, never one in the
    // process's working folder.
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        parent.join(path)
    };
    let base: Vec<_> = parent.components().collect();
    let target: Vec<_> = path.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return Ok(path);
    }
    let mut out = PathBuf::new();
    for _ in common..base.len() {
        out.push("..");
    }
    for part in &target[common..] {
        out.push(part.as_os_str());
    }
    Ok(out)
}

fn load_alignment(path: &Path) -> Result<basecraft_docs::AlignmentDoc> {
    let file = std::sync::Arc::new(basecraft_io::fasta::IndexedFasta::open(path)?);
    Ok(basecraft_docs::AlignmentDoc::from_fasta(
        "Linked input",
        &file,
    ))
}

fn names(path: &Path) -> Result<BTreeSet<String>> {
    let alignment = basecraft_io::fasta::IndexedFasta::open(path)?;
    let mut names = BTreeSet::new();
    let mut width = None;
    for row in &alignment.index.records {
        if row.name.is_empty() || !names.insert(row.name.clone()) {
            return Err(invalid("alignment identifiers must be nonempty and unique"));
        }
        if width.is_some_and(|w| w != row.len) || row.len == 0 {
            return Err(invalid("linked sequences must be nonempty and aligned"));
        }
        width = Some(row.len);
    }
    if names.is_empty() {
        return Err(invalid("empty alignment"));
    }
    Ok(names)
}

pub fn validate_taxa(tree: &TreeDoc, alignment: &Path, allow_extra: bool) -> Result<()> {
    let names = names(alignment)?;
    let mut tips = BTreeSet::new();
    for node in tree.nodes().iter().filter(|n| n.is_tip()) {
        if node.name.is_empty() || !tips.insert(node.name.clone()) {
            return Err(invalid("tree tip identifiers must be nonempty and unique"));
        }
    }
    let missing: Vec<_> = tips.difference(&names).cloned().collect();
    let extra: Vec<_> = names.difference(&tips).cloned().collect();
    if !missing.is_empty() || !allow_extra && !extra.is_empty() {
        return Err(invalid(format!(
            "tree/alignment identifiers differ; missing from alignment: {}; extra alignment rows: {}",
            missing.join(", "),
            extra.join(", ")
        )));
    }
    Ok(())
}

/// Operations on the shared alignment-link wire record.
///
/// Rust callers must import this trait alongside `AlignmentLink` to use its
/// associated functions and methods. Serialization and public fields are unchanged.
pub trait AlignmentLinkExt: Sized {
    /// For a loose tree; see [`AlignmentLinkExt::associate_in`].
    fn associate(tree_path: &Path, tree: &TreeDoc, source: &Path) -> Result<Vec<PathBuf>> {
        Self::associate_in(None, tree_path, tree, source)
    }
    fn associate_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
        source: &Path,
    ) -> Result<Vec<PathBuf>>;
    fn create_referenced(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        run: &crate::records::analysis_store::ArtifactRun,
    ) -> Result<Vec<PathBuf>>;
    /// For a loose tree; see [`AlignmentLinkExt::create_in`].
    fn create(tree: &Path, source: &Path, version: u64, inferred: bool) -> Result<Vec<PathBuf>> {
        Self::create_in(None, tree, source, version, inferred)
    }
    fn create_in(
        project: Option<&crate::project::Project>,
        tree: &Path,
        source: &Path,
        version: u64,
        inferred: bool,
    ) -> Result<Vec<PathBuf>>;
    /// For a loose tree; see [`AlignmentLinkExt::read_in`].
    fn read(tree: &Path) -> Result<Self> {
        Self::read_in(None, tree)
    }
    fn read_in(project: Option<&crate::project::Project>, tree: &Path) -> Result<Self>;
    /// For a loose tree; see [`AlignmentLinkExt::read_view_in`].
    fn read_view(tree: &Path) -> Result<Self> {
        Self::read_view_in(None, tree)
    }
    fn read_view_in(project: Option<&crate::project::Project>, tree: &Path) -> Result<Self>;
    /// The tree's alignment, checked as D25 says, with the link it came from:
    /// the explicit view link if there is one, else the tree's own. A linked
    /// file whose size or time changed but whose hash did not has its stored
    /// time updated; a changed file is [`crate::Error::AlignmentChanged`].
    fn checked_view_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
    ) -> Result<(Self, PathBuf)>;
    /// [`AlignmentLinkExt::checked_view_in`] for the tree's own link.
    fn checked_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
    ) -> Result<(Self, PathBuf)>;
    fn status(&self, tree_path: &Path, tree: &TreeDoc) -> String;
    /// The alignment to use; writes nothing. See
    /// [`AlignmentLinkExt::checked_view_in`].
    fn checked_alignment(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf>;
    /// For loose trees; see [`AlignmentLinkExt::share_to_in`].
    fn share_to(&self, from: &Path, to: &Path) -> Result<PathBuf> {
        self.share_to_in(None, from, to)
    }
    fn share_to_in(
        &self,
        project: Option<&crate::project::Project>,
        from: &Path,
        to: &Path,
    ) -> Result<PathBuf>;
    /// For loose trees; see [`AlignmentLinkExt::copy_to_in`].
    fn copy_to(&self, from: &Path, to: &Path) -> Result<Vec<PathBuf>> {
        self.copy_to_in(None, from, to)
    }
    fn copy_to_in(
        &self,
        project: Option<&crate::project::Project>,
        from: &Path,
        to: &Path,
    ) -> Result<Vec<PathBuf>>;
    fn source_changed(&self) -> bool;
}

// Evidence helpers remain private to ops; protocol contains only wire data.
trait LinkEvidence {
    fn evidence_run(&self, tree_path: &Path) -> Result<PathBuf>;
    fn verified_run_input(&self, tree_path: &Path) -> Result<PathBuf>;
    fn linked_file(&self) -> Result<LinkedFile>;
}

/// A linked file's size and modification time. Saves replace files, so the
/// device and inode are not kept.
fn file_stamp(path: &Path) -> Result<basecraft_protocol::provenance::FileStamp> {
    Ok(basecraft_protocol::provenance::FileStamp {
        device: None,
        inode: None,
        ..crate::records::input_evidence::stamp(path)?
    })
}

/// What a schema 1 link finds on disk.
enum LinkedFile {
    /// Same size and time as recorded.
    Unchanged,
    /// Size or time differ, the bytes do not: the new stamp.
    Touched(basecraft_protocol::provenance::FileStamp),
    Changed,
}

impl LinkEvidence for AlignmentLink {
    fn linked_file(&self) -> Result<LinkedFile> {
        let stamp = file_stamp(&self.source)?;
        if self.source_stamp.as_ref() == Some(&stamp) {
            Ok(LinkedFile::Unchanged)
        } else if basecraft_io::hash_file(&self.source)? == self.source_hash {
            Ok(LinkedFile::Touched(stamp))
        } else {
            Ok(LinkedFile::Changed)
        }
    }

    fn evidence_run(&self, tree_path: &Path) -> Result<PathBuf> {
        let name = self
            .analysis_run
            .as_deref()
            .ok_or_else(|| invalid("reference association has no artifact run"))?;
        let run = tree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(name)
            .canonicalize()?;
        // Never trust an arbitrary run supplied by association metadata.
        if let Some(bound) = crate::records::analysis_store::ArtifactRun::for_result(tree_path)?
            && bound != run
        {
            return Err(invalid(
                "reference association disagrees with result binding",
            ));
        }
        let store = run
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("reference run has no store"))?;
        crate::records::analysis_store::ArtifactRun::validate_store(store)?;
        Ok(run)
    }

    fn verified_run_input(&self, tree_path: &Path) -> Result<PathBuf> {
        let submitted = self
            .submitted_hash
            .as_deref()
            .ok_or_else(|| invalid("reference association has no submitted input"))?;
        let run = self.evidence_run(tree_path)?;
        let input = crate::records::input_evidence::read_input(&run)?;
        if input.state == basecraft_protocol::input_records::EvidenceState::Broken {
            return Err(invalid("alignment evidence explicitly broken"));
        }
        let source = if let Some(reference) = &input.source {
            crate::records::input_evidence::verified_source(&run, reference)?
        } else {
            run.join("submitted.fasta")
        };
        let alignment = load_alignment(&source)?;
        let mut hashing = crate::files::HashingWriter::new(std::io::sink());
        crate::files::write_alignment(&alignment, &mut hashing)?;
        let raw = input.transformation == "identity-v1";
        let actual = if raw {
            basecraft_io::hash_file(&source)?
        } else {
            hashing.hash()
        };
        if actual != submitted {
            return Err(invalid(
                "linked input differs from actual submitted alignment",
            ));
        }
        // Callers need a typed FASTA path. Materialize a disposable
        // canonical cache only on explicit use, after verifying the original;
        // never treat this cache as repair for a missing/changed reference.
        if crate::files::Format::from_path(&source).ok() != Some(crate::files::Format::Fasta)
            || !raw && input.source.as_ref().is_some_and(|r| r.hash != submitted)
        {
            let paths = basecraft_platform::paths::AppPaths::discover()
                .map_err(|e| invalid(e.to_string()))?;
            paths.remove_old_caches();
            let folder = paths.display_cache();
            std::fs::create_dir_all(&folder)?;
            let digest = submitted
                .strip_prefix("b3:")
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| invalid("invalid submitted input digest"))?;
            let target = folder.join(format!("{digest}.afa"));
            basecraft_io::write_atomic(&target, |w| {
                if raw {
                    std::io::copy(&mut std::fs::File::open(&source)?, w)?;
                    Ok(())
                } else {
                    crate::files::write_alignment(&alignment, w)
                }
            })?;
            return Ok(target);
        }
        Ok(source)
    }
}

impl AlignmentLinkExt for AlignmentLink {
    /// Link an imported tree to an alignment file.
    fn associate_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
        source: &Path,
    ) -> Result<Vec<PathBuf>> {
        validate_taxa(tree, source, false)?;
        let origin_inferred = metadata_present(&sidecar_in(project, tree_path))?
            && Self::read_in(project, tree_path)?.inferred;
        let target = if origin_inferred || metadata_present(&view_sidecar_in(project, tree_path))? {
            view_tree(tree_path)
        } else {
            tree_path.to_path_buf()
        };
        Self::create_in(project, &target, source, 0, false)
    }

    /// Reference-first inferred association. Clean disk input is never copied;
    /// dirty submitted input is already retained once by the job's artifact run.
    fn create_referenced(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        run: &crate::records::analysis_store::ArtifactRun,
    ) -> Result<Vec<PathBuf>> {
        names(input)?;
        let record = crate::records::input_evidence::read_input(run.directory())?;
        let link = Self {
            schema: 2,
            source: relative_to(&std::path::absolute(source)?, tree)?,
            source_hash: record
                .source
                .as_ref()
                .map_or_else(String::new, |r| r.hash.clone()),
            source_stamp: None,
            submitted_hash: Some(record.submitted_hash),
            alignment_version: version,
            inferred: true,
            analysis_run: Some(relative_to(run.directory(), tree)?),
            extra: BTreeMap::new(),
        };
        let metadata = hidden_sidecar(run.directory(), tree);
        crate::records::input_evidence::write_json(&metadata, &link)?;
        run.bind(tree)?;
        Ok(vec![metadata])
    }

    /// Link `tree` to the alignment file `source` as it is now.
    fn create_in(
        project: Option<&crate::project::Project>,
        tree: &Path,
        source: &Path,
        version: u64,
        inferred: bool,
    ) -> Result<Vec<PathBuf>> {
        names(source)?;
        let source = source.canonicalize()?;
        let link = Self {
            schema: 1,
            source_hash: basecraft_io::hash_file(&source)?,
            source_stamp: Some(file_stamp(&source)?),
            source: relative_to(&source, tree)?,
            submitted_hash: None,
            alignment_version: version,
            inferred,
            analysis_run: None,
            extra: BTreeMap::new(),
        };
        let metadata = sidecar_in(project, tree);
        crate::files::companion_folder(&metadata)?;
        let bytes = basecraft_protocol::records::encode(&link)?;
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes))?;
        Ok(vec![metadata])
    }

    fn read_in(project: Option<&crate::project::Project>, tree: &Path) -> Result<Self> {
        crate::records::analysis_store::ArtifactRun::for_result(tree)?;
        let (metadata, writer) = basecraft_protocol::records::decode_written::<serde_json::Value>(
            &std::fs::read(sidecar_in(project, tree))?,
        )?;
        if let Some(reason) = metadata
            .get("unavailable")
            .and_then(serde_json::Value::as_str)
        {
            let recovery = metadata.get("recovery").and_then(serde_json::Value::as_str);
            return Err(invalid(recovery.map_or_else(
                || reason.to_owned(),
                |path| {
                    format!(
                        "{reason}; recover association metadata from {}",
                        tree.parent().unwrap_or(Path::new(".")).join(path).display()
                    )
                },
            )));
        }
        let mut link: Self = basecraft_protocol::records::convert(metadata, &writer)?;
        if !matches!(link.schema, 1 | 2) {
            return Err(crate::Error::Unsupported(
                basecraft_protocol::records::other_format(&writer, link.schema, "1 or 2"),
            ));
        }
        if link.source.is_relative() {
            link.source = tree
                .parent()
                .unwrap_or(Path::new("."))
                .canonicalize()?
                .join(&link.source);
        }
        link.source = link.source.canonicalize().unwrap_or(link.source);
        Ok(link)
    }

    /// Prefer an explicit view association. A damaged view link is an error, never a fallback.
    fn read_view_in(project: Option<&crate::project::Project>, tree: &Path) -> Result<Self> {
        Self::read_in(
            project,
            &if metadata_present(&view_sidecar_in(project, tree))? {
                view_tree(tree)
            } else {
                tree.to_path_buf()
            },
        )
    }

    /// Status describes retained evidence and never reads changed source data as input.
    fn status(&self, tree_path: &Path, tree: &TreeDoc) -> String {
        let label = if self.inferred {
            "Source alignment"
        } else {
            "Linked alignment"
        };
        let mut status = format!("{label}: {}", self.source.display());
        if self.schema == 2 {
            return match self.evidence_run(tree_path).and_then(|run| {
                crate::records::input_evidence::read_input(&run).map(|input| (run, input))
            }) {
                Ok((_, input))
                    if input.state
                        == basecraft_protocol::input_records::EvidenceState::Generated =>
                {
                    format!("{status}; exact submitted input retained")
                }
                Ok((_, input))
                    if input.state
                        == basecraft_protocol::input_records::EvidenceState::Preserved =>
                {
                    format!("{status}; original preserved (verify before use)")
                }
                Ok((_, input))
                    if input.state == basecraft_protocol::input_records::EvidenceState::Broken =>
                {
                    format!("{status}; evidence explicitly broken")
                }
                Ok((run, input)) => {
                    if input.source.as_ref().is_some_and(|r| {
                        crate::records::input_evidence::stamp(&run.join(&r.path)).is_ok_and(|s| {
                            s.size == r.stamp.size
                                && s.modified_seconds == r.stamp.modified_seconds
                                && s.modified_nanos == r.stamp.modified_nanos
                        })
                    }) {
                        format!(
                            "{status}; referenced input metadata unchanged (content not rechecked)"
                        )
                    } else {
                        format!("{status}; original missing or changed; verify or relink original")
                    }
                }
                Err(e) => format!("{status}; evidence unavailable: {e}"),
            };
        }
        match self.linked_file() {
            Err(_) => status.push_str("; file missing or moved"),
            Ok(LinkedFile::Changed) => status.push_str(
                "; changed since it was linked: link alignment again to use the current file, or choose another",
            ),
            Ok(_) => match validate_taxa(tree, &self.source, self.inferred) {
                Ok(()) => status.push_str("; unchanged since it was linked"),
                Err(e) => {
                    let _ = write!(status, "; cannot be used with this tree: {e}");
                }
            },
        }
        status
    }

    fn checked_alignment(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf> {
        let alignment = if self.schema == 2 {
            self.verified_run_input(tree_path)?
        } else if matches!(self.linked_file()?, LinkedFile::Changed) {
            return Err(crate::Error::AlignmentChanged {
                path: self.source.clone(),
            });
        } else {
            self.source.clone()
        };
        validate_taxa(tree, &alignment, self.inferred)?;
        Ok(alignment)
    }

    fn checked_view_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
    ) -> Result<(Self, PathBuf)> {
        let linked = if metadata_present(&view_sidecar_in(project, tree_path))? {
            view_tree(tree_path)
        } else {
            tree_path.to_path_buf()
        };
        checked(project, &linked, tree_path, tree)
    }

    fn checked_in(
        project: Option<&crate::project::Project>,
        tree_path: &Path,
        tree: &TreeDoc,
    ) -> Result<(Self, PathBuf)> {
        checked(project, tree_path, tree_path, tree)
    }

    /// Forest members share the same retained alignment instead of copying it per tree.
    fn share_to_in(
        &self,
        project: Option<&crate::project::Project>,
        from: &Path,
        to: &Path,
    ) -> Result<PathBuf> {
        let mut link = self.clone();
        if self.schema == 2 {
            let run = self.evidence_run(from)?;
            link.analysis_run = Some(relative_to(&run, to)?);
            link.source = relative_to(&self.source, to)?;
            // Same-folder forest components share one run; no input copy.
            let metadata = hidden_sidecar(&run, to);
            crate::records::input_evidence::write_json(&metadata, &link)?;
            let original_binding = crate::records::analysis_store::ArtifactRun::for_result(from)?
                .ok_or_else(|| invalid("missing run binding"))?;
            let id = original_binding
                .file_name()
                .ok_or_else(|| invalid("run identity missing"))?
                .to_string_lossy();
            crate::records::analysis_store::bind_existing(&run, &id, to)?;
            return Ok(metadata);
        }
        link.source = relative_to(&self.source, to)?;
        let metadata = sidecar_in(project, to);
        crate::files::companion_folder(&metadata)?;
        let bytes = basecraft_protocol::records::encode(&link)?;
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes))?;
        Ok(metadata)
    }

    /// Preserve a link when a tree is saved under a new name or directory.
    fn copy_to_in(
        &self,
        project: Option<&crate::project::Project>,
        from: &Path,
        to: &Path,
    ) -> Result<Vec<PathBuf>> {
        PreparedLinks::with_origin(project, Some(self), from, to)?
            .publish(|| Ok(()))
            .map(|((), outputs)| outputs)
    }

    fn source_changed(&self) -> bool {
        self.schema == 2 && !self.source.exists()
    }
}

/// Read the link recorded for `linked` and check it. A touched but
/// unchanged file has its new size and time written back to the link.
fn checked(
    project: Option<&crate::project::Project>,
    linked: &Path,
    tree_path: &Path,
    tree: &TreeDoc,
) -> Result<(AlignmentLink, PathBuf)> {
    let mut link = AlignmentLink::read_in(project, linked)?;
    if link.schema == 1
        && let LinkedFile::Touched(stamp) = link.linked_file()?
    {
        let mut stored = link.clone();
        stored.source = relative_to(&link.source, linked)?;
        stored.source_stamp = Some(stamp.clone());
        let bytes = basecraft_protocol::records::encode(&stored)?;
        basecraft_io::write_atomic(&sidecar_in(project, linked), |out| out.write_all(&bytes))?;
        link.source_stamp = Some(stamp);
    }
    let alignment = link.checked_alignment(tree_path, tree)?;
    Ok((link, alignment))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_save_as_carries_the_link_and_replays() {
        crate::isolated_profile::isolate!();
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.afa");
        let tree_path = dir.path().join("a.nwk");
        let output = dir.path().join("copy.nwk");
        std::fs::write(&source, ">a\nA-C\n>b\nATC\n").unwrap();
        std::fs::write(&tree_path, "(a,b);").unwrap();
        let mut session = crate::Session::new(dir.path());
        let doc = session.open(&tree_path).unwrap();
        AlignmentLink::associate(&tree_path, session.tree(doc).unwrap(), &source).unwrap();
        session.save(doc, Some(&output)).unwrap();
        let link = AlignmentLink::read(&output).unwrap();
        assert_eq!(
            link.checked_alignment(&output, session.tree(doc).unwrap())
                .unwrap(),
            source.canonicalize().unwrap()
        );
        assert!(crate::records::replay::replay(dir.path()).unwrap().ok());
    }

    /// D25: same size and time, unchanged; either differs, hash it; same
    /// hash, store the new time; another hash, the alignment changed.
    #[test]
    fn a_linked_file_is_checked_by_time_size_then_hash() {
        crate::isolated_profile::isolate!();
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("a.afa");
        let tree_path = d.path().join("a.nwk");
        std::fs::write(&source, ">a\nA-C\n>b\nATC\n").unwrap();
        let tree = TreeDoc::from_parsed(
            "tree",
            &basecraft_io::newick::parse_newick("(a,b);").unwrap(),
        );
        let outputs = AlignmentLink::associate(&tree_path, &tree, &source).unwrap();
        assert_eq!(outputs, [sidecar(&tree_path)], "no copy of the alignment");
        let link = AlignmentLink::read(&tree_path).unwrap();
        assert!(!link.inferred);
        assert!(link.status(&tree_path, &tree).contains("unchanged"));
        // Rewritten with the same bytes: the new time is stored silently.
        let before = link.source_stamp.clone().unwrap();
        let file = std::fs::File::options().write(true).open(&source).unwrap();
        file.set_modified(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();
        drop(file);
        let (checked, path) = AlignmentLink::checked_in(None, &tree_path, &tree).unwrap();
        assert_eq!(path, source.canonicalize().unwrap());
        assert_ne!(checked.source_stamp.as_ref(), Some(&before));
        let stored = AlignmentLink::read(&tree_path).unwrap();
        assert_eq!(stored.source_stamp, checked.source_stamp);
        // Other bytes: the alignment changed since it was linked.
        std::fs::write(&source, ">a\nAAC\n>b\nATC\n").unwrap();
        assert!(matches!(
            AlignmentLink::checked_in(None, &tree_path, &tree),
            Err(crate::Error::AlignmentChanged { .. })
        ));
        assert!(
            stored
                .status(&tree_path, &tree)
                .contains("changed since it was linked")
        );
        std::fs::write(&source, ">a\nAAC\n>a\nATC\n").unwrap();
        assert!(validate_taxa(&tree, &source, false).is_err());
        std::fs::write(&source, ">a\nAAC\n>c\nATC\n").unwrap();
        assert!(validate_taxa(&tree, &source, false).is_err());
    }
}

#[cfg(test)]
#[path = "links/repair_record_tests.rs"]
mod repair_record_tests;
