//! Explicit tree–alignment provenance. Snapshots retain unsaved input exactly.
use crate::{Result, error::invalid};
use basecraft_docs::TreeDoc;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlignmentLink {
    pub schema: u32,
    pub source: PathBuf,
    pub source_hash: String,
    pub snapshot: PathBuf,
    pub snapshot_hash: String,
    pub alignment_version: u64,
    /// False for an explicitly associated imported tree: matching names is not inference provenance.
    pub inferred: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

pub fn sidecar(tree: &Path) -> PathBuf {
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

#[derive(Serialize)]
#[serde(tag = "entry", content = "value", rename_all = "snake_case")]
enum PreviousMetadata {
    Absent,
    /// Preserve bytes even when the old metadata cannot be decoded.
    File(Vec<u8>),
    /// Preserve the directory entry without following a possibly dangling link.
    Symlink(PathBuf),
}

impl PreviousMetadata {
    fn read(path: &Path) -> Result<Self> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Ok(Self::Symlink(std::fs::read_link(path)?))
            }
            Ok(metadata) if metadata.is_file() => Ok(Self::File(std::fs::read(path)?)),
            Ok(_) => Err(invalid(format!(
                "association metadata is not a file: {}",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(error) => Err(error.into()),
        }
    }
}

struct StagedLink {
    snapshot: PathBuf,
    metadata: Vec<u8>,
}

impl StagedLink {
    fn copy(link: &AlignmentLink, from: &Path, to: &Path) -> Result<Self> {
        let snapshot = retain_snapshot(to, &link.snapshot_path(from), &link.snapshot_hash)?;
        let mut copied = link.clone();
        copied.source = relative_to(&link.source, to)?;
        copied.snapshot = snapshot
            .file_name()
            .ok_or_else(|| invalid("snapshot filename"))?
            .into();
        Ok(Self {
            snapshot,
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
        Self::with_origin(origin, from, to)
    }

    fn with_origin(origin: Option<AlignmentLink>, from: &Path, to: &Path) -> Result<Self> {
        let view_path = view_tree(from);
        let view = metadata_present(&sidecar(&view_path))?
            .then(|| AlignmentLink::read(&view_path))
            .transpose()?;
        // Check every source before creating destination evidence or touching its tree.
        for link in origin.iter().chain(view.iter()) {
            link.verify_snapshot(from)?;
        }
        let previous_origin = PreviousMetadata::read(&sidecar(to))?;
        let previous_view = PreviousMetadata::read(&view_sidecar(to))?;
        Ok(Self {
            target: to.to_path_buf(),
            origin: origin
                .as_ref()
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
        let recovery = serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1,
            "note": "Save As association recovery: previous entries and intended metadata; snapshots are retained separately",
            "previous_origin": self.previous_origin,
            "previous_view": self.previous_view,
            "intended_origin": self.origin.as_ref().map(|link| &link.metadata),
            "intended_view": self.view.as_ref().map(|link| &link.metadata),
        }))?;
        let recovery_path = retain_file(&self.target, ".alignment-recovery.json", |out| {
            out.write_all(&recovery)
        })?;
        let unavailable = serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1,
            "unavailable": "Save As alignment publication incomplete",
            "recovery": recovery_path.file_name(),
        }))?;
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
                    outputs.extend([link.snapshot.clone(), metadata]);
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

impl AlignmentLink {
    /// Associate an imported tree, keeping a standard FASTA snapshot beside it.
    pub fn associate(tree_path: &Path, tree: &TreeDoc, source: &Path) -> Result<Vec<PathBuf>> {
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

    /// Called with the exact alignment supplied to a tree tool.
    pub fn create(
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

    pub fn read(tree: &Path) -> Result<Self> {
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
        if link.schema != 1 {
            return Err(invalid("unsupported tree–alignment link version"));
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
    pub fn read_view(tree: &Path) -> Result<Self> {
        Self::read(&if metadata_present(&view_sidecar(tree))? {
            view_tree(tree)
        } else {
            tree.to_path_buf()
        })
    }

    /// Status describes retained evidence and never reads changed source data as input.
    pub fn status(&self, tree_path: &Path, tree: &TreeDoc) -> String {
        let label = if self.inferred {
            "Source alignment"
        } else {
            "Linked alignment"
        };
        let mut status = format!("{label}: {}", self.source.display());
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

    fn snapshot_path(&self, tree_path: &Path) -> PathBuf {
        tree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.snapshot)
    }

    fn verify_snapshot(&self, tree_path: &Path) -> Result<PathBuf> {
        let snapshot = self.snapshot_path(tree_path);
        if basecraft_io::hash_file(&snapshot)? != self.snapshot_hash {
            return Err(invalid(
                "linked alignment snapshot has changed; link the alignment again",
            ));
        }
        Ok(snapshot)
    }

    pub fn checked_snapshot(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf> {
        let snapshot = self.verify_snapshot(tree_path)?;
        validate_taxa(tree, &snapshot, self.inferred)?;
        Ok(snapshot)
    }

    /// Forest members share the same retained alignment instead of copying it per tree.
    pub fn share_to(&self, from: &Path, to: &Path) -> Result<PathBuf> {
        let mut link = self.clone();
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
    pub fn copy_to(&self, from: &Path, to: &Path) -> Result<Vec<PathBuf>> {
        PreparedLinks::with_origin(Some(self.clone()), from, to)?
            .publish(|| Ok(()))
            .map(|(_, outputs)| outputs)
    }

    pub fn source_changed(&self) -> bool {
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
        AlignmentLink::associate(&tree_path, session.tree(doc), &source).unwrap();
        session.save(doc, Some(&output)).unwrap();
        let link = AlignmentLink::read(&output).unwrap();
        assert!(link.checked_snapshot(&output, session.tree(doc)).is_ok());
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
