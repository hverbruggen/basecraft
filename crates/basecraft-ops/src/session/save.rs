//! Shared save guards, writes, hashes and acceptance.
use super::SaveCheck;
use super::{DerivedTarget, DocId, Error, Op, OpResult, Path, Result, files, invalid};
use super::{SaveDecision, SaveOutcome, Session};

/// The three supported save intents; history publication never writes document data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveMode {
    Replace,
    NewDestination,
    History,
}

impl Session {
    /// Save a document to its own path, or to `path`.
    pub fn save(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.save_with_evidence(id, path, crate::input_evidence::SavePolicy::Cancel)
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

    /// Headless callers choose a policy explicitly; default save returns a structured conflict.
    pub fn save_with_evidence(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        policy: crate::input_evidence::SavePolicy,
    ) -> Result<()> {
        self.save_attempt(id, path, policy, SaveMode::Replace, None)
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
        policy: crate::input_evidence::SavePolicy,
        mode: SaveMode,
        decision: Option<SaveDecision>,
    ) -> Result<SaveOutcome> {
        let history_only = mode == SaveMode::History;
        let valid_consent = decision
            .as_ref()
            .map(|decision| self.consume_save_check(decision))
            .transpose()?
            .unwrap_or(false);

        if !self.is_open(id) {
            return Err(Error::UnknownDocument { id });
        }
        if let Some(message) = self.content(id)?.as_document().save_refusal() {
            return Err(Error::Unsupported(message.into()));
        }
        let mut warnings = Vec::new();
        let mut target_check = None;
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
        let path = path.filter(|p| !self.is_own_file(id, p));
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        let clean =
            (history_only || (self.log.is_some() && path.is_none())) && self.docs[id.0].is_clean();
        if history_only && !clean {
            return Err(invalid("history publication requires a saved document"));
        }
        if decision.is_none()
            && !history_only
            && self.log.is_some()
            && path.is_none()
            && clean
            && !self.storage_root.join("pending-save.json").exists()
        {
            return Ok(SaveOutcome {
                recovery_observation: None,
                warnings,
            });
        }
        if (new_sequence || (mode == SaveMode::NewDestination && path.is_some()))
            && !self.check_previous_save_target(id, &target)?
        {
            crate::save_guard::check_new_target(&target)?;
        }

        let mut evidence_lock = crate::input_evidence::lock_save(&target)?;
        let evidence = evidence_lock.check(&target)?;
        if !history_only
            && decision.is_none()
            && evidence.is_complete()
            && policy == crate::input_evidence::SavePolicy::Cancel
        {
            crate::input_evidence::resolve(&evidence.conflict, policy)?;
        }
        let mut source_stamps = Vec::new();
        for source in std::iter::once(target.clone()).chain(
            path.is_none()
                .then_some(&self.docs[id.0])
                .into_iter()
                .flat_map(|slot| {
                    slot.accepted_hashes
                        .iter()
                        .map(|(key, _)| self.resolve(key))
                        .chain(slot.absent_sidecars.iter().map(|key| self.resolve(key)))
                }),
        ) {
            source_stamps.push((source.clone(), crate::save_guard::change_stamp(&source)?));
        }
        let revision = super::save_authority::target_revision(&target)?;
        let before = revision.as_ref().map(|r| r.hash.clone());
        let checked_target = before.as_deref().map(|hash| (target.as_path(), hash));
        if !history_only {
            if decision.is_some()
                || !evidence.is_complete()
                || matches!(
                    self.write_publication,
                    crate::documents::WritePublication::Recovery { .. }
                )
            {
                let fresh = self.save_check_scope(id, &target, evidence, revision)?;
                warnings = self.docs[id.0].save_authority.authorize_write(
                    &fresh,
                    decision.map(|decision| (decision, valid_consent)),
                    policy,
                    || {
                        // Changed original/sidecar acceptance is not waivable.
                        if path.is_none() {
                            self.check_save_source(id, checked_target)?;
                        }
                        Ok(())
                    },
                    matches!(
                        self.write_publication,
                        crate::documents::WritePublication::Recovery { .. }
                    )
                    .then_some(&self.evidence_actions),
                )?;
                target_check = Some(fresh);
            } else {
                if path.is_none() {
                    self.check_save_source(id, checked_target)?;
                }
                crate::input_evidence::resolve(&evidence.conflict, policy)?;
            }
        }
        if history_only {
            self.check_save_source(id, checked_target)?;
        }

        self.track(id)?;
        // Retaining the original by hard link changes only its change time.
        for (source, stamp) in &mut source_stamps {
            let now = crate::save_guard::change_stamp(source)?;
            if now.as_ref().map(|now| &now.0) != stamp.as_ref().map(|stamp| &stamp.0) {
                return Err(Error::ChangedOnDisk {
                    path: source.clone(),
                });
            }
            *stamp = now;
        }

        self.flush()?;
        let mut prepared = if history_only {
            None
        } else {
            Some(self.docs[id.0].content.as_document().prepare_write(
                &target,
                self.save_format(id, &target)?,
                &self.storage_root,
                &self.write_publication,
            )?)
        };
        let expected: Vec<_> = prepared.as_ref().map_or_else(
            || {
                self.docs[id.0]
                    .accepted_hashes
                    .iter()
                    .map(|(_, hash)| hash.clone())
                    .collect()
            },
            |p| p.hashes().into_iter().map(|(_, hash)| hash).collect(),
        );
        if self.log.is_some() {
            let document = self.docs[id.0].content.as_document();
            let mut paths = document.independently_saved_files(&target);
            if paths.is_empty() {
                paths.push(target.clone());
                paths.extend(
                    document
                        .saved_companion(&target)
                        .filter(|_| expected.len() > 1),
                );
            }
            let hashes = paths
                .iter()
                .zip(&expected)
                .map(|(path, hash)| (self.relative(path), hash.clone()))
                .collect();
            let baseline = self.entry(
                id,
                self.actor.clone(),
                Op::Save {
                    path: self.relative(&target),
                },
                OpResult {
                    hashes,
                    feature: None,
                },
            );
            let intent = serde_json::to_vec_pretty(&serde_json::to_value(
                basecraft_protocol::records::PendingSave {
                    baseline,
                    schema_version: 1,
                    session: self.id.clone(),
                    target: target.clone(),
                    before_hash: before.clone(),
                    expected_hashes: expected.clone(),
                    status: basecraft_protocol::records::RecoveryStatus::Pending,
                    note: "data and provenance are separate writes".into(),
                    extra: std::collections::BTreeMap::new(),
                },
            )?)?;

            std::fs::create_dir_all(&self.storage_root)?;

            let path = self.storage_root.join("pending-save.json");
            if let Some(prepared) = &mut prepared {
                prepared.pending(&path, &intent)?;
            } else {
                basecraft_io::write_atomic(&path, |w| w.write_all(&intent))?;
            }
        }
        // Preservation and pending-intent publication can take time. A decision
        // cannot authorize an externally replaced destination during that work.
        if review {
            self.check_target_identity(id, &target)?;
        }
        if let Some(check) = &target_check
            && !check.target_is_current(&target)?
        {
            return Err(Error::SaveDecisionStale(Box::new(
                self.check_save(id, Some(&target))?,
            )));
        }
        for (source, stamp) in source_stamps {
            if crate::save_guard::change_stamp(&source)? != stamp {
                return Err(Error::ChangedOnDisk { path: source });
            }
        }
        if (new_sequence || (mode == SaveMode::NewDestination && path.is_some()))
            && !self.check_previous_save_target(id, &target)?
        {
            crate::save_guard::check_new_target(&target)?;
        }
        #[cfg(not(unix))]
        {
            // Without change time, stamps cannot detect a same-size rewrite
            // whose mtime was restored. Recheck content on these platforms.
            if path.is_none() {
                self.check_save_source(id, None)?;
            }
            if let Some(hash) = &before {
                crate::save_guard::check_hash(&target, hash)?;
            }
        }
        let outcome = if history_only {
            self.log_save(id, &target, self.docs[id.0].accepted_hashes.clone())
        } else {
            self.save_inner(id, path, prepared.unwrap(), before.is_none())
        };
        if outcome.is_ok() {
            // A review is clean after any completed save, Save As included.
            if review {
                self.docs[id.0].saved_at = Some(self.docs[id.0].mark());
            }
            self.docs[id.0].saved_target = Some((
                target.clone(),
                crate::input_evidence::stamp(&target)?,
                expected[0].clone(),
            ));
            if target == self.resolve(&self.docs[id.0].key) {
                self.docs[id.0].saved_at = Some(self.docs[id.0].mark());
                self.docs[id.0].disk_stamp = crate::input_evidence::stamp(&target)?;
            }
            if self.log.is_some() {
                #[cfg(test)]
                super::save_failure_tests::fail_write(
                    super::save_failure_tests::WriteStep::RemoveIntent,
                )?;
                std::fs::remove_file(self.storage_root.join("pending-save.json"))?;
            }
        }
        outcome.map_err(|error| {
            if target == self.resolve(&self.docs[id.0].key) {
                // Tables are replaced one by one before the manifest. Accept
                // those already written, so a retry isn't refused as a
                // conflicting outside change.
                let independent = self.docs[id.0]
                    .content
                    .as_document()
                    .independently_saved_files(&target);
                for (path, hash) in independent.iter().zip(&expected) {
                    if basecraft_io::hash_file(path).ok().as_ref() == Some(hash) {
                        let key = self.relative(path);
                        let slot = &mut self.docs[id.0];
                        slot.absent_sidecars.retain(|p| *p != key);
                        slot.accepted_hashes.retain(|(p, _)| *p != key);
                        slot.accepted_hashes.push((key, hash.clone()));
                    }
                }
            }
            let now = basecraft_io::hash_file(&target).ok();
            if now.as_ref() == expected.first()
                && let Ok(stamp) = crate::input_evidence::stamp(&target)
            {
                if self.is_own_file(id, &target) {
                    self.docs[id.0].disk_stamp = stamp.clone();
                }
                self.docs[id.0].saved_target = Some((target.clone(), stamp, expected[0].clone()));
            }
            if now != before && now.as_ref() == expected.first() {
                // Our main-file replacement completed. Accept precisely that
                // generation for retry, while retaining old sidecar checks.
                if target == self.resolve(&self.docs[id.0].key) {
                    let target_key = self.relative(&target);
                    for (key, hash) in &mut self.docs[id.0].accepted_hashes {
                        if *key == target_key {
                            hash.clone_from(&expected[0]);
                        }
                    }
                    if let Some((expected_sidecar, sidecar)) = expected.get(1).zip(
                        self.docs[id.0]
                            .content
                            .as_document()
                            .saved_companion(&target),
                    ) && basecraft_io::hash_file(&sidecar).ok().as_ref()
                        == Some(expected_sidecar)
                    {
                        let key = self.relative(&sidecar);
                        self.docs[id.0].absent_sidecars.retain(|p| *p != key);
                        self.docs[id.0].accepted_hashes.retain(|(p, _)| *p != key);
                        self.docs[id.0]
                            .accepted_hashes
                            .push((key, expected_sidecar.clone()));
                    }
                }
                Error::Damaged { path: target.clone(), detail: format!(
                    "data saved; provenance or sidecar incomplete: {error}. Recovery remains at {}", self.storage_root.display()) }
            } else {
                error
            }
        })?;
        let recovery_observation = if matches!(
            self.write_publication,
            crate::documents::WritePublication::Recovery { .. }
        ) {
            Some((|| -> Result<SaveCheck> {
                #[cfg(test)]
                super::save_failure_tests::fail_write(
                    super::save_failure_tests::WriteStep::RecoveryCheck,
                )?;
                let mut observed = self.recovery_snapshot(id)?;
                if target_check
                    .as_ref()
                    .is_some_and(|before| before.issues() != observed.issues())
                    || observed
                        .evidence_groups()
                        .any(|group| !group.conflict.analyses.is_empty())
                {
                    observed.require_final_decision();
                }

                Ok(observed)
            })())
        } else {
            None
        };
        Ok(SaveOutcome {
            recovery_observation,
            warnings,
        })
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
        self.flush()?;
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
        #[cfg(test)]
        super::save_failure_tests::fail_write(super::save_failure_tests::WriteStep::Journal)?;
        self.write(&entry)?;
        self.docs[id.0].recovery_boundary = Some(entry.id.clone());
        self.docs[id.0].baseline = Some(entry.clone());
        self.docs[id.0].baseline_logged = true;
        self.docs[id.0].source_stamps = entry
            .source
            .as_ref()
            .expect("save source")
            .files
            .iter()
            .map(|file| (self.resolve(&file.path), Some(file.stamp.clone())))
            .collect();
        self.docs[id.0].recovery_changed = false;
        if target == self.resolve(&self.docs[id.0].key) {
            self.docs[id.0]
                .accepted_hashes
                .clone_from(&entry.result.hashes);
            self.docs[id.0].disk_stamp = entry.source.as_ref().expect("save source").files[0]
                .stamp
                .clone();
            self.docs[id.0].absent_sidecars.clear();
        }
        if self.tracked
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
        {
            self.publish_save(id, target, &entry, &[], None)
                .map_err(|error| Error::Damaged {
                    path: target.into(),
                    detail: format!(
                        "data saved; history incomplete: {error}. Recovery remains at {}",
                        self.storage_root.display()
                    ),
                })?;
        }
        Ok(())
    }

    /// Reserve an export/extract destination like a save target. Replacing an
    /// open document's own file would discard its accepted revision.
    /// Known-input conflicts are resolved first, under both locks.
    pub(super) fn derived_target(&self, path: &Path) -> Result<DerivedTarget> {
        let mut evidence = crate::input_evidence::lock_save(path)?;
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
