//! One staged proposal/commit owner. Kind adapters supply only candidate edits
//! and fingerprints; Session retains pins, actors, journal and undo ownership.
use super::{Applied, Inverse, Session, Slot, execute_sequence};
use crate::{
    DocId, Result,
    error::invalid,
    files::{Loaded, Opened},
};
use basecraft_docs::{FeatureId, SequenceDoc};
use basecraft_protocol::{Actor, DocState, LogEntry, Op, OpResult};

type Execute = fn(&mut Loaded, &Op) -> Result<(Inverse, Option<FeatureId>)>;
type Fingerprint = fn(&Loaded) -> Result<DocState>;

struct PreparedEdit {
    op: Op,
    inverse: Inverse,
    entry_id: String,
}

struct PreparedBatch {
    doc: DocId,
    generation: u64,
    revision: u64,
    candidate: Loaded,
    edits: Vec<PreparedEdit>,
    entries: Vec<LogEntry>,
}

pub(super) fn sequence_state(doc: &SequenceDoc) -> DocState {
    DocState {
        len: doc.len(),
        features: doc.feature_count() as u64,
        rows: None,
    }
}

pub(super) fn delimitation_state(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> DocState {
    DocState {
        len: work.specimens.len() as u64,
        features: work.hypotheses.len() as u64,
        rows: None,
    }
}

pub(super) fn execute_delimitation(
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
    op: &Op,
) -> Result<(Inverse, Option<FeatureId>)> {
    let Op::DelimitationEdit { edit } = op else {
        return Err(invalid("this needs a delimitation edit"));
    };
    // Retain the existing inverse policy for these small hypothesis workspaces,
    // which hold assignments/settings rather than sequence residues.
    let before = Box::new(work.clone());
    basecraft_docs::delimitation::apply(work, edit).map_err(|e| invalid(e.0))?;
    Ok((Inverse::Delimitation(before), None))
}

fn sequence_edit(content: &mut Loaded, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
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
    let Loaded::Sequence(sequence) = content else {
        return Err(invalid("proposals require a sequence document"));
    };
    execute_sequence(&mut sequence.doc, op)
}

fn sequence_fingerprint(content: &Loaded) -> Result<DocState> {
    let Loaded::Sequence(sequence) = content else {
        return Err(invalid("proposals require a sequence document"));
    };
    Ok(sequence_state(&sequence.doc))
}

fn delimitation_edit(content: &mut Loaded, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
    use basecraft_protocol::delimitation::DelimitationEdit;
    if !matches!(op, Op::DelimitationEdit { edit } if matches!(edit.as_ref(), DelimitationEdit::Remove { .. } | DelimitationEdit::Combine { .. }))
    {
        return Err(invalid(
            "proposal contains an unsupported hypothesis edit or host/lifecycle operation",
        ));
    }
    let Loaded::Delimitation(work) = content else {
        return Err(invalid("proposals require a species workspace"));
    };
    execute_delimitation(work, op)
}

fn delimitation_fingerprint(content: &Loaded) -> Result<DocState> {
    let Loaded::Delimitation(work) = content else {
        return Err(invalid("proposals require a species workspace"));
    };
    Ok(delimitation_state(work))
}

impl Session {
    fn proposal_target(
        &self,
        id: DocId,
        generation: u64,
        revision: u64,
        count: usize,
    ) -> Result<&Slot> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        if slot.generation != generation || self.revision(id) != revision {
            return Err(invalid("stale_revision: proposal target changed"));
        }
        if count > 4096 {
            return Err(invalid("proposal count exceeds 4096"));
        }
        Ok(slot)
    }

    /// Retains the registered sequence proposal contract, including explicit
    /// plugin actor, feature identities and the four-edit allowlist.
    pub fn apply_sequence_proposals(
        &mut self,
        id: DocId,
        generation: u64,
        revision: u64,
        ops: &[Op],
        actor: &Actor,
    ) -> Result<Vec<String>> {
        let slot = self.proposal_target(id, generation, revision, ops.len())?;
        let Loaded::Sequence(sequence) = &slot.content else {
            return Err(invalid("proposals require a sequence document"));
        };
        let candidate = Loaded::Sequence(Opened {
            doc: sequence.doc.clone(),
            format: sequence.format,
            fasta: sequence.fasta.clone(),
        });
        let batch = self.stage_proposals(
            id,
            ops,
            actor,
            candidate,
            sequence_edit,
            sequence_fingerprint,
        )?;
        self.commit_proposals(batch)
    }

    /// Stage an explicit native hypothesis proposal batch with the session's
    /// person/script/agent actor. No edit is applied individually to live state.
    pub fn apply_delimitation_proposals(
        &mut self,
        id: DocId,
        generation: u64,
        revision: u64,
        ops: &[Op],
    ) -> Result<Vec<String>> {
        let slot = self.proposal_target(id, generation, revision, ops.len())?;
        let Loaded::Delimitation(work) = &slot.content else {
            return Err(invalid("proposals require a species workspace"));
        };
        let candidate = Loaded::Delimitation(work.clone());
        let batch = self.stage_proposals(
            id,
            ops,
            &self.actor,
            candidate,
            delimitation_edit,
            delimitation_fingerprint,
        )?;
        self.commit_proposals(batch)
    }

    fn stage_proposals(
        &self,
        id: DocId,
        ops: &[Op],
        actor: &Actor,
        mut candidate: Loaded,
        execute: Execute,
        fingerprint: Fingerprint,
    ) -> Result<PreparedBatch> {
        let mut edits = Vec::with_capacity(ops.len());
        let mut entries = Vec::with_capacity(ops.len());
        for op in ops {
            let (inverse, feature) = execute(&mut candidate, op)?;
            let mut entry = self.entry(
                id,
                actor.clone(),
                op.clone(),
                OpResult {
                    hashes: vec![],
                    feature: feature.map(|id| id.0),
                },
            );
            // Reuse the migrated Document interface when the payload supports
            // it; legacy adapters delegate to the same helpers as Session state.
            entry.state = if let Some(document) = candidate.as_document() {
                document.fingerprint()
            } else {
                fingerprint(&candidate)?
            };
            edits.push(PreparedEdit {
                op: op.clone(),
                inverse,
                entry_id: entry.id.clone(),
            });
            entries.push(entry);
        }
        Ok(PreparedBatch {
            doc: id,
            generation: self.generation(id),
            revision: self.revision(id),
            candidate,
            edits,
            entries,
        })
    }

    fn commit_proposals(&mut self, batch: PreparedBatch) -> Result<Vec<String>> {
        let PreparedBatch {
            doc,
            generation,
            revision,
            candidate,
            edits,
            entries,
        } = batch;
        self.proposal_target(doc, generation, revision, edits.len())?;
        if edits.is_empty() {
            return Ok(vec![]);
        }
        let next_generation = generation
            .checked_add(edits.len() as u64)
            .ok_or_else(|| invalid("proposal generation overflow"))?;
        let group = self.next_group;
        let next_group = group
            .checked_add(1)
            .ok_or_else(|| invalid("proposal group overflow"))?;
        let ids = entries.iter().map(|entry| entry.id.clone()).collect();
        let applied: Vec<_> = edits
            .into_iter()
            .map(|edit| Applied {
                entry_id: edit.entry_id,
                op: edit.op,
                inverse: edit.inverse,
                group: Some(group),
            })
            .collect();
        self.flush()?;
        self.track(doc)?;
        self.events.reserve(entries.len());
        self.docs[doc.0].undo.reserve(applied.len());
        if let Some(log) = &mut self.log {
            log.append_batch(&entries)?;
        }
        // All fallible preparation precedes the durable batch. Installing the
        // already checked payload requires no post-append downcast or I/O.
        if self.tracked {
            self.events.extend(entries);
        }
        let slot = &mut self.docs[doc.0];
        slot.content = candidate;
        slot.generation = next_generation;
        slot.redo.clear();
        slot.undo.extend(applied);
        self.next_group = next_group;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::{Feature, delimitation::DelimitationEdit};
    use std::path::Path;

    fn fixture() -> (tempfile::TempDir, Session, DocId) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.spart");
        std::fs::write(
            &path,
            include_str!("../../../../testdata/fixtures/delimitation/manual.spart"),
        )
        .unwrap();
        let mut session = Session::new(dir.path());
        let doc = session.open(&path).unwrap();
        (dir, session, doc)
    }

    fn remove(id: &str) -> Op {
        Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Remove {
                hypotheses: [id.to_string()].into(),
            }),
        }
    }

    fn apply(session: &mut Session, doc: DocId, ops: &[Op]) -> Result<Vec<String>> {
        session.apply_delimitation_proposals(
            doc,
            session.generation(doc),
            session.revision(doc),
            ops,
        )
    }

    #[test]
    fn stages_all_edits_before_mutation_preserves_redo_and_checks_both_pins() {
        let (dir, mut session, doc) = fixture();
        session.apply(doc, remove("partition-3")).unwrap();
        session.undo(doc).unwrap();
        let before = session.delimitation(doc).clone();
        let generation = session.generation(doc);
        let revision = session.revision(doc);
        let journal = crate::log::read_all(dir.path()).unwrap();
        let events = session.events.clone();
        assert!(
            apply(
                &mut session,
                doc,
                &[remove("partition-1"), remove("missing")]
            )
            .is_err()
        );
        for (g, r) in [(generation + 1, revision), (generation, revision + 1)] {
            assert!(
                session
                    .apply_delimitation_proposals(doc, g, r, &[remove("partition-1")])
                    .is_err()
            );
            assert!(
                session
                    .apply_delimitation_proposals(doc, g, r, &[])
                    .is_err()
            );
        }
        assert!(
            apply(
                &mut session,
                doc,
                &[Op::Save {
                    path: "unsafe.spart".into()
                }]
            )
            .is_err()
        );
        assert!(apply(&mut session, doc, &vec![remove("partition-1"); 4097]).is_err());
        assert!(
            session
                .apply_delimitation_proposals(DocId(999), 0, 0, &[])
                .is_err()
        );
        assert_eq!(session.delimitation(doc), &before);
        assert_eq!(session.generation(doc), generation);
        assert!(session.can_redo(doc));
        assert_eq!(session.events, events);
        assert_eq!(crate::log::read_all(dir.path()).unwrap(), journal);
        assert_eq!(apply(&mut session, doc, &[]).unwrap(), Vec::<String>::new());
        assert!(session.can_redo(doc));
        session.redo(doc).unwrap();
        assert_eq!(session.delimitation(doc).hypotheses.len(), 2);
    }

    #[test]
    fn prepared_batch_cannot_commit_after_an_intervening_edit() {
        let (_dir, mut session, doc) = fixture();
        let candidate = Loaded::Delimitation(session.delimitation(doc).clone());
        let batch = session
            .stage_proposals(
                doc,
                &[remove("partition-1")],
                &session.actor,
                candidate,
                delimitation_edit,
                delimitation_fingerprint,
            )
            .unwrap();
        session.apply(doc, remove("partition-3")).unwrap();
        let after_edit = session.delimitation(doc).clone();
        let events = session.events.clone();
        assert!(session.commit_proposals(batch).is_err());
        assert_eq!(session.delimitation(doc), &after_edit);
        assert_eq!(session.events, events);
        assert_eq!(session.generation(doc), 1);
    }

    fn journal_bytes(root: &Path) -> Vec<Vec<u8>> {
        let mut paths = Vec::new();
        for host in std::fs::read_dir(crate::log::log_dir(root)).unwrap() {
            for entry in std::fs::read_dir(host.unwrap().path()).unwrap() {
                paths.push(entry.unwrap().path());
            }
        }
        paths.sort();
        paths
            .into_iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect()
    }

    #[test]
    fn partial_write_and_sync_failure_roll_back_without_live_changes_and_retry_once() {
        for sync_failure in [false, true] {
            let (dir, mut session, doc) = fixture();
            session.track(doc).unwrap();
            let before = session.delimitation(doc).clone();
            let journal = journal_bytes(dir.path());
            let events = session.events.clone();
            let group = session.next_group;
            let faults = &mut session.log.as_mut().unwrap().faults;
            if sync_failure {
                faults.sync = true;
            } else {
                faults.after_bytes = Some(127);
            }
            let ops = [remove("partition-1"), remove("partition-3")];
            assert!(apply(&mut session, doc, &ops).is_err());
            assert_eq!(session.delimitation(doc), &before);
            assert_eq!(session.generation(doc), 0);
            assert!(!session.can_undo(doc));
            assert_eq!(session.events, events);
            assert_eq!(session.next_group, group);
            assert_eq!(journal_bytes(dir.path()), journal);
            let ids = apply(&mut session, doc, &ops).unwrap();
            assert_eq!(ids.len(), 2);
            assert_eq!(session.delimitation(doc).hypotheses.len(), 1);
            assert_eq!(session.generation(doc), 2);
            let entries = crate::log::read_all(dir.path()).unwrap();
            assert_eq!(entries.len(), events.len() + 2);
            assert_eq!(entries[entries.len() - 2].state.features, 2);
            assert_eq!(entries[entries.len() - 1].state.features, 1);
            session.undo(doc).unwrap();
            assert_eq!(session.delimitation(doc), &before);
            session.redo(doc).unwrap();
            assert_eq!(session.delimitation(doc).hypotheses.len(), 1);
            assert!(crate::replay::replay(dir.path()).unwrap().ok());
        }
    }

    #[test]
    fn rollback_durability_failure_poisons_the_same_writer_without_applying_edits() {
        let (dir, mut session, doc) = fixture();
        session.track(doc).unwrap();
        let before = session.delimitation(doc).clone();
        let faults = &mut session.log.as_mut().unwrap().faults;
        faults.after_bytes = Some(127);
        faults.rollback_sync = true;
        assert!(apply(&mut session, doc, &[remove("partition-1")]).is_err());
        assert!(dir.path().join("recovery-integrity-error.json").exists());
        assert!(apply(&mut session, doc, &[remove("partition-1")]).is_err());
        assert_eq!(session.delimitation(doc), &before);
        assert_eq!(session.generation(doc), 0);
        assert!(!session.can_undo(doc));
    }

    #[test]
    fn oversized_batch_and_wrong_kind_are_rejected_without_live_edits() {
        let (dir, mut session, doc) = fixture();
        session.track(doc).unwrap();
        let before = session.delimitation(doc).clone();
        let mut hypothesis = before.hypotheses[0].clone();
        hypothesis.id = "large".into();
        hypothesis.name = "x".repeat(8 << 20);
        let op = Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Combine {
                hypotheses: vec![hypothesis],
            }),
        };
        let journal = journal_bytes(dir.path());
        assert!(apply(&mut session, doc, &[op]).is_err());
        assert_eq!(session.delimitation(doc), &before);
        assert_eq!(journal_bytes(dir.path()), journal);
        assert_eq!(session.generation(doc), 0);
        let path = dir.path().join("sequence.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let sequence = session.open(&path).unwrap();
        assert!(apply(&mut session, sequence, &[remove("partition-1")]).is_err());
        assert!(
            session
                .apply_sequence_proposals(doc, 0, 0, &[], &Actor::External)
                .is_err()
        );
    }

    #[test]
    fn pending_typing_survives_a_flush_failure_before_proposal_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sequence.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let mut session = Session::new(dir.path());
        let doc = session.open(&path).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 4,
                    end: 4,
                    residues: "A".into(),
                },
            )
            .unwrap();
        assert!(session.pending.is_some());
        let generation = session.generation(doc);
        let revision = session.revision(doc);
        let journal = journal_bytes(dir.path());
        let op = Op::SeqReplace {
            start: 0,
            end: 1,
            residues: "T".into(),
        };
        session.log.as_mut().unwrap().faults.after_bytes = Some(127);
        assert!(
            session
                .apply_sequence_proposals(
                    doc,
                    generation,
                    revision,
                    std::slice::from_ref(&op),
                    &Actor::External
                )
                .is_err()
        );
        assert!(session.pending.is_some());
        assert_eq!(session.doc(doc).read(0, 5), b"ACGTA");
        assert_eq!(session.generation(doc), generation);
        assert_eq!(journal_bytes(dir.path()), journal);
        session
            .apply_sequence_proposals(doc, generation, revision, &[op], &Actor::External)
            .unwrap();
        assert!(session.pending.is_none());
        assert_eq!(session.doc(doc).read(0, 5), b"TCGTA");
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).read(0, 5), b"ACGTA");
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).read(0, 4), b"ACGT");
        assert!(crate::replay::replay(dir.path()).unwrap().ok());
    }

    #[test]
    fn sequence_uses_the_same_batch_rollback_and_preserves_feature_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sequence.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let mut session = Session::new(dir.path());
        let doc = session.open(&path).unwrap();
        session.track(doc).unwrap();
        let before = journal_bytes(dir.path());
        let op = Op::SeqReplace {
            start: 0,
            end: 1,
            residues: "T".into(),
        };
        let actor = Actor::Agent {
            model: "proposal-test".into(),
        };
        session.log.as_mut().unwrap().faults.after_bytes = Some(127);
        assert!(
            session
                .apply_sequence_proposals(doc, 0, 0, std::slice::from_ref(&op), &actor)
                .is_err()
        );
        assert_eq!(journal_bytes(dir.path()), before);
        assert_eq!(session.doc(doc).read(0, 4), b"ACGT");
        assert_eq!(session.generation(doc), 0);
        session
            .apply_sequence_proposals(doc, 0, 0, &[op], &actor)
            .unwrap();
        assert_eq!(session.doc(doc).read(0, 4), b"TCGT");
        let feature = Feature {
            kind: "CDS".into(),
            location: basecraft_protocol::Location::simple(
                0,
                2,
                basecraft_protocol::Strand::Forward,
            ),
            qualifiers: vec![],
        };
        let fid = session
            .apply(
                doc,
                Op::FeatureAdd {
                    feature: feature.clone(),
                },
            )
            .unwrap()
            .feature
            .unwrap();
        let mut changed = feature.clone();
        changed.kind = "gene".into();
        let generation = session.generation(doc);
        let revision = session.revision(doc);
        session
            .apply_sequence_proposals(
                doc,
                generation,
                revision,
                &[
                    Op::FeatureUpdate {
                        id: fid.0,
                        feature: changed,
                    },
                    Op::FeatureRemove { id: fid.0 },
                ],
                &actor,
            )
            .unwrap();
        assert!(session.doc(doc).feature(fid).is_none());
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).feature(fid), Some(&feature));
        session.redo(doc).unwrap();
        assert!(session.doc(doc).feature(fid).is_none());
        assert!(crate::replay::replay(dir.path()).unwrap().ok());
    }
}
