use std::path::{Path, PathBuf};

use basecraft_docs::{FeatureId, SequenceDoc};
use basecraft_protocol::{DocState, Feature, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::{self, Format, HashingWriter, Opened};
use crate::session::{Inverse, normalise};

impl Document for Opened {
    fn save_refusal(&self, _own_target: bool) -> Option<String> {
        self.others
            .gff
            .as_ref()
            .is_some_and(|kept| kept.embeds_sequence())
            .then(|| {
                "The GFF3 companion holds its own copy of this sequence after ##FASTA; \
                 saving would leave that copy unchanged. Remove the ##FASTA section to save."
                    .into()
            })
    }
    fn accepts(&self, op: &Op) -> bool {
        matches!(
            op,
            Op::SeqReplace { .. }
                | Op::SetAlphabet { .. }
                | Op::FeatureAdd { .. }
                | Op::FeatureRemove { .. }
                | Op::FeatureUpdate { .. }
        )
    }

    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Sequence
    }

    fn retains_companions(&self) -> bool {
        self.format == Format::Fasta
    }
    fn proposal_candidate(&self) -> Result<files::Loaded> {
        Ok(files::Loaded::Sequence(Opened {
            alphabet: self.alphabet.clone(),
            companion_warning: None,
            doc: self.doc.clone(),
            format: self.format,
            fasta: self.fasta.clone(),
            others: self.others.clone(),
        }))
    }
    fn apply_proposal(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        if !matches!(
            op,
            Op::SeqReplace { .. }
                | Op::FeatureAdd { .. }
                | Op::FeatureRemove { .. }
                | Op::FeatureUpdate { .. }
        ) {
            return Err(invalid(
                "proposal contains a prohibited host/lifecycle operation",
            ));
        }
        self.apply(op)
    }
    fn revision(&self, _generation: u64) -> u64 {
        self.doc.version()
    }
    fn fingerprint(&self) -> DocState {
        sequence_state(&self.doc, self.others.count)
    }
    fn opening_warning(&self) -> Option<&str> {
        self.companion_warning.as_deref()
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        if let Op::SetAlphabet { alphabet } = op {
            return Ok((super::set_alphabet(&mut self.alphabet, *alphabet), None));
        }
        execute_sequence(&mut self.doc, op)
    }
    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        if let Inverse::Alphabet(before) = inverse {
            self.alphabet.clone_from(before);
            return Ok(());
        }
        undo_sequence(&mut self.doc, inverse)
    }
    fn reapply(&mut self, op: &Op, inverse: &Inverse) -> Result<Inverse> {
        if let (Op::FeatureAdd { feature }, Inverse::Feature { id, .. }) = (op, inverse) {
            self.doc.restore_feature(*id, Some(feature.clone()));
            Ok(Inverse::Feature {
                id: *id,
                before: None,
            })
        } else {
            self.apply(op).map(|(inverse, _)| inverse)
        }
    }
    /// The GFF3 file when the sequence writes one, then the facts companion
    /// when it states an alphabet: the order of `saved_hashes`.
    fn saved_companions(&self, target: &Path) -> Vec<PathBuf> {
        let gff = files::write_sidecar(self, &mut std::io::sink()).is_some();
        let gff = gff.then(|| files::gff_sidecar(target));
        let facts = self.alphabet.as_ref().map(|_| files::tree_sidecar(target));
        gff.into_iter().chain(facts).collect()
    }

    fn prepare_write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<super::PreparedWrite> {
        if format != self.format {
            return Err(invalid("saving to a different format isn't supported yet"));
        }
        let mut prepared = super::PreparedWrite::default();
        let hash = prepared.push(target, |w| files::write_main(self, w))?;
        let sidecar = files::gff_sidecar(target);
        if format == Format::Fasta {
            prepared.push(&sidecar, |w| {
                if let Some(r) = files::write_sidecar(self, w) {
                    r?;
                }
                Ok(())
            })?;
        }
        super::stage_facts(&mut prepared, target, self.alphabet.as_ref(), &hash)?;
        Ok(prepared)
    }
    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        let mut hw = HashingWriter::new(std::io::sink());
        files::write_main(self, &mut hw)?;
        let main = hw.hash();
        let mut out = vec![main.clone()];
        let mut hw = HashingWriter::new(std::io::sink());
        if let Some(r) = files::write_sidecar(self, &mut hw) {
            r?;
            out.push(hw.hash());
        }
        out.extend(super::facts_hash(self.alphabet.as_ref(), &main)?);
        Ok(out)
    }
    fn merge(&self, previous: &Op, next: &Op) -> Option<Op> {
        match (previous, next) {
            (
                Op::SeqReplace {
                    start,
                    end,
                    residues,
                },
                Op::SeqReplace {
                    start: next_start,
                    end: next_end,
                    residues: next_residues,
                },
            ) if next_start == next_end
                && !next_residues.is_empty()
                && *next_start == start + residues.len() as u64 =>
            {
                Some(Op::SeqReplace {
                    start: *start,
                    end: *end,
                    residues: format!("{residues}{}", next_residues.to_ascii_uppercase()),
                })
            }
            _ => None,
        }
    }
}

pub(crate) fn sequence_state(doc: &SequenceDoc, records: usize) -> DocState {
    DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Sequence {
        residues: doc.len(),
        features: doc.feature_count() as u64,
        records: (records > 1).then_some(records as u64),
    })
}

/// Carry out a sequence edit, retaining an edit-sized inverse.
pub(crate) fn execute_sequence(
    doc: &mut SequenceDoc,
    op: &Op,
) -> Result<(Inverse, Option<FeatureId>)> {
    match op {
        Op::SeqReplace {
            start,
            end,
            residues,
        } => {
            if start > end || *end > doc.len() {
                return Err(invalid(format!(
                    "range {start}..{end} outside 0..{}",
                    doc.len()
                )));
            }
            let residues = normalise(residues)?;
            let result = doc.replace(*start, *end, residues.as_bytes());
            Ok((
                Inverse::Residues {
                    inserted: residues.len() as u64,
                    result,
                },
                None,
            ))
        }
        Op::FeatureAdd { feature } => {
            check_location(feature, doc.len())?;
            let fid = doc.add_feature(feature.clone());
            Ok((
                Inverse::Feature {
                    id: fid,
                    before: None,
                },
                Some(fid),
            ))
        }
        Op::FeatureRemove { id } => {
            let fid = FeatureId(*id);
            let before = doc
                .remove_feature(fid)
                .ok_or_else(|| invalid(format!("no feature {id}")))?;
            Ok((
                Inverse::Feature {
                    id: fid,
                    before: Some(before),
                },
                None,
            ))
        }
        Op::FeatureUpdate { id, feature } => {
            check_location(feature, doc.len())?;
            let fid = FeatureId(*id);
            if doc.feature(fid).is_none() {
                return Err(invalid(format!("no feature {id}")));
            }
            let before = doc.update_feature(fid, feature.clone());
            Ok((Inverse::Feature { id: fid, before }, None))
        }
        Op::AlnReplace { .. }
        | Op::AlnInsert { .. }
        | Op::AlnFill { .. }
        | Op::AlnInsertGaps { .. }
        | Op::AlnDelete { .. }
        | Op::AlnShift { .. }
        | Op::AlnRemoveGapColumns {}
        | Op::AlnMoveRows { .. }
        | Op::AlnRenameRow { .. }
        | Op::AlnAddSequences { .. }
        | Op::AlnRemoveRows { .. } => Err(invalid(format!("{} needs an alignment", op.name()))),
        Op::TreeReroot { .. }
        | Op::TreeMidpointRoot {}
        | Op::TreeMinimumVarianceRoot {}
        | Op::TreeRootPosition { .. }
        | Op::TreeRotate { .. }
        | Op::TreeLadderize { .. }
        | Op::TreeRenameTip { .. }
        | Op::TreeSetBranchLengths { .. }
        | Op::TreeAnnotate { .. }
        | Op::TreeAddSupport { .. }
        | Op::TreeRemoveSupport { .. }
        | Op::TreeStyle { .. } => Err(invalid(format!("{} needs a tree", op.name()))),
        Op::DelimitationEdit { .. } => Err(invalid("this needs a delimitation workspace")),
        // Handled by the opened sequence, which keeps the alphabet.
        Op::SetAlphabet { .. } => Err(invalid("set the alphabet on the open document")),
        Op::MetadataEdit { .. } => Err(invalid("this needs the project metadata")),
        Op::SangerReview { .. } | Op::SangerSelect { .. } | Op::SangerUpdate { .. } => Err(
            crate::Error::Unsupported("choose a Sanger review document".into()),
        ),
        Op::SubmissionEdit { .. } => Err(invalid("this needs a submission document")),
        Op::JobFinished { .. } => Err(invalid("use log_job_finished")),
        Op::Open { .. }
        | Op::CreateSangerReview { .. }
        | Op::CreateSequence { .. }
        | Op::Save { .. }
        | Op::Extract { .. }
        | Op::Export { .. }
        | Op::ToolRun { .. }
        | Op::Undo { .. }
        | Op::Redo { .. } => Err(invalid(format!("{} is not an edit", op.name()))),
    }
}

fn undo_sequence(doc: &mut SequenceDoc, inverse: &Inverse) -> Result<()> {
    match inverse {
        Inverse::Residues { inserted, result } => {
            let pos = result.change.pos;
            doc.replace(pos, pos + inserted, &result.removed);
            for (fid, f) in &result.before {
                doc.restore_feature(*fid, Some(f.clone()));
            }
        }
        Inverse::Feature { id: fid, before } => doc.restore_feature(*fid, before.clone()),
        Inverse::Alignment(_)
        | Inverse::Alphabet(_)
        | Inverse::Tree(_)
        | Inverse::Delimitation(_)
        | Inverse::Metadata(_)
        | Inverse::SangerSelect(_)
        | Inverse::SangerUpdate(_)
        | Inverse::SangerReview { .. }
        | Inverse::Submission(_) => return Err(invalid("undo of the wrong kind of edit")),
    }
    Ok(())
}

fn check_location(f: &Feature, len: u64) -> Result<()> {
    if f.location.segments.is_empty() {
        return Err(invalid("feature has no segments"));
    }
    for s in &f.location.segments {
        if s.start > s.end || s.end > len {
            return Err(invalid(format!(
                "segment {}..{} outside 0..{len}",
                s.start, s.end
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_interface_rejects_wrong_ops_inverses_and_formats_without_mutation() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("sequence.fasta");
        std::fs::write(&source, b">sequence\nACGT\n").unwrap();
        let mut loaded = files::load(&source).unwrap();
        let document = loaded.as_document_mut();
        let initial = document
            .saved_hashes(&source, &super::super::WritePublication::Immediate)
            .unwrap();
        let revision = document.revision(99);
        for op in [
            Op::AlnInsertGaps {
                rows: vec![(0, 1)],
                col: 0,
                count: 1,
            },
            Op::SeqReplace {
                start: 0,
                end: 5,
                residues: "A".into(),
            },
            Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "?".into(),
            },
            Op::FeatureRemove { id: 900 },
        ] {
            assert!(document.apply(&op).is_err());
            assert_eq!(
                document
                    .saved_hashes(&source, &super::super::WritePublication::Immediate)
                    .unwrap(),
                initial
            );
            assert_eq!(document.revision(99), revision);
        }
        assert!(document.revert(&Inverse::Alignment(Vec::new())).is_err());
        let target = root.path().join("wrong.gb");
        assert!(
            document
                .write(
                    &target,
                    Format::GenBank,
                    root.path(),
                    &super::super::WritePublication::Immediate
                )
                .is_err()
        );
        assert!(!target.exists());
        assert_eq!(
            document
                .saved_hashes(&source, &super::super::WritePublication::Immediate)
                .unwrap(),
            initial
        );
        assert_eq!(document.revision(99), revision);
    }

    #[test]
    fn sequence_interface_preserves_writer_bytes_features_topology_and_fasta_records() {
        crate::isolated_profile::isolate!();
        for (name, bytes) in [
            ("sequence.fasta", b">first\nACGTACGT\n".to_vec()),
            (
                "sequence.gb",
                include_str!(
                    "../../../../testdata/fixtures/compact/sequences-annotations/locus.gb"
                )
                .replace("linear", "circular")
                .into_bytes(),
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join(name);
            std::fs::write(&source, bytes).unwrap();
            let opened = files::open(&source).unwrap();
            let format = opened.format;
            let mut expected_main = Vec::new();
            files::write_main(&opened, &mut expected_main).unwrap();
            let mut expected_side = Vec::new();
            let side = files::write_sidecar(&opened, &mut expected_side).map(|r| r.unwrap());
            let mut loaded = files::Loaded::Sequence(opened);
            let document = loaded.as_document_mut();
            let initial = document
                .saved_hashes(&source, &super::super::WritePublication::Immediate)
                .unwrap();
            assert_eq!(initial[0], basecraft_io::hash_bytes(&expected_main));
            let (inverse, _) = document
                .apply(&Op::SeqReplace {
                    start: 2,
                    end: 4,
                    residues: "n".into(),
                })
                .unwrap();
            assert_ne!(
                document
                    .saved_hashes(&source, &super::super::WritePublication::Immediate)
                    .unwrap(),
                initial
            );
            document.revert(&inverse).unwrap();
            assert_eq!(
                document
                    .saved_hashes(&source, &super::super::WritePublication::Immediate)
                    .unwrap(),
                initial
            );
            let target = root.path().join(format!("copy-{name}"));
            let written = document
                .write(
                    &target,
                    format,
                    root.path(),
                    &super::super::WritePublication::Immediate,
                )
                .unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), expected_main);
            assert_eq!(written.len(), if side.is_some() { 2 } else { 1 });
            if side.is_some() {
                assert_eq!(written[1].0, files::gff_sidecar(&target));
                assert_eq!(std::fs::read(&written[1].0).unwrap(), expected_side);
            }
            for ((path, hash), expected) in written.iter().zip(&initial) {
                assert_eq!(hash, expected);
                assert_eq!(&basecraft_io::hash_file(path).unwrap(), hash);
            }
            let reopened = files::load(&target).unwrap();
            assert_eq!(
                reopened
                    .as_document()
                    .saved_hashes(&target, &super::super::WritePublication::Immediate)
                    .unwrap(),
                initial
            );
        }
    }

    #[test]
    fn sequence_typing_burst_keeps_feature_undo_and_journals_each_keystroke() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("sequence.gb");
        std::fs::write(
            &source,
            include_bytes!("../../../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
        )
        .unwrap();
        let mut session = crate::Session::new(root.path());
        let id = session.open(&source).unwrap();
        let initial = session.save_hashes(id).unwrap();
        let first = session
            .apply(
                id,
                Op::SeqReplace {
                    start: 60,
                    end: 60,
                    residues: "a".into(),
                },
            )
            .unwrap();
        let second = session
            .apply(
                id,
                Op::SeqReplace {
                    start: 61,
                    end: 61,
                    residues: "tg".into(),
                },
            )
            .unwrap();
        assert_ne!(first.entry_id, second.entry_id);
        let edited = session.save_hashes(id).unwrap();
        session.undo(id).unwrap();
        assert_eq!(session.save_hashes(id).unwrap(), initial);
        assert!(!session.can_undo(id).unwrap());
        session.redo(id).unwrap();
        assert_eq!(session.save_hashes(id).unwrap(), edited);
        session.save(id, None).unwrap();
        drop(session);
        let entries = crate::log::read_all(root.path()).unwrap();
        let replacements: Vec<_> = entries
            .iter()
            .filter_map(|entry| match &entry.op {
                Op::SeqReplace { residues, .. } => Some(residues.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(replacements.len(), 2);
        let report = crate::records::replay::replay(root.path()).unwrap();
        assert!(report.ok(), "{:?}", report.problems);
    }
}
