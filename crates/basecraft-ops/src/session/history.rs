//! Shared edit, grouping, undo, redo and journal ownership.
use super::{
    Actor, AlnUndo, Applied, DocId, EditResult, Error, Feature, FeatureId, Instant, LOG_VERSION,
    LogEntry, MERGE_WINDOW, Op, OpResult, Outcome, Pending, ReplayMode, Result, Session, invalid,
    mergeable, restore_inverse,
};

pub(crate) enum Inverse {
    SangerSelect(String),
    // Structural analysis continuations can replace all call sets; ordinary decisions use edit-sized deltas.
    SangerUpdate(Box<crate::sanger_review::SangerReview>),
    SangerReview {
        call_set: String,
        change: Box<basecraft_docs::sanger::review::ReviewChange>,
        events: usize,
    },
    Residues {
        inserted: u64,
        result: EditResult,
    },
    Feature {
        id: FeatureId,
        before: Option<Feature>,
    },
    Alignment(Vec<AlnUndo>),
    Tree(basecraft_docs::tree::TreeUndo),
    Delimitation(Box<basecraft_protocol::delimitation::DelimitationWorkspace>),
    /// The whole metadata as it was: one edit can touch every table.
    Metadata(Box<basecraft_protocol::metadata::ProjectMetadata>),
    Submission(Box<basecraft_protocol::submission::Submission>),
}

impl Session {
    pub(super) fn entry(&self, doc: DocId, actor: Actor, op: Op, result: OpResult) -> LogEntry {
        LogEntry {
            v: LOG_VERSION,
            id: self.next_id(),
            t: basecraft_platform::now_rfc3339(),
            host: self.host.clone(),
            session: self.id.clone(),
            actor,
            doc: self.docs[doc.0].key.clone(),
            op,
            result,
            state: self.docs[doc.0].content.as_document().fingerprint(),
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
        if let Some(log) = &mut self.log {
            log.append(entry)?;
        }
        if self.tracked {
            self.events.push(entry.clone());
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

    pub fn apply_as(&mut self, doc: DocId, mut op: Op, actor: Actor) -> Result<Outcome> {
        let content = &self
            .docs
            .get(doc.0)
            .ok_or_else(|| invalid("unknown document"))?
            .content;
        if !content.as_document().accepts(&op) {
            return Err(Error::Unsupported(format!(
                "{} cannot be applied to {:?}",
                op.name(),
                content.kind()
            )));
        }
        content.as_document().validate(
            &op,
            self.root(),
            matches!(self.replay_mode, ReplayMode::Live),
        )?;
        if let Op::SangerUpdate { review } = &mut op {
            *review =
                self.retain_review_record(&crate::sanger_review::read_journal_record(review)?)?;
        }
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
        // A refused edit restores identical content, so its saved hashes stay valid.
        let cached = self.docs[doc.0].current_saved_hashes.get().cloned();
        let (inverse, feature) = self.docs[doc.0]
            .content_mut()
            .as_document_mut()
            .apply(&op)?;
        if let Err(error) = self.track(doc) {
            restore_inverse(self.docs[doc.0].content_mut(), &inverse)?;
            if let Some(hashes) = cached {
                let _ = self.docs[doc.0].current_saved_hashes.set(hashes);
            }
            return Err(error);
        }
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
        let state = self.state(doc)?;
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
        self.docs.get(id.0).ok_or(Error::UnknownDocument { id })?;
        let group = self.docs[id.0].undo.last().and_then(|a| a.group);
        let mut last = self.undo_one(id)?;
        while group.is_some() && self.docs[id.0].undo.last().and_then(|a| a.group) == group {
            last = self.undo_one(id)?;
        }
        Ok(last)
    }

    /// Redo the last undone step (one operation, or a whole group).
    pub fn redo(&mut self, id: DocId) -> Result<String> {
        self.docs.get(id.0).ok_or(Error::UnknownDocument { id })?;
        let group = self.docs[id.0].redo.last().and_then(|a| a.group);
        let mut last = self.redo_one(id)?;
        while group.is_some() && self.docs[id.0].redo.last().and_then(|a| a.group) == group {
            last = self.redo_one(id)?;
        }
        Ok(last)
    }

    /// Verify an explicitly named history target before changing anything. An empty
    /// target selects the latest edit in this session.
    pub fn apply_history(&mut self, id: DocId, op: &Op) -> Result<String> {
        let slot = self.docs.get(id.0).ok_or(Error::UnknownDocument { id })?;
        let (of, stack, action) = match op {
            Op::Undo { of } => (of, &slot.undo, "undo"),
            Op::Redo { of } => (of, &slot.redo, "redo"),
            _ => return Err(Error::Unsupported("expected a history operation".into())),
        };
        let target = stack.last().ok_or(Error::NothingTo(action))?;
        if !of.is_empty() && *of != target.entry_id {
            return Err(Error::Unsupported(
                "history target is not the latest edit".into(),
            ));
        }
        if matches!(op, Op::Undo { .. }) {
            self.undo(id)
        } else {
            self.redo(id)
        }
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
        self.track(id)?;
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
        self.track(id)?;
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
