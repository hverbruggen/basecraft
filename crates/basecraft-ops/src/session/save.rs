//! Shared save guards, writes, hashes and acceptance.
use super::SaveCheck;
use super::{DerivedTarget, DocId, Error, Op, OpResult, Path, Result, files, invalid};
use super::{SaveDecision, SaveOutcome, Session};
use crate::error::{changed_on_disk, damaged};

use super::SaveWarning;
use crate::records::run_inputs::SavePolicy;

type SourceStamps = Vec<(
    std::path::PathBuf,
    Option<crate::records::save_guard::ChangeStamp>,
)>;

/// Where a save writes and what it may assume about that file.
struct SaveTarget {
    path: std::path::PathBuf,
    /// Set for Save As to another file than the document's own.
    save_as: Option<std::path::PathBuf>,
    /// A new sequence or a no-overwrite Save As: the file must not exist.
    must_be_absent: bool,
    review: bool,
}

/// The three supported save intents; history publication never writes document data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveMode {
    Replace,
    NewDestination,
    History,
}

impl Session {
    /// Save a document to its own path, or to `path`. Known analyses or an
    /// incomplete check return a typed result; `save_with_decision` continues.
    pub fn save(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.save_attempt(
            id,
            path,
            crate::records::run_inputs::SavePolicy::Cancel,
            SaveMode::Replace,
            None,
        )
        .map(|_| ())
    }

    /// Persist newly recorded history for an already saved document without
    /// rewriting its data or sidecars. Uses the ordinary guarded save publisher.
    pub fn save_history(&mut self, id: DocId) -> Result<()> {
        self.save_attempt(
            id,
            None,
            crate::records::run_inputs::SavePolicy::Cancel,
            SaveMode::History,
            None,
        )
        .map(|_| ())
    }

    /// Save to the document's own file, or Save As to a destination that must
    /// not exist yet. For callers without an overwrite confirmation (agents,
    /// scripts): an existing other file is refused, never replaced.
    pub fn save_new(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.save_attempt(
            id,
            path,
            crate::records::run_inputs::SavePolicy::Cancel,
            SaveMode::NewDestination,
            None,
        )
        .map(|_| ())
    }

    /// Whether `id` has edits its own file lacks. A new document has no file
    /// of its own until it is saved to one, so it has none.
    pub fn needs_save(&self, id: DocId) -> Result<bool> {
        let slot = self.slot(id)?;
        Ok(slot.saved_at.is_some() && !slot.is_clean())
    }

    /// Let the next save of `id` drop fields a later basecraft wrote into
    /// the files it replaces, which it otherwise refuses (D14).
    pub fn drop_newer_fields(&mut self, id: DocId) -> Result<()> {
        self.docs
            .get_mut(id.0)
            .ok_or(Error::UnknownDocument { id })?
            .drop_newer_fields = true;
        Ok(())
    }

    /// Any spelling of the document's own file is an ordinary save, so its
    /// accepted-revision checks apply.
    pub(super) fn is_own_file(&self, id: DocId, path: &Path) -> bool {
        crate::records::save_guard::same_target(path, &self.resolve(&self.docs[id.0].key))
    }

    pub(super) fn save_attempt(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        policy: SavePolicy,
        mode: SaveMode,
        decision: Option<SaveDecision>,
    ) -> Result<SaveOutcome> {
        let saved = self.save_checked(id, path, policy, mode, decision);
        if saved.is_err() {
            self.checkpoint_if_original_changed(id)?;
        }
        saved
    }

    /// A save refused after another program changed the original leaves the
    /// unsaved edits recoverable only from a checkpoint of the live document.
    fn checkpoint_if_original_changed(&mut self, id: DocId) -> Result<()> {
        if self.log.is_none()
            // A baseline still held back means nothing was edited yet.
            || self.docs.get(id.0).is_none_or(|slot| slot.baseline.is_some())
            || !self.source_changed(id)
        {
            return Ok(());
        }
        if let Some(entry) = self.recovery_checkpoint(id)? {
            self.docs[id.0].recovery_boundary = Some(entry.id.clone());
            self.write_entries(&[entry])?;
        }
        Ok(())
    }

    fn save_checked(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        policy: SavePolicy,
        mode: SaveMode,
        decision: Option<SaveDecision>,
    ) -> Result<SaveOutcome> {
        if mode == SaveMode::History {
            return self
                .save_history_record(id)
                .map(|()| SaveOutcome::default());
        }
        // The permission to drop newer fields covers this attempt only,
        // whether or not it gets as far as writing.
        let drop_newer_fields = self
            .docs
            .get_mut(id.0)
            .is_some_and(|slot| std::mem::take(&mut slot.drop_newer_fields));
        let answer_is_current = decision
            .as_ref()
            .map(|decision| self.consume_save_check(decision))
            .transpose()?
            .unwrap_or(false);
        let target = self.save_target(id, path, mode)?;
        if decision.is_none() && self.nothing_to_save(id, &target) {
            return Ok(SaveOutcome::default());
        }
        let recovery = matches!(
            self.write_publication,
            crate::documents::WritePublication::Recovery { .. }
        );

        // 1. Find and reserve the analyses that may use the target.
        let written = self.written_stores(id, &target.path)?;
        let mut evidence_lock = crate::records::run_inputs::lock_save(
            &target.path,
            &written,
            self.project.as_ref().map(crate::project::Project::root),
        )?;
        let evidence = evidence_lock.check(&target.path)?;
        let (stamps, revision) = self.observe_sources(id, &target)?;
        let before = revision.as_ref().map(|r| r.hash.clone());

        // 2. Decide: known analyses and unchecked stores need an answer.
        let (warnings, answered) = self.apply_answer(
            id,
            &target,
            evidence,
            revision,
            decision.map(|decision| (decision, answer_is_current)),
            policy,
            recovery,
        )?;

        // 3. Stage every file and the recovery intent; nothing is replaced yet.
        self.track(id)?;
        let format = self.save_format(id, &target.path)?;
        let mut prepared = self.docs[id.0].content.as_document().prepare_write(
            self.companion_project(),
            &target.path,
            format,
            &self.storage_root,
            &self.write_publication,
        )?;
        if !prepared.unknown.is_empty() && !drop_newer_fields {
            return Err(Error::NewerFieldsDropped(std::mem::take(
                &mut prepared.unknown,
            )));
        }
        let expected: Vec<_> = prepared
            .hashes()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect();
        self.stage_pending_intent(id, &target.path, before.as_ref(), &expected, &mut prepared)?;

        // 4. Nothing changed while staging, then replace and record.
        self.recheck_before_write(id, &target, answered.as_ref(), stamps, before.as_ref())?;
        let outcome = self.save_inner(id, &target.path, format, prepared, before.is_none());
        self.finish_save(
            id,
            &target.path,
            &expected,
            before.as_ref(),
            target.review,
            outcome,
        )?;

        let recovery_check = recovery.then(|| self.final_recovery_check(id, answered.as_ref()));
        Ok(SaveOutcome {
            recovery_check,
            warnings,
        })
    }

    /// Record history for an already saved document. No document data is
    /// written, so no analysis store is consulted.
    fn save_history_record(&mut self, id: DocId) -> Result<()> {
        if !self.is_open(id) {
            return Err(Error::UnknownDocument { id });
        }
        if !self.docs[id.0].is_clean() {
            return Err(invalid("history publication requires a saved document"));
        }
        let target = self.resolve(&self.docs[id.0].key);
        let _target = crate::records::save_guard::lock(&target)?;
        self.check_save_source(id, None)?;
        self.track(id)?;
        // The intent lets a repair finish a history publication that failed.
        let accepted = self.docs[id.0].accepted_hashes.clone();
        let expected: Vec<_> = accepted.iter().map(|(_, hash)| hash.clone()).collect();
        let mut intent = crate::documents::PreparedWrite::default();
        self.stage_pending_intent(id, &target, expected.first(), &expected, &mut intent)?;
        intent.publish()?;
        self.log_save(id, &target, accepted)?;
        if self.log.is_some() {
            std::fs::remove_file(self.storage_root.join("pending-save.json"))?;
        }
        Ok(())
    }

    fn save_target(&self, id: DocId, path: Option<&Path>, mode: SaveMode) -> Result<SaveTarget> {
        if !self.is_open(id) {
            return Err(Error::UnknownDocument { id });
        }
        let own_target = path.is_none_or(|path| self.is_own_file(id, path));
        if let Some(message) = self.content(id)?.as_document().save_refusal(own_target) {
            return Err(Error::Unsupported(message));
        }
        let new_sequence = self.docs[id.0].accepted_hashes.is_empty();
        if new_sequence && path.is_none() {
            return Err(invalid(
                "Choose a destination with Save as for this new sequence",
            ));
        }
        // Reviews keep the link and identity guards their former payload owned.
        let review = self.docs[id.0].content.kind() == files::Kind::SangerReview;
        if review {
            let requested =
                path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
            self.check_target_identity(id, &requested)?;
        }
        let save_as = path
            .filter(|p| !self.is_own_file(id, p))
            .map(Path::to_path_buf);
        let path = save_as
            .clone()
            .unwrap_or_else(|| self.resolve(&self.docs[id.0].key));
        let must_be_absent =
            new_sequence || (mode == SaveMode::NewDestination && save_as.is_some());
        let target = SaveTarget {
            path,
            save_as,
            must_be_absent,
            review,
        };
        self.check_absent_target(id, &target)?;
        Ok(target)
    }

    /// An unchanged own file with no interrupted save needs no write.
    fn nothing_to_save(&self, id: DocId, target: &SaveTarget) -> bool {
        self.log.is_some()
            && target.save_as.is_none()
            && self.docs[id.0].is_clean()
            && !self.storage_root.join("pending-save.json").exists()
    }

    fn check_absent_target(&self, id: DocId, target: &SaveTarget) -> Result<()> {
        if target.must_be_absent && !self.check_previous_save_target(id, &target.path)? {
            crate::records::save_guard::check_new_target(&target.path)?;
        }
        Ok(())
    }

    /// Stamps of the target and accepted companions, and the target's content.
    fn observe_sources(
        &self,
        id: DocId,
        target: &SaveTarget,
    ) -> Result<(SourceStamps, Option<super::save_answer::TargetRevision>)> {
        let slot = &self.docs[id.0];
        let companions = target
            .save_as
            .is_none()
            .then_some(slot)
            .into_iter()
            .flat_map(|slot| {
                slot.accepted_hashes
                    .iter()
                    .map(|(key, _)| self.resolve(key))
                    .chain(slot.absent_sidecars.iter().map(|key| self.resolve(key)))
            });
        let mut stamps = Vec::new();
        for source in std::iter::once(target.path.clone()).chain(companions) {
            let stamp = crate::records::save_guard::change_stamp(&source)?;
            stamps.push((source, stamp));
        }
        Ok((stamps, super::save_answer::target_revision(&target.path)?))
    }

    /// Returns the warnings to report and, when a check was bound, the check
    /// the write must still match.
    #[allow(clippy::too_many_arguments)]
    fn apply_answer(
        &mut self,
        id: DocId,
        target: &SaveTarget,
        evidence: crate::records::run_inputs::InputCheck,
        revision: Option<super::save_answer::TargetRevision>,
        decision: Option<(SaveDecision, bool)>,
        policy: SavePolicy,
        recovery: bool,
    ) -> Result<(Vec<SaveWarning>, Option<SaveCheck>)> {
        let checked_target = revision
            .as_ref()
            .map(|r| (target.path.clone(), r.hash.clone()));
        let own_file = target.save_as.is_none();
        let check_source = |session: &Self| -> Result<()> {
            if own_file {
                session.check_save_source(
                    id,
                    checked_target
                        .as_ref()
                        .map(|(p, h)| (p.as_path(), h.as_str())),
                )?;
            }
            Ok(())
        };
        // A new file holds no recorded bytes: unchecked stores only warn.
        if decision.is_none()
            && !recovery
            && revision.is_none()
            && evidence.conflict.analyses.is_empty()
            && !evidence.is_complete()
        {
            return Ok((
                vec![SaveWarning::EvidenceNotChecked {
                    issues: evidence.issues,
                }],
                None,
            ));
        }
        if decision.is_none() && !recovery && evidence.is_complete() {
            check_source(self)?;
            crate::records::run_inputs::resolve(&evidence.conflict, policy)?;
            return Ok((Vec::new(), None));
        }
        let check = self.save_check_scope(id, &target.path, evidence, revision)?;
        let actions = recovery.then_some(&self.evidence_actions);
        let warnings = self.docs[id.0].pending_check.apply_answer(
            &check,
            decision,
            policy,
            || check_source(self),
            actions,
        )?;
        Ok((warnings, Some(check)))
    }

    fn stage_pending_intent(
        &self,
        id: DocId,
        target: &Path,
        before: Option<&String>,
        expected: &[String],
        prepared: &mut crate::documents::PreparedWrite,
    ) -> Result<()> {
        if self.log.is_none() {
            return Ok(());
        }
        let document = self.docs[id.0].content.as_document();
        let mut paths = document.independently_saved_files(target);
        if paths.is_empty() {
            paths.push(target.to_path_buf());
            paths.extend(document.saved_companions(self.companion_project(), target));
        }
        let hashes = paths
            .iter()
            .zip(expected)
            .map(|(path, hash)| (self.relative(path), hash.clone()))
            .collect();
        let baseline = self.entry(
            id,
            self.actor.clone(),
            Op::Save {
                path: self.relative(target),
            },
            OpResult {
                notes: Vec::new(),
                hashes,
                feature: None,
            },
        );
        let intent =
            basecraft_protocol::records::encode(&basecraft_protocol::records::PendingSave {
                baseline,
                schema_version: 1,
                session: self.id.clone(),
                target: target.to_path_buf(),
                before_hash: before.cloned(),
                expected_hashes: expected.to_vec(),
                status: basecraft_protocol::records::RecoveryStatus::Pending,
                note: "data and provenance are separate writes".into(),
                extra: std::collections::BTreeMap::new(),
            })?;
        std::fs::create_dir_all(&self.storage_root)?;
        prepared.pending(&self.storage_root.join("pending-save.json"), &intent)
    }

    /// Staging took time. Refuse if the target or a companion changed, or if a
    /// bound decision no longer matches the target.
    fn recheck_before_write(
        &self,
        id: DocId,
        target: &SaveTarget,
        answered: Option<&SaveCheck>,
        stamps: SourceStamps,
        before: Option<&String>,
    ) -> Result<()> {
        if target.review {
            self.check_target_identity(id, &target.path)?;
        }
        if let Some(check) = answered
            && !check.target_is_current(&target.path)?
        {
            return Err(Error::SaveDecisionStale(Box::new(
                self.check_save(id, Some(&target.path))?,
            )));
        }
        for (source, stamp) in stamps {
            if crate::records::save_guard::change_stamp(&source)? != stamp {
                return Err(changed_on_disk(source));
            }
        }
        self.check_absent_target(id, target)?;
        #[cfg(not(unix))]
        {
            // Without change time, stamps cannot detect a same-size rewrite
            // whose mtime was restored. Recheck content on these platforms.
            if target.save_as.is_none() {
                self.check_save_source(id, None)?;
            }
            if let Some(hash) = before {
                crate::records::save_guard::check_hash(&target.path, hash)?;
            }
        }
        #[cfg(unix)]
        let _ = before;
        Ok(())
    }

    /// Accept what was written. After a partial failure, accept the files that
    /// match, so a retry is not refused as an outside change.
    fn finish_save(
        &mut self,
        id: DocId,
        target: &Path,
        expected: &[String],
        before: Option<&String>,
        review: bool,
        outcome: Result<()>,
    ) -> Result<()> {
        let own = *target == self.resolve(&self.docs[id.0].key);
        if outcome.is_ok() {
            // A review is clean after any completed save, Save As included.
            if review || own {
                self.docs[id.0].saved_at = Some(self.docs[id.0].mark());
            }
            let stamp = crate::records::run_inputs::stamp(target)?;
            if own {
                self.docs[id.0].disk_stamp = stamp.clone();
                self.wrote_gff(id, target)?;
            }
            self.docs[id.0].saved_target = Some((target.to_path_buf(), stamp, expected[0].clone()));
            if self.log.is_some() {
                #[cfg(test)]
                super::save_failure_tests::fail_write(
                    super::save_failure_tests::WriteStep::RemoveIntent,
                )?;
                std::fs::remove_file(self.storage_root.join("pending-save.json"))?;
            }
            return Ok(());
        }
        let error = outcome.unwrap_err();
        let document = self.docs[id.0].content.as_document();
        let mut accepted = Vec::new();
        if own {
            for (path, hash) in document
                .independently_saved_files(target)
                .iter()
                .zip(expected)
            {
                if basecraft_io::hash_file(path).ok().as_ref() == Some(hash) {
                    accepted.push((self.relative(path), hash.clone()));
                }
            }
        }
        let now = basecraft_io::hash_file(target).ok();
        let main_written = now.as_ref() == expected.first();
        if main_written && own {
            accepted.push((self.relative(target), expected[0].clone()));
            for (hash, sidecar) in expected
                .iter()
                .skip(1)
                .zip(document.saved_companions(self.companion_project(), target))
            {
                if basecraft_io::hash_file(&sidecar).ok().as_ref() == Some(hash) {
                    accepted.push((self.relative(&sidecar), hash.clone()));
                }
            }
        }
        let slot = &mut self.docs[id.0];
        for (key, hash) in accepted {
            slot.absent_sidecars.retain(|p| *p != key);
            slot.accepted_hashes.retain(|(p, _)| *p != key);
            slot.accepted_hashes.push((key, hash));
        }
        if main_written && let Ok(stamp) = crate::records::run_inputs::stamp(target) {
            if own {
                self.docs[id.0].disk_stamp = stamp.clone();
            }
            self.docs[id.0].saved_target = Some((target.to_path_buf(), stamp, expected[0].clone()));
        }
        if main_written && now.as_ref() != before {
            return Err(damaged(
                target,
                format!(
                    "data saved; provenance or sidecar incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ),
            ));
        }
        Err(error)
    }

    /// A GFF3 this save wrote for the document's own file is from now on
    /// one that was read with it (D27): later saves write it again.
    fn wrote_gff(&mut self, id: DocId, target: &Path) -> Result<()> {
        if let files::Loaded::Sequence(opened) = self.docs[id.0].content_mut()
            && opened.others.gff.is_none()
            && files::writes_gff(opened)
        {
            let bytes = std::fs::read(files::gff_sidecar(target))?;
            let name = opened.doc.name.clone();
            opened.others.gff = Some(std::sync::Arc::new(basecraft_io::gff3::keep_others(
                &bytes, &name,
            )));
        }
        Ok(())
    }

    /// A recovery write ends with a fresh check of its final destination.
    fn final_recovery_check(
        &mut self,
        id: DocId,
        answered: Option<&SaveCheck>,
    ) -> Result<SaveCheck> {
        #[cfg(test)]
        super::save_failure_tests::fail_write(super::save_failure_tests::WriteStep::RecoveryCheck)?;
        let mut check = self.recovery_snapshot(id)?;
        if answered.is_some_and(|before| before.issues() != check.issues())
            || check
                .evidence_groups()
                .any(|group| !group.conflict.analyses.is_empty())
        {
            check.require_new_answer();
        }
        Ok(check)
    }

    pub(super) fn check_save_source(
        &self,
        id: DocId,
        checked: Option<(&Path, &str)>,
    ) -> Result<()> {
        for key in &self.docs[id.0].absent_sidecars {
            if self.resolve(key).exists() {
                return Err(changed_on_disk(self.resolve(key)));
            }
        }
        for (key, hash) in &self.docs[id.0].accepted_hashes {
            let source = self.resolve(key);
            if let Some((_, actual)) =
                checked.filter(|(path, _)| crate::records::save_guard::same_target(path, &source))
            {
                if actual != hash {
                    return Err(changed_on_disk(source));
                }
            } else {
                crate::records::save_guard::check_hash(&source, hash)?;
            }
        }
        Ok(())
    }

    /// Replace the files staged in `prepared` and record the save. The
    /// caller has tracked the document and checked the target.
    fn save_inner(
        &mut self,
        id: DocId,
        target: &Path,
        format: files::Format,
        prepared: crate::documents::PreparedWrite,
        create: bool,
    ) -> Result<()> {
        let original = self.resolve(&self.docs[id.0].key);
        let document = self.docs[id.0].content.as_document();
        let links = document.prepare_links(self.companion_project(), &original, target, format)?;
        let publication = if create
            && matches!(
                self.write_publication,
                crate::documents::WritePublication::Immediate
            ) {
            &crate::documents::WritePublication::Create
        } else {
            &self.write_publication
        };
        #[cfg(test)]
        super::save_failure_tests::fail_write(super::save_failure_tests::WriteStep::Replace)?;
        let project = self.companion_project();
        let new_run = prepared
            .run
            .as_ref()
            .map(|run| run.directory().to_path_buf());
        let write = || document.publish_write(project, prepared, target, publication);
        let (hashes, outputs) = if let Some(links) = links {
            links.publish(write)?
        } else {
            (write()?, Vec::new())
        };
        if new_run.is_some() && original == target {
            // The save bound the document's own file to a new run, in every
            // slot that has it open.
            let key = self.docs[id.0].key.clone();
            for slot in self.docs.iter_mut().filter(|slot| slot.key == key) {
                slot.run.clone_from(&new_run);
            }
        }
        let hashes = hashes
            .into_iter()
            .map(|(path, hash)| (self.relative(&path), hash))
            .collect();
        self.log_save(id, target, hashes)?;
        if !outputs.is_empty() {
            let outputs = crate::session::ToolRunRecord::hashed(outputs)?;
            self.record_tool_run(
                id,
                self.actor.clone(),
                crate::session::ToolRunRecord::new(
                    ("basecraft.links", "copy"),
                    serde_json::json!({"from": original, "to": target}),
                    &[],
                    &outputs,
                ),
            )?;
        }
        Ok(())
    }

    pub(super) fn log_save(
        &mut self,
        id: DocId,
        target: &Path,
        hashes: Vec<(String, String)>,
    ) -> Result<()> {
        let key = self.relative(target);
        let mut entry = self.entry(
            id,
            self.actor.clone(),
            Op::Save { path: key },
            OpResult {
                notes: Vec::new(),
                hashes,
                feature: None,
            },
        );
        entry.source.as_mut().expect("save source").files = entry
            .result
            .hashes
            .iter()
            .map(|(path, hash)| {
                Ok(basecraft_protocol::provenance::InputReference {
                    path: path.clone(),
                    stamp: crate::records::run_inputs::stamp(&self.resolve(path))?,
                    hash: hash.clone(),
                    extra: std::collections::BTreeMap::default(),
                })
            })
            .collect::<Result<_>>()?;
        // History is staged first so its files sync alongside the journal
        // append. None becomes visible until the Save record is durable.
        let history = self.tracked.then(|| {
            self.history(id)
                .and_then(|history| history.save(target, &entry))
        });
        let (prepared, effective, mut history_error) = match history {
            Some(Ok(staged)) => (staged.write, Some(staged.effective), None),
            Some(Err(error)) => (
                crate::documents::PreparedWrite::default(),
                None,
                Some(error),
            ),
            None => (crate::documents::PreparedWrite::default(), None, None),
        };
        let mut journal_written = false;
        let published = prepared.publish_with(|| {
            #[cfg(test)]
            super::save_failure_tests::fail_write(super::save_failure_tests::WriteStep::Journal)?;
            self.write(&entry)?;
            journal_written = true;
            self.accept_logged_save(id, target, &entry);
            Ok(())
        });
        match published {
            Err(error) if !journal_written => return Err(error),
            Err(error) => history_error = Some(error),
            Ok(_) => {}
        }
        if history_error.is_none()
            && let Some(effective) = effective
        {
            self.recorded_save(id, target, &entry, effective);
        }
        if let Some(error) = history_error {
            return Err(damaged(
                target,
                format!(
                    "data saved; history incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ),
            ));
        }
        Ok(())
    }

    /// Reserve an export/extract destination like a save target. Replacing an
    /// open document's own file would discard its accepted revision.
    /// Known-input conflicts are resolved first, under both locks.
    pub(super) fn derived_target(&self, path: &Path) -> Result<DerivedTarget> {
        let mut evidence = crate::records::run_inputs::lock_save(
            path,
            &[],
            self.project.as_ref().map(crate::project::Project::root),
        )?;
        let check = evidence.check(path)?;
        if !check.is_complete() {
            return Err(Error::InputCheckIncomplete(Box::new(check)));
        }
        crate::records::run_inputs::resolve(
            &check.conflict,
            crate::records::run_inputs::SavePolicy::Cancel,
        )?;
        if self
            .docs
            .iter()
            .any(|slot| crate::records::save_guard::same_target(path, &self.resolve(&slot.key)))
        {
            return Err(invalid(
                "this destination is an open document; choose another file",
            ));
        }
        Ok(DerivedTarget {
            _evidence: evidence,
        })
    }

    /// Hashes the files a save would write, without writing them.
    pub fn save_hashes(&self, id: DocId) -> Result<Vec<String>> {
        let slot = &self.docs[id.0];
        self.save_hashes_at(id, Path::new(&slot.key))
    }

    /// Replay a save using its actual exchange format, including Save As.
    pub fn save_hashes_at(&self, id: DocId, target: &Path) -> Result<Vec<String>> {
        self.docs[id.0].content.as_document().saved_hashes(
            &self.resolve(&target.to_string_lossy()),
            &self.write_publication,
        )
    }
}

impl Session {
    /// A save's history record is published. For the document's own file its
    /// events move from the unsaved list to the recorded one, and the
    /// recovery journal keeps only what is still unsaved (D24).
    fn recorded_save(
        &mut self,
        id: DocId,
        target: &Path,
        entry: &basecraft_protocol::LogEntry,
        effective: Vec<String>,
    ) {
        if target != self.resolve(&self.docs[id.0].key) {
            return;
        }
        let slot = &mut self.docs[id.0];
        slot.last_record = Some((entry.id.clone(), effective));
        let saved = std::mem::take(&mut slot.unsaved);
        slot.recorded.extend(saved);
        let key = slot.key.clone();
        let save = entry.id.clone();
        // The same file open twice shares a key: the other slot's unsaved
        // edits stay in the journal.
        let shared = self.docs.iter().filter(|slot| slot.key == key).count() > 1;
        if self.storage_root != self.root
            && !shared
            && let Some(log) = &mut self.log
        {
            // The Save entry is the document's recovery baseline from now on.
            // The save is done either way: a journal that could not be cut
            // back still recovers the same edits, it is only longer.
            let mut before_save = true;
            let _ = log.keep(|kept| {
                if kept.id == save {
                    before_save = false;
                }
                kept.doc != key || !before_save
            });
        }
    }

    /// The Save record is durable: it becomes the recovery baseline and, for the
    /// document's own file, the accepted revision.
    fn accept_logged_save(
        &mut self,
        id: DocId,
        target: &Path,
        entry: &basecraft_protocol::LogEntry,
    ) {
        let files = &entry.source.as_ref().expect("save source").files;
        self.docs[id.0].recovery_boundary = Some(entry.id.clone());
        self.docs[id.0].baseline = Some(entry.clone());
        self.docs[id.0].baseline_logged = true;
        self.docs[id.0].source_stamps = files
            .iter()
            .map(|file| (self.resolve(&file.path), Some(file.stamp.clone())))
            .collect();
        self.docs[id.0].recovery_notice = None;
        if target == self.resolve(&self.docs[id.0].key) {
            self.docs[id.0]
                .accepted_hashes
                .clone_from(&entry.result.hashes);
            self.docs[id.0].disk_stamp = files[0].stamp.clone();
            self.docs[id.0].absent_sidecars.clear();
        }
    }

    /// The stores this save may write after the data: the one that holds the
    /// target's binding and any run the save makes, in the project or beside
    /// a loose file (rule 2.5), and that of the run the document's file is
    /// bound to, which takes its history. Reserved exclusively up front,
    /// never upgraded later.
    fn written_stores(&self, id: DocId, target: &Path) -> Result<Vec<std::path::PathBuf>> {
        let base = crate::project::Project::base_of(self.companion_project(), target)?;
        let mut stores = vec![base.join(".basecraft/analyses-v1")];
        let run = self.docs[id.0].run.as_deref();
        stores.extend(run.and_then(|run| Some(run.parent()?.parent()?.to_path_buf())));
        stores.dedup();
        stores.retain(|store| store.is_dir());
        Ok(stores)
    }

    fn save_format(&self, id: DocId, target: &Path) -> Result<files::Format> {
        if self.docs[id.0].content.kind() == files::Kind::SangerReview {
            Ok(files::Format::SangerReview)
        } else {
            files::Format::from_path(target)
        }
    }

    fn check_target_identity(&self, id: DocId, target: &Path) -> Result<()> {
        crate::records::save_guard::reject_link_target(target)?;
        let slot = &self.docs[id.0];
        let expected = slot
            .saved_target
            .as_ref()
            .filter(|(path, _, _)| crate::records::save_guard::same_target(path, target))
            .map(|(_, stamp, _)| stamp)
            .or_else(|| {
                (!slot.accepted_hashes.is_empty() && self.is_own_file(id, target))
                    .then_some(&slot.disk_stamp)
            });
        if let Some(expected) = expected
            && crate::records::run_inputs::stamp(target)? != *expected
        {
            return Err(changed_on_disk(target));
        }
        Ok(())
    }

    // The latest Save As target remains owned by this session without changing the
    // journal's stable document key or discarding its undo stack.
    pub(super) fn check_previous_save_target(
        &self,
        id: DocId,
        target: &std::path::Path,
    ) -> Result<bool> {
        if self.docs.iter().enumerate().any(|(index, slot)| {
            index != id.0
                && crate::records::save_guard::same_target(&self.resolve(&slot.key), target)
        }) {
            return Ok(false);
        }
        let Some((_, _, hash)) = self.docs[id.0]
            .saved_target
            .as_ref()
            .filter(|(path, _, _)| crate::records::save_guard::same_target(path, target))
        else {
            return Ok(false);
        };
        crate::records::save_guard::check_hash(target, hash)?;
        Ok(true)
    }
}
