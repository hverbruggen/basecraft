//! Tree payload behavior; Session retains identity, links, history and publication.
use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_docs::tree::TreeDoc;
use basecraft_protocol::tree::{DisplacedLabel, LabelPlacement};
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Error;
use crate::Result;
use crate::files::{self, Format, HashingWriter, OpenedTree};
use crate::session::Inverse;

impl Document for OpenedTree {
    fn accepts(&self, op: &Op) -> bool {
        matches!(
            op,
            Op::TreeReroot { .. }
                | Op::TreeMidpointRoot {}
                | Op::TreeMinimumVarianceRoot {}
                | Op::TreeRootPosition { .. }
                | Op::TreeRotate { .. }
                | Op::TreeLadderize { .. }
                | Op::TreeRenameTip { .. }
                | Op::TreeSetBranchLengths { .. }
                | Op::TreeAnnotate { .. }
                | Op::TreePlaceLabel { .. }
                | Op::TreeAddSupport { .. }
                | Op::TreeRemoveSupport { .. }
                | Op::TreeStyle { .. }
        )
    }

    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Tree
    }

    fn retains_companions(&self) -> bool {
        true
    }
    fn opening_warning(&self) -> Option<&str> {
        self.metadata_warning.as_deref()
    }

    fn revision(&self, _generation: u64) -> u64 {
        self.doc.version()
    }

    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Tree {
            tips: self.doc.tip_count() as u64,
            annotations: self.doc.annotations().len() as u64,
        })
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        execute_tree(&mut self.doc, op).map(|before| (Inverse::Tree(before), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Tree(before) = inverse else {
            return Err(Error::Unsupported("undo of the wrong kind of edit".into()));
        };
        self.doc.undo_edit(before);
        Ok(())
    }

    fn prepare_links(
        &self,
        original: &Path,
        target: &Path,
        format: Format,
    ) -> Result<Option<crate::records::links::PreparedLinks>> {
        if format != Format::Tree {
            return Err(Error::Unsupported(
                "a tree can only be saved as a tree file".into(),
            ));
        }
        if crate::records::links::document_entry(original)?
            == crate::records::links::document_entry(target)?
        {
            Ok(None)
        } else {
            crate::records::links::PreparedLinks::prepare(original, target).map(Some)
        }
    }

    fn saved_companions(&self, target: &Path) -> Vec<PathBuf> {
        vec![files::tree_sidecar(target)]
    }

    fn prepare_write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<super::PreparedWrite> {
        if format != Format::Tree {
            return Err(Error::Unsupported(
                "a tree can only be saved as a tree file".into(),
            ));
        }
        let mut prepared = super::PreparedWrite::default();
        let sidecar = files::tree_sidecar(target);
        let hash = prepared.push(target, |w| files::write_tree(self, w))?;
        prepared.push(&sidecar, |w| {
            files::tree_companion(&self.doc, &hash).write(w)
        })?;
        Ok(prepared)
    }

    fn finish_write(
        &self,
        target: &Path,
        _: &[(PathBuf, String)],
        _: Option<&crate::records::analysis_store::ArtifactRun>,
        _: &super::WritePublication,
    ) -> Result<()> {
        crate::records::analysis_store::ArtifactRun::refresh_binding(target)
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        let mut hw = HashingWriter::new(std::io::sink());
        files::write_tree(self, &mut hw)?;
        let mut side = HashingWriter::new(std::io::sink());
        files::tree_companion(&self.doc, &hw.hash()).write(&mut side)?;
        Ok(vec![hw.hash(), side.hash()])
    }
}

/// Labels the tree in `content` marks on a group other than the one they
/// were made for.
pub(crate) fn displaced(content: &files::Loaded) -> Vec<u32> {
    content
        .tree()
        .map_or_else(Vec::new, |tree| tree.doc.displaced_annotations())
}

/// The labels an edit, such as a reroot, displaced: displaced now and not
/// `before`.
pub(crate) fn newly_displaced(content: &files::Loaded, before: &[u32]) -> Vec<DisplacedLabel> {
    let Some(tree) = content.tree() else {
        return Vec::new();
    };
    let mut labels = tree.doc.displaced_labels();
    labels.retain(|label| !before.contains(&label.id));
    labels
}

/// History notes for each label `tree.place_label` places, taken before it
/// runs so a removed label keeps its name in History.
pub(crate) fn placement_notes(content: &files::Loaded, op: &Op) -> Vec<String> {
    let (Some(tree), Op::TreePlaceLabel { labels, choice }) = (content.tree(), op) else {
        return Vec::new();
    };
    let ids = if labels.is_empty() {
        displaced(content)
    } else {
        labels.clone()
    };
    let done = match choice {
        LabelPlacement::Exact => "moved to the clade holding exactly its tips",
        LabelPlacement::Smallest => "moved to the smallest clade holding its tips",
        LabelPlacement::Remove => "removed",
    };
    ids.iter()
        .filter_map(|id| tree.doc.annotations().get(id))
        .map(|a| format!("Label {:?} {done}", a.label))
        .collect()
}

/// The note for a label an edit displaced.
pub(crate) fn displaced_note(label: &DisplacedLabel) -> String {
    format!(
        "Label {:?} no longer marks the group it was made for",
        label.label
    )
}

pub(crate) fn execute_tree(doc: &mut TreeDoc, op: &Op) -> Result<basecraft_docs::tree::TreeUndo> {
    doc.apply_edit(op)
        .map_err(|error| Error::Unsupported(error.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_interface_rejects_wrong_operations_inverses_and_formats_without_changes() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("tree.nwk");
        std::fs::write(&source, "((a:1,b:1):1,c:2);\n(d:1,e:1);\n").unwrap();
        let mut loaded = files::load(&source).unwrap();
        let document = loaded.as_document_mut();
        let before = document
            .saved_hashes(&source, &super::super::WritePublication::Immediate)
            .unwrap();
        let revision = document.revision(0);
        for op in [
            Op::AlnRemoveGapColumns {},
            Op::TreeRenameTip {
                from: "missing".into(),
                to: "other".into(),
            },
            Op::TreeRenameTip {
                from: "a".into(),
                to: "b".into(),
            },
        ] {
            assert!(document.apply(&op).is_err());
            assert_eq!(
                document
                    .saved_hashes(&source, &super::super::WritePublication::Immediate)
                    .unwrap(),
                before
            );
            assert_eq!(document.revision(0), revision);
        }
        assert!(document.revert(&Inverse::Alignment(Vec::new())).is_err());
        let target = root.path().join("wrong.afa");
        assert!(
            document
                .write(
                    &target,
                    Format::Fasta,
                    root.path(),
                    &super::super::WritePublication::Immediate
                )
                .is_err()
        );
        assert!(!target.exists());
        assert!(!files::tree_sidecar(&target).exists());
        assert_eq!(
            document
                .saved_hashes(&source, &super::super::WritePublication::Immediate)
                .unwrap(),
            before
        );
        assert_eq!(document.revision(0), revision);
    }

    #[test]
    fn tree_interface_returns_target_paths_and_exact_writer_hashes_for_forests() {
        for (name, bytes) in [
            ("forest.nwk", "((a:1,b:1):1,c:2);\n(d:1,e:1);\n"),
            (
                "forest.nex",
                "#NEXUS\nBEGIN TREES;\nTREE first = ((a:1,b:1):1,c:2);\nTREE second = (d:1,e:1);\nEND;\n",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join(name);
            std::fs::write(&source, bytes).unwrap();
            let mut loaded = files::load(&source).unwrap();
            let document = loaded.as_document_mut();
            let initial = document
                .saved_hashes(&source, &super::super::WritePublication::Immediate)
                .unwrap();
            let revision = document.revision(0);
            let (inverse, feature) = document
                .apply(&Op::TreeRenameTip {
                    from: "a".into(),
                    to: "renamed".into(),
                })
                .unwrap();
            assert!(feature.is_none());
            let edited = document
                .saved_hashes(&source, &super::super::WritePublication::Immediate)
                .unwrap();
            assert_ne!(edited, initial);
            document.revert(&inverse).unwrap();
            assert_eq!(document.revision(0), revision);
            assert_eq!(
                document
                    .saved_hashes(&source, &super::super::WritePublication::Immediate)
                    .unwrap(),
                initial
            );
            document
                .apply(&Op::TreeRenameTip {
                    from: "a".into(),
                    to: "renamed".into(),
                })
                .unwrap();
            let target = root.path().join(format!("copy-{name}"));
            assert_eq!(
                document
                    .saved_hashes(&target, &super::super::WritePublication::Immediate)
                    .unwrap(),
                edited
            );
            let written = document
                .write(
                    &target,
                    Format::Tree,
                    root.path(),
                    &super::super::WritePublication::Immediate,
                )
                .unwrap();
            assert_eq!(
                written.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
                vec![target.clone(), files::tree_sidecar(&target)]
            );
            assert_eq!(
                written.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>(),
                edited
            );
            for (path, hash) in written {
                assert_eq!(basecraft_io::hash_file(&path).unwrap(), hash);
            }
            let reopened = files::load(&target).unwrap();
            assert_eq!(
                reopened
                    .as_document()
                    .saved_hashes(&target, &super::super::WritePublication::Immediate)
                    .unwrap(),
                edited
            );
        }
    }
}
