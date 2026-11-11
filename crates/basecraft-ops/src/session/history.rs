//! Shared edit, grouping, undo, redo and journal ownership.
use super::{
    Actor, Applied, DocId, Error, FeatureId, Instant, Inverse, LOG_VERSION, LogEntry, MERGE_WINDOW,
    Op, OpResult, Outcome, Pending, ReplayMode, Result, Session, invalid, mergeable,
    restore_inverse,
};

impl Session {
    pub(super) fn entry(&self, doc: DocId, actor: Actor, op: Op, result: OpResult) -> LogEntry {
        LogEntry {
            source: matches!(&op, Op::Open { .. } | Op::CreateSequence { .. }).then(|| {
                basecraft_protocol::ops::SourceRecord {
                    root: self
                        .root
                        .canonicalize()
                        .unwrap_or_else(|_| self.root.clone()),
                    files: Vec::new(),
                }
            }),
            v: LOG_VERSION,
            id: self.next_id(),
            t: basecraft_platform::now_rfc3339(),
            host: self.host.clone(),
            session: self.id.clone(),
            actor,
            doc: self.docs[doc.0].key.clone(),
            op,
            result,
            state: self.state(doc),
        }
    }

    pub(super) fn next_id(&self) -> String {
        let mut g = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.generate()
            .unwrap_or_else(|_| ulid::Ulid::generate())
            .to_string()
    }

    pub(super) fn write(&mut self, entry: &LogEntry) -> Result<()> {
        self.write_entries(std::slice::from_ref(entry))
    }

    pub(super) fn write_entries(&mut self, entries: &[LogEntry]) -> Result<()> {
        let mut batch = self.pending_baselines.clone();
        batch.extend_from_slice(entries);
        if let Some(log) = &mut self.log {
            log.append_batch(&batch)?;
        }
        self.pending_baselines.clear();
        if self.tracked {
            self.events.extend(batch);
        }
        Ok(())
    }

    /// Write out a pending typing burst.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(p) = self.pending.take()
            && let Err(error) = self.write(&p.entry)
        {
            self.pending = Some(p);
            return Err(error);
        }
        Ok(())
    }

    /// Apply an edit as the session's default actor.
    pub fn apply(&mut self, doc: DocId, op: Op) -> Result<Outcome> {
        let actor = self.actor.clone();
        self.apply_as(doc, op, actor)
    }

    pub fn apply_as(&mut self, doc: DocId, op: Op, actor: Actor) -> Result<Outcome> {
        let content = &self
            .docs
            .get(doc.0)
            .ok_or_else(|| invalid("unknown document"))?
            .content;
        content
            .as_document()
            .check_preparation_kind(&op, matches!(self.replay_mode, ReplayMode::Live))?;
        let prepared = content.as_document().prepare_edit(
            &op,
            self.root(),
            matches!(self.replay_mode, ReplayMode::Live),
        )?;
        self.track(doc)?;
        if self.merge && self.try_merge(doc, &op, &actor)? {
            self.docs[doc.0].generation += 1;
            let id = self
                .pending
                .as_ref()
                .map(|p| p.entry.id.clone())
                .unwrap_or_default();
            return Ok(Outcome {
                entry_id: id,
                feature: None,
            });
        }
        self.flush()?;
        let (inverse, feature) = self.docs[doc.0]
            .content_mut()
            .as_document_mut()
            .apply_prepared(&op, prepared)?;
        self.docs[doc.0].generation += 1;
        let result = OpResult {
            hashes: Vec::new(),
            feature: feature.map(|f| f.0),
        };
        let entry = self.entry(doc, actor, op.clone(), result);
        let entry_id = entry.id.clone();
        let mergeable = mergeable(&op);
        if !(self.merge && mergeable)
            && let Err(error) = self.write(&entry)
        {
            restore_inverse(self.docs[doc.0].content_mut(), &inverse)?;
            self.docs[doc.0].generation -= 1;
            return Err(error);
        }
        let slot = &mut self.docs[doc.0];
        slot.redo.clear();
        slot.undo.push(Applied {
            entry_id: entry_id.clone(),
            op,
            inverse,
            group: self.group,
        });
        if mergeable && self.merge {
            self.pending = Some(Pending {
                doc,
                entry,
                last: Instant::now(),
            });
        }
        Ok(Outcome { entry_id, feature })
    }

    /// Merge an insertion into the pending burst if it continues it.
    pub(super) fn try_merge(&mut self, doc: DocId, op: &Op, actor: &Actor) -> Result<bool> {
        let Some(p) = &self.pending else {
            return Ok(false);
        };
        if p.doc != doc || p.entry.actor != *actor || p.last.elapsed() >= MERGE_WINDOW {
            return Ok(false);
        }
        let Some(merged) = self.docs[doc.0]
            .content
            .as_document()
            .merge(&p.entry.op, op)
        else {
            return Ok(false);
        };
        let (inverse, _) = self.execute(doc, op)?;
        let state = self.state(doc);
        let slot = &mut self.docs[doc.0];
        slot.redo.clear();
        let top = slot
            .undo
            .last_mut()
            .ok_or_else(|| invalid("pending burst without undo entry"))?;
        match (&mut top.inverse, inverse) {
            (Inverse::Alignment(first), Inverse::Alignment(steps)) => first.extend(steps),
            (
                Inverse::Residues {
                    inserted,
                    result: first,
                },
                Inverse::Residues {
                    inserted: added,
                    result,
                },
            ) => {
                *inserted += added;
                // Keep the pre-burst state of each feature; add features the
                // burst touched later.
                for (id, f) in result.before {
                    if !first.before.iter().any(|(i, _)| *i == id) {
                        first.before.push((id, f));
                    }
                }
            }
            _ => return Err(invalid("merged edit has incompatible undo")),
        }
        top.op = merged.clone();
        if let Some(p) = &mut self.pending {
            p.entry.op = merged;
            p.entry.state = state;
            p.last = Instant::now();
        }
        Ok(true)
    }

    /// Carry out an edit on the document; returns its inverse.
    pub(super) fn execute(&mut self, id: DocId, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        self.docs[id.0].content_mut().as_document_mut().apply(op)
    }

    /// Undo the last operation on a document; returns the undone entry's id.
    /// Start an undo group: operations applied until [`Session::end_group`]
    /// undo and redo as one step. Each is still logged on its own, so the
    /// log replays exactly.
    pub fn begin_group(&mut self) -> u64 {
        let group = self.next_group;
        self.group = Some(group);
        self.next_group += 1;
        group
    }

    /// Resume an interrupted group only while it remains the last undo step.
    /// The caller must also pin the document revision before retrying its edits.
    pub fn resume_group(&mut self, id: DocId, group: u64) -> Result<()> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        if self.group.is_some()
            || slot.undo.last().and_then(|edit| edit.group) != Some(group)
            || !slot.redo.is_empty()
        {
            return Err(invalid("cannot resume undo group after intervening edits"));
        }
        self.group = Some(group);
        Ok(())
    }

    pub fn end_group(&mut self) {
        self.group = None;
    }

    /// Undo the last step on a document (one operation, or a whole group);
    /// returns the id of the last entry undone.
    pub fn undo(&mut self, id: DocId) -> Result<String> {
        let group = self.docs[id.0].undo.last().and_then(|a| a.group);
        let mut last = self.undo_one(id)?;
        while group.is_some() && self.docs[id.0].undo.last().and_then(|a| a.group) == group {
            last = self.undo_one(id)?;
        }
        Ok(last)
    }

    /// Redo the last undone step (one operation, or a whole group).
    pub fn redo(&mut self, id: DocId) -> Result<String> {
        let group = self.docs[id.0].redo.last().and_then(|a| a.group);
        let mut last = self.redo_one(id)?;
        while group.is_some() && self.docs[id.0].redo.last().and_then(|a| a.group) == group {
            last = self.redo_one(id)?;
        }
        Ok(last)
    }

    /// Undo exactly one operation, ignoring groups. Replay uses this: the
    /// log holds one undo entry per operation.
    pub fn undo_single(&mut self, id: DocId) -> Result<String> {
        self.undo_one(id)
    }

    pub fn redo_single(&mut self, id: DocId) -> Result<String> {
        self.redo_one(id)
    }

    pub(super) fn undo_one(&mut self, id: DocId) -> Result<String> {
        self.flush()?;
        let applied = self.docs[id.0].undo.pop().ok_or(Error::NothingTo("undo"))?;
        self.docs[id.0]
            .content_mut()
            .as_document_mut()
            .revert(&applied.inverse)?;
        self.docs[id.0].generation += 1;
        let of = applied.entry_id.clone();
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Undo { of: of.clone() },
            OpResult::default(),
        );
        if let Err(error) = self.write(&entry) {
            let inverse = self.reexecute(id, &applied)?;
            self.docs[id.0].generation -= 1;
            self.docs[id.0].undo.push(Applied { inverse, ..applied });
            return Err(error);
        }
        self.docs[id.0].redo.push(applied);
        Ok(of)
    }

    pub(super) fn redo_one(&mut self, id: DocId) -> Result<String> {
        self.flush()?;
        let applied = self.docs[id.0].redo.pop().ok_or(Error::NothingTo("redo"))?;
        let inverse = self.reexecute(id, &applied)?;
        self.docs[id.0].generation += 1;
        let of = applied.entry_id.clone();
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Redo { of: of.clone() },
            OpResult::default(),
        );
        if let Err(error) = self.write(&entry) {
            restore_inverse(self.docs[id.0].content_mut(), &inverse)?;
            self.docs[id.0].generation -= 1;
            self.docs[id.0].redo.push(applied);
            return Err(error);
        }
        self.docs[id.0].undo.push(Applied { inverse, ..applied });
        Ok(of)
    }

    pub(super) fn reexecute(&mut self, id: DocId, applied: &Applied) -> Result<Inverse> {
        self.docs[id.0]
            .content_mut()
            .as_document_mut()
            .reapply(&applied.op, &applied.inverse)
    }
}
