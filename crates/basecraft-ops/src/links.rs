//! Explicit tree–alignment provenance. Snapshots retain unsaved input exactly.
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

pub fn sidecar(tree: &Path) -> PathBuf {
    if let Ok(Some(run)) = crate::analysis_store::ArtifactRun::for_result(tree) {
        return hidden_sidecar(&run, tree);
    }
    let mut name = tree.as_os_str().to_os_string();
    name.push(".alignment.json");
    PathBuf::from(name)
}

/// A presentation association has its own snapshot and never overwrites inference input.
fn view_tree(tree: &Path) -> PathBuf {
    let mut name = tree.as_os_str().to_os_string();
    name.push(".view");
    PathBuf::from(name)
}

pub fn view_sidecar(tree: &Path) -> PathBuf {
    sidecar(&view_tree(tree))
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

/// Never replace retained bytes. Metadata is the sole publication point for a
/// new snapshot. Old snapshots (including legacy fixed-name files) remain intact.
fn retain_file(
    tree: &Path,
    suffix: &str,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> Result<PathBuf> {
    let prefix = format!(
        "{}.",
        tree.file_name().unwrap_or_default().to_string_lossy()
    );
    let mut temporary = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(suffix)
        .tempfile_in(tree.parent().unwrap_or(Path::new(".")))?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    let (_, path) = temporary.keep().map_err(|error| error.error)?;
    Ok(path)
}

fn retain_snapshot(tree: &Path, input: &Path, expected: &str) -> Result<PathBuf> {
    let snapshot = retain_file(tree, ".alignment.afa", |out| {
        io::copy(&mut std::fs::File::open(input)?, out)?;
        Ok(())
    })?;
    if basecraft_io::hash_file(&snapshot)? != expected {
        return Err(invalid(format!(
            "alignment changed while retaining its snapshot; staged evidence remains at {}",
            snapshot.display()
        )));
    }
    Ok(snapshot)
}

/// Keep filesystem inspection in ops; protocol owns only the wire declaration.
fn previous_metadata(path: &Path) -> Result<PreviousMetadata> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(PreviousMetadata::Symlink(std::fs::read_link(path)?))
        }
        Ok(metadata) if metadata.is_file() => Ok(PreviousMetadata::File(std::fs::read(path)?)),
        Ok(_) => Err(invalid(format!(
            "association metadata is not a file: {}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PreviousMetadata::Absent),
        Err(error) => Err(error.into()),
    }
}

struct StagedLink {
    snapshot: Option<PathBuf>,
    metadata: Vec<u8>,
}

impl StagedLink {
    fn copy(link: &AlignmentLink, from: &Path, to: &Path) -> Result<Self> {
        if link.schema == 2 {
            link.verify_snapshot(from)?;
            let run = link.evidence_run(from)?;
            let mut copied = link.clone();
            copied.source = relative_to(&link.source, to)?;
            copied.extra.insert(
                "analysis_run".into(),
                serde_json::json!(relative_to(&run, to)?),
            );
            return Ok(Self {
                snapshot: None,
                metadata: serde_json::to_vec_pretty(&copied)?,
            });
        }
        let snapshot = retain_snapshot(to, &link.snapshot_path(from), &link.snapshot_hash)?;
        let mut copied = link.clone();
        copied.source = relative_to(&link.source, to)?;
        copied.snapshot = snapshot
            .file_name()
            .ok_or_else(|| invalid("snapshot filename"))?
            .into();
        Ok(Self {
            snapshot: Some(snapshot),
            metadata: serde_json::to_vec_pretty(&copied)?,
        })
    }
}

/// Prepare all source associations before replacing a tree. Publication uses an
/// explicit unavailable view entry until both metadata states are committed.
/// A crash/error leaves extraction disabled and a recovery record beside the tree.
pub(crate) struct PreparedLinks {
    target: PathBuf,
    origin: Option<StagedLink>,
    view: Option<StagedLink>,
    previous_origin: PreviousMetadata,
    previous_view: PreviousMetadata,
}

impl PreparedLinks {
    pub(crate) fn prepare(from: &Path, to: &Path) -> Result<Self> {
        let origin = metadata_present(&sidecar(from))?
            .then(|| AlignmentLink::read(from))
            .transpose()?;
        Self::with_origin(origin.as_ref(), from, to)
    }

    fn with_origin(origin: Option<&AlignmentLink>, from: &Path, to: &Path) -> Result<Self> {
        let view_path = view_tree(from);
        let view = metadata_present(&sidecar(&view_path))?
            .then(|| AlignmentLink::read(&view_path))
            .transpose()?;
        // Check every source before creating destination evidence or touching its tree.
        for link in origin.into_iter().chain(view.iter()) {
            link.verify_snapshot(from)?;
        }
        let previous_origin = previous_metadata(&sidecar(to))?;
        let previous_view = previous_metadata(&view_sidecar(to))?;
        Ok(Self {
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
        let recovery = serde_json::to_vec_pretty(&serde_json::to_value(AlignmentLinkRecovery {
            schema: 1,
            note: "Save As association recovery: previous entries and intended metadata; snapshots are retained separately".into(),
            previous_origin: self.previous_origin.clone(),
            previous_view: self.previous_view.clone(),
            intended_origin: self.origin.as_ref().map(|link| link.metadata.clone()),
            intended_view: self.view.as_ref().map(|link| link.metadata.clone()),
        })?)?;
        let recovery_path = retain_file(&self.target, ".alignment-recovery.json", |out| {
            out.write_all(&recovery)
        })?;
        let unavailable =
            serde_json::to_vec_pretty(&serde_json::to_value(AlignmentLinkUnavailable {
                schema: 1,
                unavailable: "Save As alignment publication incomplete".into(),
                recovery: recovery_path.file_name().map(std::ffi::OsStr::to_os_string),
            })?)?;
        // Install the guard before changing data or origin. If this fails, the old
        // tree and all its associations are untouched. Never remove a failed guard.
        basecraft_io::write_atomic(&view_sidecar(&self.target), |out| {
            out.write_all(&unavailable)
        })?;
        let finish = || -> Result<(T, Vec<PathBuf>)> {
            // The scientific origin must also become unavailable before replacing
            // data, so partial files cannot claim the overwritten tree's origin.
            basecraft_io::write_atomic(&sidecar(&self.target), |out| out.write_all(&unavailable))?;
            let result = save_data()?;
            let mut outputs = vec![recovery_path.clone()];
            for (metadata, staged) in [
                (sidecar(&self.target), &self.origin),
                (view_sidecar(&self.target), &self.view),
            ] {
                if let Some(link) = staged {
                    basecraft_io::write_atomic(&metadata, |out| out.write_all(&link.metadata))?;
                    if let Some(snapshot) = &link.snapshot {
                        outputs.push(snapshot.clone());
                    }
                    outputs.push(metadata);
                } else if metadata_present(&metadata)? {
                    // Intentional absence is part of Save As state. Prior entries
                    // are preserved in recovery, and their snapshots are untouched.
                    std::fs::remove_file(&metadata)?;
                }
            }
            Ok((result, outputs))
        };
        finish().map_err(|error| invalid(format!(
            "Save As incomplete; alignment association may be unavailable; recovery metadata at {}: {error}", recovery_path.display()
        )))
    }
}

/// Store portable references relative to the document's directory where possible.
fn relative_to(path: &Path, tree: &Path) -> Result<PathBuf> {
    let parent = tree.parent().unwrap_or(Path::new(".")).canonicalize()?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
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
    let file = std::sync::Arc::new(basecraft_io::cache::open_fasta(path)?);
    Ok(basecraft_docs::AlignmentDoc::from_fasta(
        "Linked input",
        &file,
    ))
}

fn names(path: &Path) -> Result<BTreeSet<String>> {
    let alignment = basecraft_io::cache::open_fasta(path)?;
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
    fn associate(tree_path: &Path, tree: &TreeDoc, source: &Path) -> Result<Vec<PathBuf>>;
    fn create_referenced(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        run: &crate::analysis_store::ArtifactRun,
    ) -> Result<Vec<PathBuf>>;
    fn create(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        inferred: bool,
        source_hash: Option<&str>,
    ) -> Result<Vec<PathBuf>>;
    fn read(tree: &Path) -> Result<Self>;
    fn read_view(tree: &Path) -> Result<Self>;
    fn status(&self, tree_path: &Path, tree: &TreeDoc) -> String;
    fn checked_snapshot(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf>;
    fn share_to(&self, from: &Path, to: &Path) -> Result<PathBuf>;
    fn copy_to(&self, from: &Path, to: &Path) -> Result<Vec<PathBuf>>;
    fn source_changed(&self) -> bool;
}

// Evidence helpers remain private to ops; protocol contains only wire data.
trait LinkEvidence {
    fn snapshot_path(&self, tree_path: &Path) -> PathBuf;
    fn evidence_run(&self, tree_path: &Path) -> Result<PathBuf>;
    fn verify_snapshot(&self, tree_path: &Path) -> Result<PathBuf>;
}

impl LinkEvidence for AlignmentLink {
    fn snapshot_path(&self, tree_path: &Path) -> PathBuf {
        tree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.snapshot)
    }

    fn evidence_run(&self, tree_path: &Path) -> Result<PathBuf> {
        let name = self
            .extra
            .get("analysis_run")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("reference association has no artifact run"))?;
        let run = tree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(name)
            .canonicalize()?;
        // Never trust an arbitrary run supplied by association metadata.
        if let Some(bound) = crate::analysis_store::ArtifactRun::for_result(tree_path)?
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
        crate::analysis_store::ArtifactRun::validate_store(store)?;
        Ok(run)
    }

    fn verify_snapshot(&self, tree_path: &Path) -> Result<PathBuf> {
        if self.schema == 2 {
            let run = self.evidence_run(tree_path)?;
            let input = crate::input_evidence::read_input(&run)?;
            if input.state == basecraft_protocol::input_records::EvidenceState::Broken {
                return Err(invalid("alignment evidence explicitly broken"));
            }
            let source = if let Some(reference) = &input.source {
                crate::input_evidence::verified_source(&run, reference)?
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
            if actual != self.snapshot_hash {
                return Err(invalid(
                    "linked input differs from actual submitted alignment",
                ));
            }
            // The snapshot API needs a typed FASTA path. Materialize a disposable
            // canonical cache only on explicit use, after verifying the original;
            // never treat this cache as repair for a missing/changed reference.
            if crate::files::Format::from_path(&source).ok() != Some(crate::files::Format::Fasta)
                || !raw
                    && input
                        .source
                        .as_ref()
                        .is_some_and(|r| r.hash != self.snapshot_hash)
            {
                let paths = basecraft_platform::paths::AppPaths::discover()
                    .map_err(|e| invalid(e.to_string()))?;
                let folder = paths.cache.join("analysis-inputs");
                std::fs::create_dir_all(&folder)?;
                let digest = self
                    .snapshot_hash
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
            return Ok(source);
        }
        let snapshot = self.snapshot_path(tree_path);
        if basecraft_io::hash_file(&snapshot)? != self.snapshot_hash {
            return Err(crate::Error::ChangedOnDisk { path: snapshot });
        }
        Ok(snapshot)
    }
}

impl AlignmentLinkExt for AlignmentLink {
    /// Associate an imported tree, keeping a standard FASTA snapshot beside it.
    fn associate(tree_path: &Path, tree: &TreeDoc, source: &Path) -> Result<Vec<PathBuf>> {
        validate_taxa(tree, source, false)?;
        let origin_inferred =
            metadata_present(&sidecar(tree_path))? && Self::read(tree_path)?.inferred;
        let target = if origin_inferred || metadata_present(&view_sidecar(tree_path))? {
            view_tree(tree_path)
        } else {
            tree_path.to_path_buf()
        };
        Self::create(&target, source, source, 0, false, None)
    }

    /// Reference-first inferred association. Clean disk input is never copied;
    /// dirty submitted input is already retained once by the job's artifact run.
    fn create_referenced(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        run: &crate::analysis_store::ArtifactRun,
    ) -> Result<Vec<PathBuf>> {
        names(input)?;
        let record = crate::input_evidence::read_input(run.directory())?;
        let link = Self {
            schema: 2,
            source: relative_to(&std::path::absolute(source)?, tree)?,
            source_hash: record
                .source
                .as_ref()
                .map_or_else(String::new, |r| r.hash.clone()),
            snapshot: PathBuf::new(),
            snapshot_hash: record.submitted_hash,
            alignment_version: version,
            inferred: true,
            extra: [(
                "analysis_run".into(),
                serde_json::json!(relative_to(run.directory(), tree)?),
            )]
            .into_iter()
            .collect(),
        };
        let metadata = hidden_sidecar(run.directory(), tree);
        crate::input_evidence::write_json(&metadata, &link)?;
        run.bind(tree)?;
        Ok(vec![metadata])
    }

    /// Called with the exact alignment supplied to a tree tool.
    fn create(
        tree: &Path,
        source: &Path,
        input: &Path,
        version: u64,
        inferred: bool,
        source_hash: Option<&str>,
    ) -> Result<Vec<PathBuf>> {
        names(input)?;
        let source = source.canonicalize()?;
        let source_hash = source_hash
            .map(str::to_owned)
            .map_or_else(|| basecraft_io::hash_file(&source), Ok)?;
        let snapshot_hash = basecraft_io::hash_file(input)?;
        let snapshot = retain_snapshot(tree, input, &snapshot_hash)?;
        let link = Self {
            schema: 1,
            source_hash,
            source: relative_to(&source, tree)?,
            snapshot: snapshot
                .file_name()
                .ok_or_else(|| invalid("snapshot filename"))?
                .into(),
            snapshot_hash,
            alignment_version: version,
            inferred,
            extra: BTreeMap::new(),
        };
        let metadata = sidecar(tree);
        let bytes = serde_json::to_vec_pretty(&link)?;
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes)).map_err(|error| invalid(format!(
            "alignment metadata replacement failed; previous snapshots are unchanged; staged snapshot retained at {}: {error}", snapshot.display()
        )))?;
        Ok(vec![snapshot, metadata])
    }

    fn read(tree: &Path) -> Result<Self> {
        crate::analysis_store::ArtifactRun::for_result(tree)?;
        let metadata: serde_json::Value =
            serde_json::from_reader(std::fs::File::open(sidecar(tree))?)?;
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
        let mut link: Self = serde_json::from_value(metadata)?;
        if !matches!(link.schema, 1 | 2) {
            return Err(crate::Error::Unsupported(
                "unsupported tree–alignment link version".into(),
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
    fn read_view(tree: &Path) -> Result<Self> {
        Self::read(&if metadata_present(&view_sidecar(tree))? {
            view_tree(tree)
        } else {
            tree.to_path_buf()
        })
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
            return match self
                .evidence_run(tree_path)
                .and_then(|run| crate::input_evidence::read_input(&run).map(|input| (run, input)))
            {
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
                        crate::input_evidence::stamp(&run.join(&r.path)).is_ok_and(|s| {
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
        if !self.source.exists() {
            status.push_str(" (source missing or moved)");
        } else if self.source_changed() {
            status.push_str(" (source changed)");
        }
        match self.checked_snapshot(tree_path, tree) {
            Ok(_) => status.push_str("; retained snapshot available"),
            Err(e) => {
                let _ = write!(status, "; retained snapshot unavailable: {e}");
            }
        }
        status
    }

    fn checked_snapshot(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf> {
        let snapshot = self.verify_snapshot(tree_path)?;
        validate_taxa(tree, &snapshot, self.inferred)?;
        Ok(snapshot)
    }

    /// Forest members share the same retained alignment instead of copying it per tree.
    fn share_to(&self, from: &Path, to: &Path) -> Result<PathBuf> {
        let mut link = self.clone();
        if self.schema == 2 {
            let run = self.evidence_run(from)?;
            link.extra.insert(
                "analysis_run".into(),
                serde_json::json!(relative_to(&run, to)?),
            );
            link.source = relative_to(&self.source, to)?;
            // Same-folder forest components share one run; no input copy.
            let metadata = hidden_sidecar(&run, to);
            crate::input_evidence::write_json(&metadata, &link)?;
            let original_binding = crate::analysis_store::ArtifactRun::for_result(from)?
                .ok_or_else(|| invalid("missing run binding"))?;
            let id = original_binding
                .file_name()
                .ok_or_else(|| invalid("run identity missing"))?
                .to_string_lossy();
            crate::analysis_store::bind_existing(&run, &id, to)?;
            return Ok(metadata);
        }
        link.snapshot = relative_to(
            &from
                .parent()
                .unwrap_or(Path::new("."))
                .join(&self.snapshot)
                .canonicalize()?,
            to,
        )?;
        link.source = relative_to(&self.source, to)?;
        let metadata = sidecar(to);
        let bytes = serde_json::to_vec_pretty(&link)?;
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes))?;
        Ok(metadata)
    }

    /// Preserve a link when a tree is saved under a new name or directory.
    fn copy_to(&self, from: &Path, to: &Path) -> Result<Vec<PathBuf>> {
        PreparedLinks::with_origin(Some(self), from, to)?
            .publish(|| Ok(()))
            .map(|((), outputs)| outputs)
    }

    fn source_changed(&self) -> bool {
        if self.schema == 2 {
            return !self.source.exists();
        }
        basecraft_io::hash_file(&self.source).map_or(true, |h| h != self.source_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_save_as_carries_snapshot_and_replays() {
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
        assert!(
            link.checked_snapshot(&output, session.tree(doc).unwrap())
                .is_ok()
        );
        session.flush().unwrap();
        assert!(crate::replay::replay(dir.path()).unwrap().ok());
    }

    #[test]
    fn linkage_checks_labels_snapshot_and_changed_source() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("a.afa");
        let tree_path = d.path().join("a.nwk");
        std::fs::write(&source, ">a\nA-C\n>b\nATC\n").unwrap();
        let tree = TreeDoc::from_parsed(
            "tree",
            &basecraft_io::newick::parse_newick("(a,b);").unwrap(),
        );
        AlignmentLink::associate(&tree_path, &tree, &source).unwrap();
        let link = AlignmentLink::read(&tree_path).unwrap();
        assert!(!link.inferred);
        assert!(!link.source_changed());
        let snapshot = link.checked_snapshot(&tree_path, &tree).unwrap();
        std::fs::write(&source, ">a\nAAC\n>b\nATC\n").unwrap();
        assert!(link.source_changed());
        assert!(link.checked_snapshot(&tree_path, &tree).is_ok());
        std::fs::write(snapshot, ">a\nAAC\n>b\nATC\n").unwrap();
        assert!(link.checked_snapshot(&tree_path, &tree).is_err());
        std::fs::write(&source, ">a\nAAC\n>a\nATC\n").unwrap();
        assert!(validate_taxa(&tree, &source, false).is_err());
        std::fs::write(&source, ">a\nAAC\n>c\nATC\n").unwrap();
        assert!(validate_taxa(&tree, &source, false).is_err());
    }
}

#[cfg(test)]
#[path = "links/repair_record_tests.rs"]
mod repair_record_tests;
