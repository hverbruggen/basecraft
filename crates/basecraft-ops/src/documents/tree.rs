//! Tree payload behavior; Session retains identity, links, history and publication.
use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_docs::tree::TreeDoc;
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::{self, Format, HashingWriter, OpenedTree};
use crate::session::Inverse;

impl Document for OpenedTree {
    fn revision(&self, _generation: u64) -> u64 {
        self.doc.version()
    }

    fn fingerprint(&self) -> DocState {
        DocState {
            len: self.doc.tip_count() as u64,
            features: self.doc.annotations().len() as u64,
            rows: None,
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        execute_tree(&mut self.doc, op).map(|before| (Inverse::Tree(before), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Tree(before) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        self.doc = (**before).clone();
        Ok(())
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != Format::Tree {
            return Err(invalid("a tree can only be saved as a tree file"));
        }
        let mut hashes = Vec::new();
        let sidecar = files::tree_sidecar(target);
        if sidecar.is_file() {
            let bytes = std::fs::read(&sidecar)?;
            let metadata: serde_json::Value = serde_json::from_slice(&bytes)?;
            if metadata.get("schema_version").is_none() || metadata["schema_version"] == 0 {
                let backup = sidecar.with_file_name(format!(
                    "{}.unverified-{}",
                    sidecar.file_name().unwrap_or_default().to_string_lossy(),
                    basecraft_io::hash_bytes(&bytes).trim_start_matches("b3:")
                ));
                if backup.try_exists()? {
                    if std::fs::read(&backup)? != bytes {
                        return Err(invalid(
                            "legacy metadata backup differs; original was not replaced",
                        ));
                    }
                } else {
                    let mut staged = tempfile::NamedTempFile::new_in(
                        sidecar.parent().unwrap_or(Path::new(".")),
                    )?;
                    std::io::Write::write_all(&mut staged, &bytes)?;
                    staged.as_file().sync_all()?;
                    staged.persist_noclobber(&backup).map_err(|e| e.error)?;
                }
            }
        }
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
                invalid(format!("tree data saved; metadata incomplete: {error}. Keep the open document or temporary recovery and retry saving"))
            } else { error.into() })?;
            hashes.push((path.to_path_buf(), hash));
        }
        crate::analysis_store::ArtifactRun::refresh_binding(target).map_err(|error| invalid(format!("tree and metadata saved; binding publication incomplete: {error}. Keep the open document and retry saving")))?;
        Ok(hashes)
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

// Preserve the existing whole-tree snapshot inverse in this migration. Edit-sized
// inverses remain separate architecture debt; this extraction changes no mutation.
pub(crate) fn execute_tree(doc: &mut TreeDoc, op: &Op) -> Result<Box<TreeDoc>> {
    let before = Box::new(doc.clone());
    let edit = match op {
        Op::TreeReroot { clade } => doc.reroot(clade),
        Op::TreeMidpointRoot {} => doc.midpoint_root(),
        Op::TreeMinimumVarianceRoot {} => doc.minimum_variance_root(),
        Op::TreeRootPosition { clade, fraction } => doc.set_root_position(clade, *fraction),
        Op::TreeRotate { clade } => doc.rotate(clade),
        Op::TreeLadderize { small_first } => {
            doc.ladderize(*small_first);
            Ok(())
        }
        Op::TreeRenameTip { from, to } => doc.rename_tip(from, to),
        Op::TreeAnnotate { id, annotation } => doc.annotate(*id, annotation.clone()),
        Op::TreeAddSupport {
            name,
            source,
            values,
            kind,
        } => doc
            .add_support(name, source, values)
            .map(|()| doc.set_support_kind(name, *kind)),
        Op::TreeRemoveSupport { name } => doc.remove_support(name),
        Op::TreeStyle {
            style,
            remove_supports,
        } => {
            if style
                .support_decimals
                .values()
                .any(|&decimals| decimals > 10)
            {
                return Err(invalid("support decimal places must be between 0 and 10"));
            }
            let mut candidate = doc.clone();
            candidate.set_style(style.clone());
            remove_supports
                .iter()
                .try_for_each(|name| candidate.remove_support(name))
                .map(|()| *doc = candidate)
        }
        _ => {
            return Err(invalid(format!(
                "{} needs a sequence or an alignment",
                op.name()
            )));
        }
    };
    edit.map_err(|e| invalid(e.0))?;
    Ok(before)
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
