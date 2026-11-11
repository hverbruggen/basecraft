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
            return Err(invalid("save document is not open"));
        }
        let mut warnings = Vec::new();
        let mut target_check = None;
        let new_sequence = self.docs[id.0].accepted_hashes.is_empty();
        if new_sequence && path.is_none() {
            return Err(invalid(
                "Choose a destination with Save as for this new sequence",
            ));
        }
        let path = path.filter(|p| !self.is_own_file(id, p));
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);

        let clean = (history_only || (self.log.is_some() && path.is_none()))
            && self.save_hashes(id)? == self.docs[id.0].saved_state_hashes;
        if history_only && !clean {
            return Err(invalid("history publication requires a saved document"));
        }
        if decision.is_none() && !history_only && self.log.is_some() && path.is_none() && clean {
            return Ok(SaveOutcome {
                recovery_observation: None,
                warnings,
            });
        }
        // Cooperating Basecraft processes serialize the entire data + sidecar +
        // provenance publication. External programs still require hash checks.

        let _save_lock = if self.tracked
            || matches!(
                self.write_publication,
                crate::documents::WritePublication::Recovery { .. }
            ) {
            Some(crate::save_guard::lock(&target)?)
        } else {
            None
        };
        if new_sequence || (mode == SaveMode::NewDestination && path.is_some()) {
            crate::save_guard::Revision::check(None, &target)?;
        }

        let _evidence_lock = crate::input_evidence::lock()?;
        if !history_only {
            let evidence = crate::input_evidence::check(&target)?;
            if decision.is_some()
                || !evidence.is_complete()
                || matches!(
                    self.write_publication,
                    crate::documents::WritePublication::Recovery { .. }
                )
            {
                let fresh = self.save_check_scope(id, &target, evidence)?;
                warnings = self.docs[id.0].save_authority.authorize_write(
                    &fresh,
                    decision.map(|decision| (decision, valid_consent)),
                    policy,
                    || {
                        // Changed original/sidecar acceptance is not waivable.
                        if path.is_none() {
                            self.check_save_source(id)?;
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
                crate::input_evidence::resolve(&evidence.conflict, policy)?;
            }
        }
        if history_only {
            self.check_save_source(id)?;
        }

        self.track(id)?;

        self.flush()?;
        let before = basecraft_io::hash_file(&target).ok();

        let expected = self.save_hashes_at(id, &target)?;
        if self.log.is_some() {
            let intent = serde_json::to_vec_pretty(&serde_json::to_value(
                basecraft_protocol::records::PendingSave {
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

            basecraft_io::write_atomic(&self.storage_root.join("pending-save.json"), |w| {
                w.write_all(&intent)
            })?;
        }
        // Preservation and pending-intent publication can take time. A decision
        // cannot authorize an externally replaced destination during that work.

        if let Some(check) = &target_check
            && !check.target_is_current(&target)?
        {
            return Err(Error::SaveDecisionStale(Box::new(
                self.check_save(id, Some(&target))?,
            )));
        }
        if new_sequence || (mode == SaveMode::NewDestination && path.is_some()) {
            crate::save_guard::Revision::check(None, &target)?;
        }
        let outcome = if history_only {
            self.log_save(id, &target, self.docs[id.0].accepted_hashes.clone())
        } else {
            self.save_inner(id, path)
        };
        if outcome.is_ok() {
            if target == self.resolve(&self.docs[id.0].key) {
                self.docs[id.0].saved_state_hashes = self.save_hashes_at(id, &target)?;

                self.docs[id.0].disk_stamp = crate::input_evidence::stamp(&target)?;
            }
            if self.log.is_some() {
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
                invalid(format!(
                    "data saved; provenance or sidecar incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ))
            } else {
                error
            }
        })?;
        let recovery_observation = if matches!(
            self.write_publication,
            crate::documents::WritePublication::Recovery { .. }
        ) {
            Some((|| -> Result<SaveCheck> {
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

    pub(super) fn check_save_source(&self, id: DocId) -> Result<()> {
        for key in &self.docs[id.0].absent_sidecars {
            if self.resolve(key).exists() {
                return Err(invalid(
                    "a source sidecar appeared since opening; reopen before saving",
                ));
            }
        }
        for (key, hash) in &self.docs[id.0].accepted_hashes {
            crate::save_guard::check_hash(&self.resolve(key), hash)?;
        }
        Ok(())
    }

    pub(super) fn save_inner(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.flush()?;
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        // An unchanged ordinary save neither rewrites data nor creates history.

        self.track(id)?;
        if target == self.resolve(&self.docs[id.0].key) {
            self.check_save_source(id)?;
        }

        let format = files::Format::from_path(&target)?;
        let original = self.resolve(&self.docs[id.0].key);
        let document = self.docs[id.0].content.as_document();
        let links = document.prepare_links(&original, &target, format)?;
        let write = || document.write(&target, format, &self.storage_root, &self.write_publication);
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
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Save { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );

        self.write(&entry)?;

        if target == self.resolve(&self.docs[id.0].key) {
            self.docs[id.0]
                .accepted_hashes
                .clone_from(&entry.result.hashes);
            self.docs[id.0].absent_sidecars.clear();
        }
        if self.tracked
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
        {
            self.publish_save(id, target, &entry, &[], None)
                .map_err(|e| {
                    invalid(format!(
                        "data saved; provenance incomplete: {e}. Recovery has been retained at {}",
                        self.storage_root.display()
                    ))
                })?;
        }
        Ok(())
    }

    /// Reserve an export/extract destination like a save target. Replacing an
    /// open document's own file would discard its accepted revision.
    /// Known-input conflicts are resolved first, under both locks.
    pub(super) fn derived_target(&self, path: &Path) -> Result<DerivedTarget> {
        let target = if self.tracked {
            Some(crate::save_guard::lock(path)?)
        } else {
            None
        };
        let evidence = crate::input_evidence::lock()?;
        crate::input_evidence::resolve(
            &crate::input_evidence::conflicts(path)?,
            crate::input_evidence::SavePolicy::Cancel,
        )?;
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
            _target: target,
            _evidence: evidence,
        })
    }

    /// Hashes the files a save would write, without writing them.
    pub fn save_hashes(&self, id: DocId) -> Result<Vec<String>> {
        let slot = &self.docs[id.0];
        if !slot.content.as_document().saved_hashes_are_content_only() {
            return self.save_hashes_at(id, Path::new(&slot.key));
        }
        if let Some(hashes) = slot.current_saved_hashes.get() {
            return Ok(hashes.clone());
        }
        let hashes = self.save_hashes_at(id, Path::new(&slot.key))?;
        // Concurrent readers may compute the same vector; a losing set is ordinary.
        let _ = slot.current_saved_hashes.set(hashes.clone());
        Ok(hashes)
    }

    /// Replay a save using its actual exchange format, including Save As.
    pub fn save_hashes_at(&self, id: DocId, target: &Path) -> Result<Vec<String>> {
        #[cfg(test)]
        super::saved_fingerprint_tests::attempt();
        self.docs[id.0].content.as_document().saved_hashes(
            &self.resolve(&target.to_string_lossy()),
            &self.write_publication,
        )
    }
}
