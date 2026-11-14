//! Shared edit, grouping, undo, redo and journal ownership.
use super::{
    Actor, AlnUndo, Applied, DocId, EditResult, Error, Feature, FeatureId, Instant, LOG_VERSION,
    LogEntry, Op, OpResult, Outcome, ReplayMode, Result, Session, TYPING_WINDOW, Typing, invalid,
    restore_inverse, typing,
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
    /// The alphabet an alignment or sequence held before it was set.
    Alphabet(Option<basecraft_protocol::facts::Fact<basecraft_protocol::facts::Alphabet>>),
    Tree(basecraft_docs::tree::TreeUndo),
    Delimitation(Box<WorkspaceUndo>),
    Metadata(Box<MetadataUndo>),
    Submission(Box<SubmissionUndo>),
}

/// The changed window of a list: the entries it held before, and how many
/// entries now stand in their place. Unchanged ends are not retained.
pub(crate) struct Splice<T> {
    start: usize,
    len: usize,
    before: Vec<T>,
}

impl<T: PartialEq + Clone> Splice<T> {
    fn between(mut before: Vec<T>, after: &[T]) -> Self {
        let start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
        let end = before[start..]
            .iter()
            .rev()
            .zip(after[start..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        before.truncate(before.len() - end);
        Self {
            start,
            len: after.len() - start - end,
            before: before.split_off(start),
        }
    }

    fn undo(&self, list: &mut Vec<T>) {
        list.splice(
            self.start..self.start + self.len,
            self.before.iter().cloned(),
        );
    }
}

/// Undo for a species workspace: its small settings whole, its specimen and
/// hypothesis lists only where the edit changed them.
pub(crate) struct WorkspaceUndo {
    rest: basecraft_protocol::delimitation::DelimitationWorkspace,
    specimens: Splice<basecraft_protocol::delimitation::Specimen>,
    hypotheses: Splice<basecraft_protocol::delimitation::Hypothesis>,
}

impl WorkspaceUndo {
    pub(crate) fn between(
        mut rest: basecraft_protocol::delimitation::DelimitationWorkspace,
        after: &basecraft_protocol::delimitation::DelimitationWorkspace,
    ) -> Self {
        let specimens = Splice::between(std::mem::take(&mut rest.specimens), &after.specimens);
        let hypotheses = Splice::between(std::mem::take(&mut rest.hypotheses), &after.hypotheses);
        Self {
            rest,
            specimens,
            hypotheses,
        }
    }

    pub(crate) fn undo(&self, work: &mut basecraft_protocol::delimitation::DelimitationWorkspace) {
        let mut specimens = std::mem::take(&mut work.specimens);
        let mut hypotheses = std::mem::take(&mut work.hypotheses);
        self.specimens.undo(&mut specimens);
        self.hypotheses.undo(&mut hypotheses);
        *work = basecraft_protocol::delimitation::DelimitationWorkspace {
            specimens,
            hypotheses,
            ..self.rest.clone()
        };
    }
}

/// Undo for project metadata: settings and columns whole, the roster rows
/// and record lists only where the edit changed them.
pub(crate) struct MetadataUndo {
    rest: basecraft_protocol::metadata::ProjectMetadata,
    rows: Splice<basecraft_protocol::metadata::Row>,
    taxa: Splice<basecraft_protocol::metadata::Taxon>,
    identifications: Splice<basecraft_protocol::metadata::Identification>,
    links: Splice<basecraft_protocol::metadata::RecordLink>,
    partitions: Splice<basecraft_protocol::delimitation::Hypothesis>,
}

impl MetadataUndo {
    pub(crate) fn between(
        mut rest: basecraft_protocol::metadata::ProjectMetadata,
        after: &basecraft_protocol::metadata::ProjectMetadata,
    ) -> Self {
        use std::mem::take;
        Self {
            rows: Splice::between(take(&mut rest.specimens.rows), &after.specimens.rows),
            taxa: Splice::between(take(&mut rest.taxa), &after.taxa),
            identifications: Splice::between(
                take(&mut rest.identifications),
                &after.identifications,
            ),
            links: Splice::between(take(&mut rest.links), &after.links),
            partitions: Splice::between(take(&mut rest.partitions), &after.partitions),
            rest,
        }
    }

    pub(crate) fn undo(&self, metadata: &mut basecraft_protocol::metadata::ProjectMetadata) {
        use std::mem::take;
        let mut current = take(metadata);
        self.rows.undo(&mut current.specimens.rows);
        self.taxa.undo(&mut current.taxa);
        self.identifications.undo(&mut current.identifications);
        self.links.undo(&mut current.links);
        self.partitions.undo(&mut current.partitions);
        *metadata = basecraft_protocol::metadata::ProjectMetadata {
            specimens: basecraft_protocol::metadata::Table {
                rows: current.specimens.rows,
                ..self.rest.specimens.clone()
            },
            taxa: current.taxa,
            identifications: current.identifications,
            links: current.links,
            partitions: current.partitions,
            ..self.rest.clone()
        };
    }
}

/// Undo for a submission: its settings whole, its per-record lists only
/// where the edit changed them.
pub(crate) struct SubmissionUndo {
    rest: basecraft_protocol::submission::Submission,
    rows: Splice<basecraft_protocol::submission::RowDecision>,
    exported: Splice<basecraft_protocol::submission::ExportMapping>,
    frames: Splice<basecraft_protocol::submission::FrameChoice>,
    accessions: Splice<basecraft_protocol::submission::AccessionEntry>,
}

impl SubmissionUndo {
    pub(crate) fn between(
        mut rest: basecraft_protocol::submission::Submission,
        after: &basecraft_protocol::submission::Submission,
    ) -> Self {
        use std::mem::take;
        Self {
            rows: Splice::between(take(&mut rest.rows), &after.rows),
            exported: Splice::between(take(&mut rest.exported), &after.exported),
            frames: Splice::between(take(&mut rest.frames), &after.frames),
            accessions: Splice::between(take(&mut rest.accessions), &after.accessions),
            rest,
        }
    }

    pub(crate) fn undo(&self, submission: &mut basecraft_protocol::submission::Submission) {
        use std::mem::take;
        let mut current = take(submission);
        self.rows.undo(&mut current.rows);
        self.exported.undo(&mut current.exported);
        self.frames.undo(&mut current.frames);
        self.accessions.undo(&mut current.accessions);
        *submission = basecraft_protocol::submission::Submission {
            rows: current.rows,
            exported: current.exported,
            frames: current.frames,
            accessions: current.accessions,
            ..self.rest.clone()
        };
    }
}

impl Session {
    pub(super) fn entry(&self, doc: DocId, actor: Actor, op: Op, result: OpResult) -> LogEntry {
        LogEntry {
            source: matches!(
                &op,
                Op::Open { .. } | Op::CreateSequence { .. } | Op::Save { .. }
            )
            .then(|| basecraft_protocol::ops::SourceRecord {
                checkpoint: false,
                feature_slots: self.docs[doc.0]
                    .content
                    .sequence()
                    .map_or_else(Vec::new, |opened| opened.doc.feature_slots()),
                node_ids: self.docs[doc.0]
                    .content
                    .tree()
                    .map_or_else(Vec::new, |opened| opened.doc.preorder()),
                root: self
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| self.root.clone()),
                files: Vec::new(),
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
        self.write_entries(std::slice::from_ref(entry))
    }

    pub(super) fn write_entries(&mut self, entries: &[LogEntry]) -> Result<()> {
        let mut batch = std::mem::take(&mut self.pending_baselines);
        batch.extend_from_slice(entries);
        if let Some(log) = &mut self.log
            && let Err(error) = log.append_batch(&batch)
        {
            batch.truncate(batch.len() - entries.len());
            self.pending_baselines = batch;
            return Err(error);
        }
        self.journaled += batch.len() as u64;
        if self.tracked {
            // Each document keeps its own entries until its next save. A file
            // opened twice has two slots under one key; both see its entries.
            for entry in batch {
                for slot in self.docs.iter_mut().filter(|slot| slot.key == entry.doc) {
                    slot.unsaved.push(entry.clone());
                }
            }
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
        content.as_document().name_record(&mut op)?;
        // The log names every label placed, each once.
        if let Op::TreePlaceLabel { labels, .. } = &mut op {
            if labels.is_empty() {
                *labels = crate::documents::tree::displaced(content);
            }
            labels.sort_unstable();
            labels.dedup();
        }
        if let Op::SangerUpdate { review } = &mut op {
            *review =
                self.retain_review_record(&crate::sanger_review::read_journal_record(review)?)?;
        }
        let typing = typing(&op) && self.group.is_none();
        let burst = self
            .typing
            .as_ref()
            .filter(|burst| {
                typing
                    && burst.doc == doc
                    && burst.actor == actor
                    && burst.last.elapsed() < TYPING_WINDOW
                    && self.docs[doc.0].undo.last().and_then(|a| a.group) == Some(burst.group)
                    && self.docs[doc.0]
                        .content
                        .as_document()
                        .merge(&burst.op, &op)
                        .is_some()
            })
            .map(|burst| burst.group);
        let group = match (burst, typing) {
            (Some(group), _) => Some(group),
            (None, true) => {
                self.next_group += 1;
                Some(self.next_group - 1)
            }
            (None, false) => self.group,
        };
        // A checkpoint describes the content an edit applies to, so it is taken
        // first. Otherwise apply first, so a refused edit starts no tracking.
        let slot = &self.docs[doc.0];
        // Before tracking takes the files on disk as known.
        let changed_on_disk = self.source_changed(doc);
        let checkpoints =
            self.tracked && (changed_on_disk || slot.baseline_logged && slot.baseline.is_some());
        if checkpoints {
            self.track(doc)?;
        }
        let alphabet = matches!(op, Op::SetAlphabet { .. }).then(|| self.docs[doc.0].is_clean());
        let before = crate::documents::tree::displaced(&self.docs[doc.0].content);
        let mut notes = crate::documents::tree::placement_notes(&self.docs[doc.0].content, &op);
        let (inverse, feature) = self.docs[doc.0]
            .content_mut()
            .as_document_mut()
            .apply(&op)?;
        let displaced = crate::documents::tree::newly_displaced(&self.docs[doc.0].content, &before);
        notes.extend(displaced.iter().map(|label| {
            format!(
                "Label {:?} no longer marks the group it was made for",
                label.label
            )
        }));
        if !checkpoints && let Err(error) = self.track(doc) {
            restore_inverse(self.docs[doc.0].content_mut(), &inverse)?;
            return Err(error);
        }
        self.docs[doc.0].generation += 1;
        let result = OpResult {
            notes: notes.clone(),
            hashes: Vec::new(),
            feature: feature.map(|f| f.0),
        };
        let entry = self.entry(doc, actor, op.clone(), result);
        let entry_id = entry.id.clone();
        if let Err(error) = self.write(&entry) {
            restore_inverse(self.docs[doc.0].content_mut(), &inverse)?;
            self.docs[doc.0].generation -= 1;
            return Err(error);
        }
        self.typing = typing.then(|| Typing {
            doc,
            actor: entry.actor.clone(),
            op: op.clone(),
            group: group.unwrap_or_default(),
            last: Instant::now(),
        });
        // Only an explicit group can span documents.
        if let Some(group) = self.group {
            self.group_docs.entry(group).or_default().insert(doc.0);
        }
        let slot = &mut self.docs[doc.0];
        let mut dropped = slot.clear_redo();
        slot.undo.push(Applied {
            entry_id: entry_id.clone(),
            op,
            inverse,
            group,
        });
        let settled = slot.settle();
        dropped.extend(self.settle_members(settled));
        self.forget_groups(dropped);
        if let Some(was_clean) = alphabet
            && let Some(note) = self.answer_written(doc, was_clean, changed_on_disk)
        {
            notes.push(note);
        }
        Ok(Outcome {
            entry_id,
            feature,
            notes,
            displaced,
        })
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
        self.step_group(id, false)
    }

    /// Redo the last undone step (one operation, or a whole group).
    pub fn redo(&mut self, id: DocId) -> Result<String> {
        self.step_group(id, true)
    }

    /// Undo or redo the top step of `id`, and the rest of its group in every
    /// document the group changed. Refused when any of them has a later step
    /// on top, so a rename never ends up half undone.
    fn step_group(&mut self, id: DocId, redo: bool) -> Result<String> {
        let slot = self.docs.get(id.0).ok_or(Error::UnknownDocument { id })?;
        let top = |slot: &super::Slot| {
            if redo { &slot.redo } else { &slot.undo }
                .last()
                .and_then(|a| a.group)
        };
        let Some(group) = top(slot) else {
            return self.step(id, !redo, true);
        };
        let mut members = vec![id.0];
        members.extend(
            self.group_docs
                .get(&group)
                .into_iter()
                .flatten()
                .filter(|&&d| d != id.0),
        );
        for &member in &members {
            if top(&self.docs[member]) != Some(group) {
                let file = &self.docs[member].key;
                return Err(invalid(if redo {
                    format!("{file} was edited after this step was undone, so it can't be redone")
                } else {
                    format!("undo the later edit in {file} first")
                }));
            }
        }
        // Each document's part of the group goes in one journal batch.
        let mut last = String::new();
        for member in members {
            let entry = self.step(DocId(member), !redo, true)?;
            if member == id.0 {
                last = entry;
            }
        }
        Ok(last)
    }

    /// Settle every member's part of groups one document settled, with the
    /// older steps beneath it, so no group stays half undoable. Returns
    /// every group settled.
    pub(super) fn settle_members(&mut self, mut pending: Vec<u64>) -> Vec<u64> {
        let mut settled = Vec::new();
        while let Some(group) = pending.pop() {
            settled.push(group);
            let members = self.group_docs.get(&group).cloned().unwrap_or_default();
            for member in members {
                let slot = &mut self.docs[member];
                if let Some(last) = slot.undo.iter().rposition(|a| a.group == Some(group)) {
                    pending.extend(slot.settle_oldest(last + 1));
                }
            }
        }
        settled
    }

    /// Forget the documents of groups no document can undo or redo any more.
    pub(super) fn forget_groups(&mut self, mut dropped: Vec<u64>) {
        dropped.sort_unstable();
        dropped.dedup();
        for group in dropped {
            let held = self.group_docs.get(&group).is_some_and(|members| {
                members.iter().any(|&member| self.docs[member].holds(group))
            });
            if !held {
                self.group_docs.remove(&group);
            }
        }
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
        self.step(id, true, false)
    }

    pub fn redo_single(&mut self, id: DocId) -> Result<String> {
        self.step(id, false, false)
    }

    /// Undo or redo one operation, or with `whole` its whole group. Every
    /// operation gets its own journal entry, written together in one batch,
    /// so a grouped step costs one journal write. If the write fails, the
    /// document returns to where it was.
    fn step(&mut self, id: DocId, undo: bool, whole: bool) -> Result<String> {
        self.docs.get(id.0).ok_or(Error::UnknownDocument { id })?;
        // Before tracking takes the files on disk as known.
        let changed_on_disk = self.source_changed(id);
        self.track(id)?;
        self.typing = None;
        let was_clean = self.docs[id.0].is_clean();
        let group = whole
            .then(|| self.docs[id.0].stack(undo).last().and_then(|a| a.group))
            .flatten();
        let mut entries = Vec::new();
        let mut undone = Vec::new();
        loop {
            let Some(applied) = self.docs[id.0].stack(undo).pop() else {
                if entries.is_empty() {
                    return Err(Error::NothingTo(if undo { "undo" } else { "redo" }));
                }
                break;
            };
            let flipped = if undo {
                self.docs[id.0]
                    .content_mut()
                    .as_document_mut()
                    .revert(&applied.inverse)
                    .map(|()| None)
            } else {
                self.reexecute(id, &applied).map(Some)
            };
            let flipped = match flipped {
                Ok(Some(inverse)) => Applied { inverse, ..applied },
                Ok(None) => applied,
                Err(error) => {
                    self.docs[id.0].stack(undo).push(applied);
                    self.unstep(id, undo, entries.len())?;
                    return Err(error);
                }
            };
            self.docs[id.0].generation += 1;
            let of = flipped.entry_id.clone();
            let op = if undo {
                Op::Undo { of: of.clone() }
            } else {
                Op::Redo { of: of.clone() }
            };
            entries.push(self.entry(id, self.actor.clone(), op, OpResult::default()));
            undone.push(of);
            self.docs[id.0].stack(!undo).push(flipped);
            if group.is_none() || self.docs[id.0].stack(undo).last().and_then(|a| a.group) != group
            {
                break;
            }
        }
        if let Err(error) = self.write_steps(id, &entries, &undone) {
            self.unstep(id, undo, entries.len())?;
            return Err(error);
        }
        let moved: Vec<bool> = (self.docs[id.0].stack(!undo).iter().rev())
            .take(undone.len())
            .map(|step| matches!(step.op, Op::SetAlphabet { .. }))
            .collect();
        // Only a step of alphabet answers alone leaves a saved document
        // saved; with other edits in the group, those still need a save.
        if moved.contains(&true) {
            self.answer_written(id, was_clean && !moved.contains(&false), changed_on_disk);
        }
        Ok(undone.pop().unwrap_or_default())
    }

    /// Take back the last `count` operations a failed [`Session::step`] moved.
    fn unstep(&mut self, id: DocId, undo: bool, count: usize) -> Result<()> {
        for _ in 0..count {
            let Some(applied) = self.docs[id.0].stack(!undo).pop() else {
                break;
            };
            let applied = if undo {
                Applied {
                    inverse: self.reexecute(id, &applied)?,
                    ..applied
                }
            } else {
                restore_inverse(self.docs[id.0].content_mut(), &applied.inverse)?;
                applied
            };
            self.docs[id.0].generation -= 1;
            self.docs[id.0].stack(undo).push(applied);
        }
        Ok(())
    }

    fn write_steps(&mut self, id: DocId, entries: &[LogEntry], undone: &[String]) -> Result<()> {
        let changed = self.source_changed(id);
        let boundary = self.docs[id.0].recovery_boundary.as_deref();
        if self.log.is_some()
            && (changed || boundary.is_some_and(|b| undone.iter().any(|of| of.as_str() <= b)))
        {
            if let Some(checkpoint) = self.recovery_checkpoint(id)? {
                let mut batch = entries.to_vec();
                batch.push(checkpoint.clone());
                self.write_entries(&batch)?;
                self.docs[id.0].recovery_boundary = Some(checkpoint.id);
            } else {
                self.write_entries(entries)?;
            }
            self.docs[id.0].baseline = None;
            self.refresh_source_stamps(id);
            if changed {
                self.note_changed_on_disk(id);
            }
        } else {
            self.write_entries(entries)?;
        }
        Ok(())
    }

    pub(super) fn reexecute(&mut self, id: DocId, applied: &Applied) -> Result<Inverse> {
        self.docs[id.0]
            .content_mut()
            .as_document_mut()
            .reapply(&applied.op, &applied.inverse)
    }
}

impl super::Slot {
    /// A new edit ends redo; returns the groups of the dropped steps.
    pub(super) fn clear_redo(&mut self) -> Vec<u64> {
        self.redo
            .drain(..)
            .filter_map(|applied| applied.group)
            .collect()
    }

    pub(super) fn holds(&self, group: u64) -> bool {
        self.undo
            .iter()
            .chain(&self.redo)
            .any(|applied| applied.group == Some(group))
    }
}

#[cfg(test)]
mod tests {
    use super::Splice;

    #[test]
    fn splice_keeps_only_the_changed_window_and_restores_it() {
        let before: Vec<u32> = (0..100).collect();
        let cases: Vec<Vec<u32>> = vec![
            before.clone(),
            [&before[..40], &[999], &before[40..]].concat(),
            [&before[..40], &before[41..]].concat(),
            [&before[..40], &[7], &before[41..]].concat(),
            before[..60].to_vec(),
            vec![],
        ];
        for (after, kept) in cases.into_iter().zip([0, 0, 1, 1, 40, 100]) {
            let splice = Splice::between(before.clone(), &after);
            assert_eq!(splice.before.len(), kept);
            let mut restored = after;
            splice.undo(&mut restored);
            assert_eq!(restored, before);
        }
    }
}
