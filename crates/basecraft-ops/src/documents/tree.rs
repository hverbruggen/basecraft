//! Tree payload behavior; Session retains identity, links, history and publication.
use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_docs::tree::TreeDoc;
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
                | Op::TreeAnnotate { .. }
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
    ) -> Result<Option<crate::links::PreparedLinks>> {
        if format != Format::Tree {
            return Err(Error::Unsupported(
                "a tree can only be saved as a tree file".into(),
            ));
        }
        if crate::links::document_entry(original)? == crate::links::document_entry(target)? {
            Ok(None)
        } else {
            crate::links::PreparedLinks::prepare(original, target).map(Some)
        }
    }

    fn saved_companion(&self, target: &Path) -> Option<PathBuf> {
        Some(files::tree_sidecar(target))
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != Format::Tree {
            return Err(Error::Unsupported(
                "a tree can only be saved as a tree file".into(),
            ));
        }
        let mut hashes = Vec::new();
        let sidecar = files::tree_sidecar(target);
        for (path, sidecar_file) in [(target, false), (sidecar.as_path(), true)] {
            let mut hash = String::new();
            basecraft_io::write_atomic(path, |w| {
                let mut hw = HashingWriter::new(w);
                if sidecar_file {
                    files::write_tree_sidecar_at(
                        &self.doc,
                        &basecraft_io::hash_file(target).map_err(std::io::Error::other)?,
                        &mut hw,
                    )?;
                } else {
                    files::write_tree(self, &mut hw)?;
                }
                hash = hw.hash();
                Ok(())
            }).map_err(|error| if sidecar_file {
                Error::Damaged { path: target.into(), detail: format!("tree data saved; metadata incomplete: {error}. Keep the open document or temporary recovery and retry saving") }
            } else { error.into() })?;
            hashes.push((path.to_path_buf(), hash));
        }
        crate::analysis_store::ArtifactRun::refresh_binding(target).map_err(|error| Error::Damaged { path: target.into(), detail: format!("tree and metadata saved; binding publication incomplete: {error}. Keep the open document and retry saving") })?;
        Ok(hashes)
    }

    fn saved_hashes_are_content_only(&self) -> bool {
        true
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        let mut hw = HashingWriter::new(std::io::sink());
        files::write_tree(self, &mut hw)?;
        let mut side = HashingWriter::new(std::io::sink());
        files::write_tree_sidecar_at(&self.doc, &hw.hash(), &mut side)?;
        Ok(vec![hw.hash(), side.hash()])
    }
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
