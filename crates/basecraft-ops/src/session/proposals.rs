//! One staged proposal/commit owner. Kind adapters supply only candidate edits
//! and fingerprints; Session retains pins, actors, journal and undo ownership.
use super::{Applied, Inverse, Session, Slot};
use crate::{DocId, Result, error::invalid, files::Loaded};
use basecraft_protocol::{
    Actor, LogEntry, Op, OpResult,
    ops::{ProposalDecision, ProposalSource},
};

/// Edits a plugin offers for a sequence, as it sent them, pinned to the
/// document generation and revision it read.
pub struct SequenceOffer<'a> {
    pub generation: u64,
    pub revision: u64,
    pub operations: &'a [Op],
    /// Who offered them.
    pub actor: &'a Actor,
    pub source: ProposalSource,
    /// Leave out feature additions the document already holds.
    pub skip_known: bool,
}

/// What became of an offer: the log ids of the edits applied, and why any
/// were left out.
#[derive(Debug, Default)]
pub struct ProposalResult {
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
}

/// The offered edits to apply, and a note for each feature addition left out
/// because the document already holds that feature.
fn known_left_out(slot: &Slot, operations: &[Op], skip_known: bool) -> (Vec<Op>, Vec<String>) {
    let known = slot.content.sequence().filter(|_| skip_known);
    let mut skipped = Vec::new();
    let kept = operations
        .iter()
        .filter(|op| match (op, known) {
            (Op::FeatureAdd { feature, .. }, Some(opened))
                if opened.doc.features().any(|(_, present)| {
                    present.kind == feature.kind && present.location == feature.location
                }) =>
            {
                skipped.push(format!("already annotated: {}", feature.kind));
                false
            }
            _ => true,
        })
        .cloned()
        .collect();
    (kept, skipped)
}

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
            .ok_or(crate::Error::UnknownDocument { id })?;
        if slot.generation != generation || self.revision(id)? != revision {
            return Err(crate::Error::StaleRevision { id, revision });
        }
        if count > 4096 {
            return Err(crate::Error::Limit {
                what: "proposal count".into(),
                limit: 4096,
            });
        }
        Ok(slot)
    }

    /// Apply the edits a plugin offers for a sequence as one undo group, and
    /// record the offer and the decision around them: accepted, modified
    /// when known features were left out, or rejected when the document
    /// changed since the plugin read it, or the edits don't apply. A refused
    /// batch returns its error after the rejection is recorded. The plugin's
    /// actor, feature identities and the four-edit allowlist are kept.
    pub fn apply_sequence_proposals(
        &mut self,
        id: DocId,
        offer: SequenceOffer<'_>,
    ) -> Result<ProposalResult> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or(crate::Error::UnknownDocument { id })?;
        if slot.content.kind() != crate::files::Kind::Sequence {
            return Err(invalid("proposals require a sequence document"));
        }
        let SequenceOffer {
            generation,
            revision,
            operations,
            actor,
            source,
            skip_known,
        } = offer;
        if operations.is_empty() {
            self.proposal_target(id, generation, revision, 0)?;
            return Ok(ProposalResult::default());
        }
        self.track(id)?;
        let proposal = self.next_id();
        let offered = self.entry(
            id,
            actor.clone(),
            Op::ProposalOffer {
                proposal: proposal.clone(),
                source,
                base_revision: revision,
                operations: operations.to_vec(),
            },
            OpResult::default(),
        );
        let decide = |session: &Self, outcome, applied, note| {
            session.entry(
                id,
                session.actor.clone(),
                Op::ProposalDecide {
                    proposal: proposal.clone(),
                    outcome,
                    automatic: true,
                    applied,
                    note,
                },
                OpResult::default(),
            )
        };
        let staged = self
            .proposal_target(id, generation, revision, operations.len())
            .and_then(|slot| {
                let (kept, skipped) = known_left_out(slot, operations, skip_known);
                let candidate = slot.content.as_document().proposal_candidate()?;
                let batch = self.stage_proposals(id, &kept, actor, candidate)?;
                Ok((batch, skipped))
            });
        let (mut batch, skipped) = match staged {
            Ok(staged) => staged,
            Err(error @ crate::Error::Limit { .. }) => return Err(error),
            Err(error) => {
                let refused = decide(
                    self,
                    ProposalDecision::Rejected,
                    Vec::new(),
                    Some(error.to_string()),
                );
                self.write_entries(&[offered, refused])?;
                return Err(error);
            }
        };
        let note = (!skipped.is_empty()).then(|| format!("left out {}", skipped.join("; ")));
        let applied: Vec<String> = batch.edits.iter().map(|e| e.entry_id.clone()).collect();
        let outcome = match (applied.is_empty(), skipped.is_empty()) {
            (true, _) => ProposalDecision::Rejected,
            (false, true) => ProposalDecision::Accepted,
            (false, false) => ProposalDecision::Modified,
        };
        let mut decided = decide(self, outcome, applied, note);
        decided.state = batch.candidate.as_document().fingerprint();
        batch.entries.insert(0, offered);
        batch.entries.push(decided);
        let applied = self.commit_proposals(batch)?;
        Ok(ProposalResult { applied, skipped })
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
        if slot.content.kind() != crate::files::Kind::Delimitation {
            return Err(invalid("proposals require a species workspace"));
        }
        let candidate = slot.content.as_document().proposal_candidate()?;
        let batch = self.stage_proposals(id, ops, &self.actor, candidate)?;
        self.commit_proposals(batch)
    }

    fn stage_proposals(
        &self,
        id: DocId,
        ops: &[Op],
        actor: &Actor,
        mut candidate: Loaded,
    ) -> Result<PreparedBatch> {
        let mut edits = Vec::with_capacity(ops.len());
        let mut entries = Vec::with_capacity(ops.len());
        for op in ops {
            let mut op = op.clone();
            candidate.as_document().name_record(&mut op)?;
            let (inverse, feature) = candidate.as_document_mut().apply_proposal(&op)?;
            let mut entry = self.entry(
                id,
                actor.clone(),
                op.clone(),
                OpResult {
                    notes: Vec::new(),
                    hashes: vec![],
                    feature: feature.map(|id| id.0),
                },
            );
            entry.state = candidate.as_document().fingerprint();
            edits.push(PreparedEdit {
                op,
                inverse,
                entry_id: entry.id.clone(),
            });
            entries.push(entry);
        }
        Ok(PreparedBatch {
            doc: id,
            generation: self.generation(id)?,
            revision: self.revision(id)?,
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
        if entries.is_empty() {
            return Ok(vec![]);
        }
        let next_generation = generation
            .checked_add(edits.len() as u64)
            .ok_or_else(|| invalid("proposal generation overflow"))?;
        let group = self.next_group;
        let next_group = group
            .checked_add(1)
            .ok_or_else(|| invalid("proposal group overflow"))?;
        let ids = edits.iter().map(|edit| edit.entry_id.clone()).collect();
        let applied: Vec<_> = edits
            .into_iter()
            .map(|edit| Applied {
                entry_id: edit.entry_id,
                op: edit.op,
                inverse: edit.inverse,
                group: Some(group),
            })
            .collect();
        self.track(doc)?;
        self.docs[doc.0].undo.reserve(applied.len());
        self.write_entries(&entries)?;
        // All fallible preparation precedes the durable batch. Installing the
        // already checked payload requires no post-append downcast or I/O.
        let slot = &mut self.docs[doc.0];
        *slot.content_mut() = candidate;
        slot.generation = next_generation;
        let mut dropped = slot.clear_redo();
        slot.undo.extend(applied);
        let settled = slot.settle();
        dropped.extend(self.settle_members(settled));
        self.forget_groups(dropped);
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

    fn offer<'a>(
        generation: u64,
        revision: u64,
        operations: &'a [Op],
        actor: &'a Actor,
    ) -> SequenceOffer<'a> {
        SequenceOffer {
            generation,
            revision,
            operations,
            actor,
            source: ProposalSource::Run {
                id: "test-run".into(),
            },
            skip_known: false,
        }
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
            session.generation(doc).unwrap(),
            session.revision(doc).unwrap(),
            ops,
        )
    }

    #[test]
    fn stages_all_edits_before_mutation_preserves_redo_and_checks_both_pins() {
        let (dir, mut session, doc) = fixture();
        session.apply(doc, remove("partition-3")).unwrap();
        session.undo(doc).unwrap();
        let before = session.delimitation(doc).unwrap().clone();
        let generation = session.generation(doc).unwrap();
        let revision = session.revision(doc).unwrap();
        let journal = crate::log::read_all(dir.path()).unwrap();
        let events = session.events.matching(|_| true).unwrap();
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
                    path: "unsafe.spart".into(),
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
        assert_eq!(session.delimitation(doc).unwrap(), &before);
        assert_eq!(session.generation(doc).unwrap(), generation);
        assert!(session.can_redo(doc).unwrap());
        assert_eq!(session.events.matching(|_| true).unwrap(), events);
        assert_eq!(crate::log::read_all(dir.path()).unwrap(), journal);
        assert_eq!(apply(&mut session, doc, &[]).unwrap(), Vec::<String>::new());
        assert!(session.can_redo(doc).unwrap());
        session.redo(doc).unwrap();
        assert_eq!(session.delimitation(doc).unwrap().hypotheses.len(), 2);
    }

    #[test]
    fn prepared_batch_cannot_commit_after_an_intervening_edit() {
        let (_dir, mut session, doc) = fixture();
        let candidate = Loaded::Delimitation(session.delimitation(doc).unwrap().clone());
        let batch = session
            .stage_proposals(doc, &[remove("partition-1")], &session.actor, candidate)
            .unwrap();
        session.apply(doc, remove("partition-3")).unwrap();
        let after_edit = session.delimitation(doc).unwrap().clone();
        let events = session.events.matching(|_| true).unwrap();
        assert!(session.commit_proposals(batch).is_err());
        assert_eq!(session.delimitation(doc).unwrap(), &after_edit);
        assert_eq!(session.events.matching(|_| true).unwrap(), events);
        assert_eq!(session.generation(doc).unwrap(), 1);
    }

    fn journal_bytes(root: &Path) -> Vec<Vec<u8>> {
        let mut paths = Vec::new();
        let hosts = match std::fs::read_dir(crate::log::log_dir(root)) {
            Ok(hosts) => hosts,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => panic!("cannot read journal: {error}"),
        };
        for host in hosts {
            for entry in std::fs::read_dir(host.unwrap().path()).unwrap() {
                paths.push(entry.unwrap().path());
            }
        }
        paths.sort();
        paths
            .into_iter()
            .map(|path| std::fs::read(path).unwrap())
            .filter(|bytes| !bytes.is_empty())
            .collect()
    }

    #[test]
    fn partial_write_and_sync_failure_roll_back_without_live_changes_and_retry_once() {
        for sync_failure in [false, true] {
            let (dir, mut session, doc) = fixture();
            session.track(doc).unwrap();
            let before = session.delimitation(doc).unwrap().clone();
            let journal = journal_bytes(dir.path());
            let events = session.events.matching(|_| true).unwrap();
            let group = session.next_group;
            let faults = &mut session.log.as_mut().unwrap().faults;
            if sync_failure {
                faults.sync = true;
            } else {
                faults.after_bytes = Some(127);
            }
            let ops = [remove("partition-1"), remove("partition-3")];
            assert!(apply(&mut session, doc, &ops).is_err());
            assert_eq!(session.delimitation(doc).unwrap(), &before);
            assert_eq!(session.generation(doc).unwrap(), 0);
            assert!(!session.can_undo(doc).unwrap());
            assert_eq!(session.events.matching(|_| true).unwrap(), events);
            assert_eq!(session.next_group, group);
            assert_eq!(journal_bytes(dir.path()), journal);
            let ids = apply(&mut session, doc, &ops).unwrap();
            assert_eq!(ids.len(), 2);
            assert_eq!(session.delimitation(doc).unwrap().hypotheses.len(), 1);
            assert_eq!(session.generation(doc).unwrap(), 2);
            let entries = crate::log::read_all(dir.path()).unwrap();
            assert_eq!(entries.len(), 3);
            assert!(matches!(entries[0].op, Op::Open { .. }));
            assert_eq!(entries[entries.len() - 2].state.features, 2);
            assert_eq!(entries[entries.len() - 1].state.features, 1);
            session.undo(doc).unwrap();
            assert_eq!(session.delimitation(doc).unwrap(), &before);
            session.redo(doc).unwrap();
            assert_eq!(session.delimitation(doc).unwrap().hypotheses.len(), 1);
            assert!(crate::records::replay::replay(dir.path()).unwrap().ok());
        }
    }

    #[test]
    fn rollback_durability_failure_poisons_the_same_writer_without_applying_edits() {
        let (dir, mut session, doc) = fixture();
        session.track(doc).unwrap();
        let before = session.delimitation(doc).unwrap().clone();
        let faults = &mut session.log.as_mut().unwrap().faults;
        faults.after_bytes = Some(127);
        faults.rollback_sync = true;
        assert!(apply(&mut session, doc, &[remove("partition-1")]).is_err());
        assert!(dir.path().join("recovery-integrity-error.json").exists());
        assert!(apply(&mut session, doc, &[remove("partition-1")]).is_err());
        assert_eq!(session.delimitation(doc).unwrap(), &before);
        assert_eq!(session.generation(doc).unwrap(), 0);
        assert!(!session.can_undo(doc).unwrap());
    }

    #[test]
    fn oversized_batch_and_wrong_kind_are_rejected_without_live_edits() {
        let (dir, mut session, doc) = fixture();
        session.track(doc).unwrap();
        let before = session.delimitation(doc).unwrap().clone();
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
        assert_eq!(session.delimitation(doc).unwrap(), &before);
        assert_eq!(journal_bytes(dir.path()), journal);
        assert_eq!(session.generation(doc).unwrap(), 0);
        let path = dir.path().join("sequence.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let sequence = session.open(&path).unwrap();
        assert!(apply(&mut session, sequence, &[remove("partition-1")]).is_err());
        assert!(
            session
                .apply_sequence_proposals(doc, offer(0, 0, &[], &Actor::External))
                .is_err()
        );
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
        let op = Op::seq_replace(0, 1, "T");
        let actor = Actor::Agent {
            model: "proposal-test".into(),
        };
        session.log.as_mut().unwrap().faults.after_bytes = Some(127);
        assert!(
            session
                .apply_sequence_proposals(doc, offer(0, 0, std::slice::from_ref(&op), &actor))
                .is_err()
        );
        assert_eq!(journal_bytes(dir.path()), before);
        assert_eq!(session.doc(doc).unwrap().read(0, 4), b"ACGT");
        assert_eq!(session.generation(doc).unwrap(), 0);
        session
            .apply_sequence_proposals(doc, offer(0, 0, &[op], &actor))
            .unwrap();
        assert_eq!(session.doc(doc).unwrap().read(0, 4), b"TCGT");
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
            .apply(doc, Op::feature_add(feature.clone()))
            .unwrap()
            .feature
            .unwrap();
        let mut changed = feature.clone();
        changed.kind = "gene".into();
        let generation = session.generation(doc).unwrap();
        let revision = session.revision(doc).unwrap();
        session
            .apply_sequence_proposals(
                doc,
                offer(
                    generation,
                    revision,
                    &[
                        Op::feature_update(fid.0, changed),
                        Op::feature_remove(fid.0),
                    ],
                    &actor,
                ),
            )
            .unwrap();
        assert!(session.doc(doc).unwrap().feature(fid).is_none());
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().feature(fid), Some(&feature));
        session.redo(doc).unwrap();
        assert!(session.doc(doc).unwrap().feature(fid).is_none());
        assert!(crate::records::replay::replay(dir.path()).unwrap().ok());
    }
}
