//! Explicit tree–alignment provenance. Snapshots retain unsaved input exactly.
use crate::{Result, error::invalid};
use basecraft_docs::TreeDoc;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
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
        let target = if sidecar(tree_path).exists() && Self::read(tree_path)?.inferred {
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
        let mut filename = tree.as_os_str().to_os_string();
        filename.push(".alignment.afa");
        let snapshot = PathBuf::from(filename);
        let source = source.canonicalize()?;
        if snapshot.exists() && snapshot.canonicalize()? == source {
            return Err(invalid(
                "alignment snapshot output would overwrite its source",
            ));
        }
        let source_hash = source_hash
            .map(str::to_owned)
            .map_or_else(|| basecraft_io::hash_file(&source), Ok)?;
        let snapshot_hash = basecraft_io::hash_file(input)?;
        basecraft_io::write_atomic(&snapshot, |out| {
            std::io::copy(&mut std::fs::File::open(input)?, out)?;
            Ok(())
        })?;
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
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes))?;
        Ok(vec![snapshot, metadata])
    }

    pub fn read(tree: &Path) -> Result<Self> {
        let mut link: Self = serde_json::from_reader(std::fs::File::open(sidecar(tree))?)?;
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
        Self::read(&if view_sidecar(tree).exists() {
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
            Err(e) => status.push_str(&format!("; retained snapshot unavailable: {e}")),
        }
        status
    }

    pub fn checked_snapshot(&self, tree_path: &Path, tree: &TreeDoc) -> Result<PathBuf> {
        let snapshot = tree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.snapshot);
        if basecraft_io::hash_file(&snapshot)? != self.snapshot_hash {
            return Err(invalid(
                "linked alignment snapshot has changed; link the alignment again",
            ));
        }
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
        let old_snapshot = from.parent().unwrap_or(Path::new(".")).join(&self.snapshot);
        if basecraft_io::hash_file(&old_snapshot)? != self.snapshot_hash {
            return Err(invalid("linked alignment snapshot has changed"));
        }
        let mut filename = to.as_os_str().to_os_string();
        filename.push(".alignment.afa");
        let snapshot = PathBuf::from(filename);
        if snapshot.exists() && snapshot.canonicalize()? == self.source {
            return Err(invalid(
                "alignment snapshot output would overwrite its source",
            ));
        }
        basecraft_io::write_atomic(&snapshot, |out| {
            std::io::copy(&mut std::fs::File::open(&old_snapshot)?, out)?;
            Ok(())
        })?;
        let mut link = self.clone();
        link.snapshot = snapshot
            .file_name()
            .ok_or_else(|| invalid("snapshot filename"))?
            .into();
        link.source = relative_to(&self.source, to)?;
        let metadata = sidecar(to);
        let bytes = serde_json::to_vec_pretty(&link)?;
        basecraft_io::write_atomic(&metadata, |out| out.write_all(&bytes))?;
        let mut outputs = vec![snapshot, metadata];
        if view_sidecar(from).exists() {
            let view = Self::read(&view_tree(from))?;
            outputs.extend(view.copy_to(&view_tree(from), &view_tree(to))?);
        }
        Ok(outputs)
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
