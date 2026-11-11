//! Shared save guards, writes, hashes and acceptance.
use super::SaveCheck;
use super::{DerivedTarget, DocId, Error, Op, OpResult, Path, Result, files, invalid};
use super::{SaveDecision, SaveOutcome, Session};

use super::SaveWarning;
use crate::input_evidence::SavePolicy;

type SourceStamps = Vec<(std::path::PathBuf, Option<crate::save_guard::ChangeStamp>)>;

/// Where a save writes and what it may assume about that file.
struct SaveTarget {
    path: std::path::PathBuf,
    /// Set for Save As to another file than the document's own.
    save_as: Option<std::path::PathBuf>,
    /// A new sequence or a no-overwrite Save As: the file must not exist.
    must_be_absent: bool,
    review: bool,
}

/// Retaining the original by hard link changes only its change time.
fn restamp_after_retention(mut stamps: SourceStamps) -> Result<SourceStamps> {
    for (source, stamp) in &mut stamps {
        let now = crate::save_guard::change_stamp(source)?;
        if now.as_ref().map(|now| &now.0) != stamp.as_ref().map(|stamp| &stamp.0) {
            return Err(Error::ChangedOnDisk {
                path: source.clone(),
            });
        }
        *stamp = now;
    }
    Ok(stamps)
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
            crate::input_evidence::SavePolicy::Cancel,
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
            crate::input_evidence::SavePolicy::Cancel,
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
            crate::input_evidence::SavePolicy::Cancel,
            SaveMode::NewDestination,
            None,
        )
        .map(|_| ())
    }

    /// Any spelling of the document's own file is an ordinary save, so its
    /// accepted-revision checks apply.
    pub(super) fn is_own_file(&self, id: DocId, path: &Path) -> bool {
        crate::save_guard::same_target(path, &self.resolve(&self.docs[id.0].key))
    }

    pub(super) fn save_attempt(
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
        let valid_consent = decision
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
        let written = Self::written_stores(&target.path)?;
        let mut evidence_lock =
            crate::input_evidence::lock_save(&target.path, policy != SavePolicy::Cancel, &written)?;
        let evidence = evidence_lock.check(&target.path)?;
        let (stamps, revision) = self.observe_sources(id, &target)?;
        let before = revision.as_ref().map(|r| r.hash.clone());

        // 2. Decide: known analyses and unchecked stores need consent.
        let (warnings, consented) = self.consent(
            id,
            &target,
            evidence,
            revision,
            decision.map(|decision| (decision, valid_consent)),
            policy,
            recovery,
        )?;

        // 3. Stage every file and the recovery intent; nothing is replaced yet.
        self.track(id)?;
        let stamps = restamp_after_retention(stamps)?;
        let mut prepared = self.docs[id.0].content.as_document().prepare_write(
            &target.path,
            self.save_format(id, &target.path)?,
            &self.storage_root,
            &self.write_publication,
        )?;
        let expected: Vec<_> = prepared
            .hashes()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect();
        self.stage_pending_intent(id, &target.path, before.as_ref(), &expected, &mut prepared)?;

        // 4. Nothing changed while staging, then replace and record.
        self.recheck_before_write(id, &target, consented.as_ref(), stamps, before.as_ref())?;
        let outcome = self.save_inner(id, target.save_as.as_deref(), prepared, before.is_none());
        self.finish_save(
            id,
            &target.path,
            &expected,
            before.as_ref(),
            target.review,
            outcome,
        )?;

        let recovery_check = recovery.then(|| self.final_recovery_check(id, consented.as_ref()));
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
        let _target = crate::save_guard::lock(&target)?;
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
            crate::save_guard::check_new_target(&target.path)?;
        }
        Ok(())
    }

    /// Stamps of the target and accepted companions, and the target's content.
    fn observe_sources(
        &self,
        id: DocId,
        target: &SaveTarget,
    ) -> Result<(SourceStamps, Option<super::save_consent::TargetRevision>)> {
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
            let stamp = crate::save_guard::change_stamp(&source)?;
            stamps.push((source, stamp));
        }
        Ok((stamps, super::save_consent::target_revision(&target.path)?))
    }

    /// Returns the warnings to report and, when a check was bound, the check
    /// the write must still match.
    #[allow(clippy::too_many_arguments)]
    fn consent(
        &mut self,
        id: DocId,
        target: &SaveTarget,
        evidence: crate::input_evidence::EvidenceCheck,
        revision: Option<super::save_consent::TargetRevision>,
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
            crate::input_evidence::resolve(&evidence.conflict, policy)?;
            return Ok((Vec::new(), None));
        }
        let check = self.save_check_scope(id, &target.path, evidence, revision)?;
        let actions = recovery.then_some(&self.evidence_actions);
        let warnings = self.docs[id.0].pending_check.consent(
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
            paths.extend(
                document
                    .saved_companion(target)
                    .filter(|_| expected.len() > 1),
            );
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
                hashes,
                feature: None,
            },
        );
        let intent = serde_json::to_vec_pretty(&basecraft_protocol::records::PendingSave {
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
        consented: Option<&SaveCheck>,
        stamps: SourceStamps,
        before: Option<&String>,
    ) -> Result<()> {
        if target.review {
            self.check_target_identity(id, &target.path)?;
        }
        if let Some(check) = consented
            && !check.target_is_current(&target.path)?
        {
            return Err(Error::SaveDecisionStale(Box::new(
                self.check_save(id, Some(&target.path))?,
            )));
        }
        for (source, stamp) in stamps {
            if crate::save_guard::change_stamp(&source)? != stamp {
                return Err(Error::ChangedOnDisk { path: source });
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
                crate::save_guard::check_hash(&target.path, hash)?;
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
            self.docs[id.0].saved_target = Some((
                target.to_path_buf(),
                crate::input_evidence::stamp(target)?,
                expected[0].clone(),
            ));
            if own {
                self.docs[id.0].disk_stamp = crate::input_evidence::stamp(target)?;
            }
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
            if let Some((hash, sidecar)) = expected.get(1).zip(document.saved_companion(target))
                && basecraft_io::hash_file(&sidecar).ok().as_ref() == Some(hash)
            {
                accepted.push((self.relative(&sidecar), hash.clone()));
            }
        }
        let slot = &mut self.docs[id.0];
        for (key, hash) in accepted {
            slot.absent_sidecars.retain(|p| *p != key);
            slot.accepted_hashes.retain(|(p, _)| *p != key);
            slot.accepted_hashes.push((key, hash));
        }
        if main_written && let Ok(stamp) = crate::input_evidence::stamp(target) {
            if own {
                self.docs[id.0].disk_stamp = stamp.clone();
            }
            self.docs[id.0].saved_target = Some((target.to_path_buf(), stamp, expected[0].clone()));
        }
        if main_written && now.as_ref() != before {
            return Err(Error::Damaged {
                path: target.into(),
                detail: format!(
                    "data saved; provenance or sidecar incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ),
            });
        }
        Err(error)
    }

    /// A recovery write ends with a fresh check of its final destination.
    fn final_recovery_check(
        &mut self,
        id: DocId,
        consented: Option<&SaveCheck>,
    ) -> Result<SaveCheck> {
        #[cfg(test)]
        super::save_failure_tests::fail_write(super::save_failure_tests::WriteStep::RecoveryCheck)?;
        let mut check = self.recovery_snapshot(id)?;
        if consented.is_some_and(|before| before.issues() != check.issues())
            || check
                .evidence_groups()
                .any(|group| !group.conflict.analyses.is_empty())
        {
            check.require_final_decision();
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
                return Err(Error::ChangedOnDisk {
                    path: self.resolve(key),
                });
            }
        }
        for (key, hash) in &self.docs[id.0].accepted_hashes {
            let source = self.resolve(key);
            if let Some((_, actual)) =
                checked.filter(|(path, _)| crate::save_guard::same_target(path, &source))
            {
                if actual != hash {
                    return Err(Error::ChangedOnDisk { path: source });
                }
            } else {
                crate::save_guard::check_hash(&source, hash)?;
            }
        }
        Ok(())
    }

    pub(super) fn save_inner(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        prepared: crate::documents::PreparedWrite,
        create: bool,
    ) -> Result<()> {
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        // An unchanged ordinary save neither rewrites data nor creates history.

        self.track(id)?;
        let format = self.save_format(id, &target)?;
        let original = self.resolve(&self.docs[id.0].key);
        let document = self.docs[id.0].content.as_document();
        let links = document.prepare_links(&original, &target, format)?;
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
        let write = || document.publish_write(prepared, &target, publication);
        let (hashes, outputs) = if let Some(links) = links {
            links.publish(write)?
        } else {
            (write()?, Vec::new())
        };
        let hashes = hashes
            .into_iter()
            .map(|(path, hash)| (self.relative(&path), hash))
            .collect();
        self.log_save(id, &target, hashes)?;
        if !outputs.is_empty() {
            self.log_tool_run(
                id,
                self.actor.clone(),
                ("basecraft.links", "copy"),
                serde_json::json!({"from": original, "to": target}),
                &[],
                &outputs,
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
                    stamp: crate::input_evidence::stamp(&self.resolve(path))?,
                    hash: hash.clone(),
                    extra: std::collections::BTreeMap::default(),
                })
            })
            .collect::<Result<_>>()?;
        // History is staged first so its files sync alongside the journal
        // append. None becomes visible until the Save record is durable.
        let history = (self.tracked
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None)
            .then(|| self.prepare_save_history(id, target, &entry, &[], None));
        let (prepared, mut history_error) = match history {
            Some(Ok(prepared)) => (prepared, None),
            Some(Err(error)) => (crate::documents::PreparedWrite::default(), Some(error)),
            None => (crate::documents::PreparedWrite::default(), None),
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
        if let Some(error) = history_error {
            return Err(Error::Damaged {
                path: target.into(),
                detail: format!(
                    "data saved; history incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ),
            });
        }
        Ok(())
    }

    /// Reserve an export/extract destination like a save target. Replacing an
    /// open document's own file would discard its accepted revision.
    /// Known-input conflicts are resolved first, under both locks.
    pub(super) fn derived_target(&self, path: &Path) -> Result<DerivedTarget> {
        let mut evidence = crate::input_evidence::lock_save(path, false, &[])?;
        let check = evidence.check(path)?;
        if !check.is_complete() {
            return Err(Error::EvidenceCheckIncomplete(Box::new(check)));
        }
        crate::input_evidence::resolve(&check.conflict, crate::input_evidence::SavePolicy::Cancel)?;
        if self
            .docs
            .iter()
            .any(|slot| crate::save_guard::same_target(path, &self.resolve(&slot.key)))
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
        self.docs[id.0].recovery_changed = false;
        if target == self.resolve(&self.docs[id.0].key) {
            self.docs[id.0]
                .accepted_hashes
                .clone_from(&entry.result.hashes);
            self.docs[id.0].disk_stamp = files[0].stamp.clone();
            self.docs[id.0].absent_sidecars.clear();
        }
    }

    /// Stores this save may write after the data: the store of a bound result
    /// (binding, history) and the target folder's store (new hidden runs).
    /// They are reserved exclusively up front, never upgraded later.
    fn written_stores(target: &Path) -> Result<Vec<std::path::PathBuf>> {
        let mut stores: Vec<_> = crate::analysis_store::ArtifactRun::for_result(target)?
            .and_then(|run| run.parent().and_then(Path::parent).map(Path::to_path_buf))
            .into_iter()
            .collect();
        if let Some(folder) = target.parent() {
            let store = folder.join(".basecraft/analyses-v1");
            if store.is_dir() {
                stores.push(store);
            }
        }
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
        crate::save_guard::reject_link_target(target)?;
        let slot = &self.docs[id.0];
        let expected = slot
            .saved_target
            .as_ref()
            .filter(|(path, _, _)| crate::save_guard::same_target(path, target))
            .map(|(_, stamp, _)| stamp)
            .or_else(|| {
                (!slot.accepted_hashes.is_empty() && self.is_own_file(id, target))
                    .then_some(&slot.disk_stamp)
            });
        if let Some(expected) = expected
            && crate::input_evidence::stamp(target)? != *expected
        {
            return Err(Error::ChangedOnDisk {
                path: target.into(),
            });
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
            index != id.0 && crate::save_guard::same_target(&self.resolve(&slot.key), target)
        }) {
            return Ok(false);
        }
        let Some((_, _, hash)) = self.docs[id.0]
            .saved_target
            .as_ref()
            .filter(|(path, _, _)| crate::save_guard::same_target(path, target))
        else {
            return Ok(false);
        };
        crate::save_guard::check_hash(target, hash)?;
        Ok(true)
    }
}
