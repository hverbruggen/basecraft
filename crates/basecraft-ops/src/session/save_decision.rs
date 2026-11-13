//! Transient save decisions. No consent is written to disk or deserialised.
use std::path::{Path, PathBuf};

use super::save::SaveMode;
pub use super::save_consent::{EvidenceAction, SaveCheck, SaveDecision, SaveOutcome, SaveWarning};
use super::save_consent::{PendingSaveCheck, SaveScope, target_revision};
use super::{DocId, Session};
use crate::records::input_evidence::{EvidenceCheck, SavePolicy};
use crate::{Result, error::invalid};

impl Session {
    /// Advisory and side-effect-free. No lock is retained across caller UI.
    /// The authoritative save always repeats this inspection under its guard.
    pub fn check_save(&self, id: DocId, path: Option<&Path>) -> Result<SaveCheck> {
        if !self.is_open(id) {
            return Err(invalid("save document is not open"));
        }
        if self.docs[id.0].accepted_hashes.is_empty() && path.is_none() {
            return Err(invalid(
                "Choose a destination with Save as for this new sequence",
            ));
        }
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        let evidence = self.evidence_check(&target)?;
        let check = self.save_check_scope(id, &target, evidence, target_revision(&target)?)?;
        self.publish_save_check(id, check)
    }

    /// Bind a wire acknowledgement to the Session's one pending check for this
    /// document. This does not rescan. `save_with_decision`/`save_new_with_decision`
    /// consume its consent on entry and recheck under the normal guard.
    pub fn decide_save(
        &self,
        id: DocId,
        path: Option<&Path>,
        token: &str,
        known: SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<SaveDecision> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("save document is not open"))?;
        if let Some(decision) = slot
            .pending_check
            .decide(token, known, acknowledge_incomplete)?
        {
            return Ok(decision);
        }
        Err(crate::Error::SaveDecisionStale(Box::new(
            self.check_save(id, path)?,
        )))
    }

    pub(super) fn publish_save_check(&self, id: DocId, check: SaveCheck) -> Result<SaveCheck> {
        self.docs[id.0].pending_check.publish(check)
    }

    pub(super) fn consume_save_check(&self, decision: &SaveDecision) -> Result<bool> {
        // Resolve the origin, not the requested document, before validation/IO.
        let Some(origin) = PendingSaveCheck::origin(&self.id, decision) else {
            return Ok(false);
        };
        let Some(slot) = self.docs.get(origin) else {
            return Ok(false);
        };
        slot.pending_check.consume(decision)
    }

    pub(super) fn save_check_scope(
        &self,
        id: DocId,
        target: &Path,
        evidence: EvidenceCheck,
        target_revision: Option<super::save_consent::TargetRevision>,
    ) -> Result<SaveCheck> {
        let target = crate::records::save_guard::target_path(target)?;
        let publication = match &self.write_publication {
            crate::documents::WritePublication::Immediate
            | crate::documents::WritePublication::Create => None,
            crate::documents::WritePublication::Recovery { destination, .. } => Some((
                self.write_publication.logical(&target)?,
                crate::records::replay::directory_identity(
                    destination
                        .parent()
                        .ok_or_else(|| invalid("recovery parent missing"))?,
                )?,
            )),
        };
        let additional_evidence = publication
            .as_ref()
            .map(|(target, _)| self.evidence_check(target))
            .transpose()?
            .into_iter()
            .collect();
        let scope = SaveScope {
            evidence,
            additional_evidence,
            publication,
            finalization: false,
            session: self.id.clone(),
            document: id.0,
            document_key: self.docs[id.0].key.clone(),
            generation: self.generation(id)?,
            revision: self.revision(id)?,
            accepted_hashes: self.docs[id.0].accepted_hashes.clone(),
            target,
            target_revision,
        };
        // The digest includes all typed evidence and document/target context.
        // It is private consent, never reconstructed from a wire snapshot.
        SaveCheck::from_scope(scope)
    }

    /// Explicit one-attempt decision, including overwrite confirmation supplied
    /// by the caller. Adapters without overwrite confirmation use `save_new_with_decision`.
    pub fn save_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(id, path, decision.known, SaveMode::Replace, Some(decision))
    }

    /// The same decision boundary while retaining `save_new`'s no-overwrite rule.
    pub fn save_new_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(
            id,
            path,
            decision.known,
            SaveMode::NewDestination,
            Some(decision),
        )
    }
}

impl Session {
    pub(crate) fn recovery_placement(&mut self, stage: PathBuf, destination: PathBuf) {
        self.write_publication = crate::documents::WritePublication::Recovery {
            stage,
            destination,
            stores: std::sync::Mutex::new(Vec::new()),
        };
    }

    /// The analysis check for a save of `target` from this session.
    fn evidence_check(&self, target: &Path) -> Result<EvidenceCheck> {
        crate::records::input_evidence::check_for(
            target,
            self.project.as_ref().map(crate::project::Project::root),
        )
    }

    /// Check only, not consent. Called under the evidence guard
    /// immediately after a successful write or authorized finalization.
    pub(crate) fn recovery_snapshot(&self, id: DocId) -> Result<SaveCheck> {
        let target = self.resolve(&self.docs[id.0].key);
        let mut check = self.save_check_scope(
            id,
            &target,
            self.evidence_check(&target)?,
            target_revision(&target)?,
        )?;
        check.scope.finalization = true;
        check.scope_revision = check.scope.revision()?;
        Ok(check)
    }

    pub(crate) fn recovery_check(&self, id: DocId, saved: bool) -> Result<SaveCheck> {
        if saved {
            self.publish_save_check(id, self.recovery_snapshot(id)?)
        } else {
            self.check_save(id, Some(&self.resolve(&self.docs[id.0].key)))
        }
    }

    /// Caller holds the pinned final target reservation, then evidence guard.
    /// Reauthorizes only publication: never serializes a successful document again.
    pub(crate) fn recovery_finalize(
        &self,
        id: DocId,
        expected: Option<&SaveCheck>,
        decision: Option<SaveDecision>,
    ) -> Result<SaveCheck> {
        let valid = decision
            .as_ref()
            .map(|d| self.consume_save_check(d))
            .transpose()?
            .unwrap_or(false);
        self.check_save_source(id, None)?;
        let fresh = self.recovery_snapshot(id)?;
        let decided = decision.is_some();
        self.docs[id.0].pending_check.authorize_finalization(
            &fresh,
            expected,
            decision.map(|decision| (decision, valid)),
            Some(&self.evidence_actions),
        )?;
        if decided {
            let mut observed = self.recovery_snapshot(id)?;
            if observed.issues() != fresh.issues()
                || observed
                    .evidence_groups()
                    .any(|group| !group.conflict.analyses.is_empty())
            {
                observed.require_final_decision();
            }
            Ok(observed)
        } else {
            Ok(fresh)
        }
    }

    pub(crate) fn recovery_stores(
        &self,
    ) -> Result<Vec<crate::records::analysis_store::ArtifactRun>> {
        match &self.write_publication {
            crate::documents::WritePublication::Immediate
            | crate::documents::WritePublication::Create => Err(invalid("not a recovery Session")),
            crate::documents::WritePublication::Recovery { stores, .. } => Ok(stores
                .lock()
                .map_err(|_| invalid("recovery stores unavailable"))?
                .clone()),
        }
    }
}

impl Session {
    pub(crate) fn save_recovery(&mut self, id: DocId) -> Result<SaveOutcome> {
        let target = self.resolve(&self.docs[id.0].key);
        self.save_attempt(
            id,
            Some(&target),
            SavePolicy::Cancel,
            SaveMode::Replace,
            None,
        )
    }
}

impl Session {
    pub(crate) fn recovery_actions(&self) -> Result<Vec<EvidenceAction>> {
        Ok(self
            .evidence_actions
            .lock()
            .map_err(|_| invalid("save action state unavailable"))?
            .clone())
    }
    pub(crate) fn discard_recovery_decision(&self, decision: &SaveDecision) -> Result<()> {
        self.consume_save_check(decision).map(|_| ())
    }
}

#[cfg(test)]
use crate::isolated_profile as isolated_identity_profile;

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_platform::files::{ObjectIdentity, OrdinaryKind, object_identity};

    fn recovery_session(root: &Path) -> (Session, DocId, PathBuf, PathBuf) {
        let stage = root.join("stage");
        let parent = root.join("parent");
        std::fs::create_dir(&stage).unwrap();
        std::fs::create_dir(&parent).unwrap();
        let target = stage.join("a.fasta");
        std::fs::write(&target, b">gene\nACGT\n").unwrap();
        let mut session = Session::unlogged(&stage);
        let doc = session.open(&target).unwrap();
        session
            .apply(
                doc,
                basecraft_protocol::Op::SeqReplace {
                    start: 1,
                    end: 2,
                    residues: "T".into(),
                },
            )
            .unwrap();
        session.recovery_placement(stage, parent.join("out"));
        (session, doc, target, parent)
    }

    #[test]
    fn actual_json_scope_digest_retains_all_identity_bits_and_variant() {
        let Some(root) = isolated_identity_profile::enter(
            "session::save_decision::tests::actual_json_scope_digest_retains_all_identity_bits_and_variant",
        ) else {
            return;
        };
        let (session, doc, _, parent) = recovery_session(&root);
        let mut baseline = session.recovery_check(doc, false).unwrap();
        assert_eq!(
            baseline.scope.publication.as_ref().unwrap().1,
            object_identity(&parent, OrdinaryKind::Directory).unwrap()
        );
        assert_eq!(baseline.scope_revision, baseline.scope.revision().unwrap());
        // Synthetic Windows values exercise the very same typed SaveScope JSON
        // digest used by production, even when this test runs on Unix.
        baseline.scope.publication.as_mut().unwrap().1 = ObjectIdentity::Windows {
            volume_serial: 0,
            file_id: [0; 16],
        };
        baseline.scope_revision = baseline.scope.revision().unwrap();
        let mut digests = std::collections::BTreeSet::new();
        digests.insert(baseline.scope_revision.clone());
        for bit in 0..64 {
            let mut changed = baseline.clone();
            changed.scope.publication.as_mut().unwrap().1 = ObjectIdentity::Windows {
                volume_serial: 1_u64 << bit,
                file_id: [0; 16],
            };
            changed.scope_revision = changed.scope.revision().unwrap();
            let wire = serde_json::to_value(&changed).unwrap();
            assert_eq!(
                wire["publication"][1]["volume_serial"].as_u64(),
                Some(1_u64 << bit)
            );
            assert_eq!(wire["publication"][1]["platform"], "windows");
            assert!(!changed.same_scope(&baseline), "volume bit {bit}");
            assert!(digests.insert(changed.scope_revision), "volume bit {bit}");
        }
        for bit in 0..128 {
            let mut bytes = [0; 16];
            bytes[bit / 8] = 1 << (bit % 8);
            let mut changed = baseline.clone();
            changed.scope.publication.as_mut().unwrap().1 = ObjectIdentity::Windows {
                volume_serial: 0,
                file_id: bytes,
            };
            changed.scope_revision = changed.scope.revision().unwrap();
            let wire = serde_json::to_value(&changed).unwrap();
            assert_eq!(wire["publication"][1]["file_id"], serde_json::json!(bytes));
            assert!(!changed.same_scope(&baseline), "file ID bit {bit}");
            assert!(digests.insert(changed.scope_revision), "file ID bit {bit}");
        }
        let mut unix = baseline.clone();
        unix.scope.publication.as_mut().unwrap().1 = ObjectIdentity::Unix {
            device: 0,
            inode: 0,
        };
        unix.scope_revision = unix.scope.revision().unwrap();
        assert_eq!(
            serde_json::to_value(&unix).unwrap()["publication"][1]["platform"],
            "unix"
        );
        assert!(!unix.same_scope(&baseline));
        assert!(digests.insert(unix.scope_revision));
        assert_eq!(digests.len(), 194);
    }

    #[test]
    fn changed_full_parent_identity_spends_receipt_and_returns_stale_scope() {
        let Some(root) = isolated_identity_profile::enter(
            "session::save_decision::tests::changed_full_parent_identity_spends_receipt_and_returns_stale_scope",
        ) else {
            return;
        };
        let (mut session, doc, target, parent) = recovery_session(&root);
        let check = session.recovery_check(doc, false).unwrap();
        let old_identity = check.scope.publication.as_ref().unwrap().1.clone();
        // Retain the old object so this does not assume IDs cannot be recycled.
        std::fs::rename(&parent, root.join("old-parent")).unwrap();
        std::fs::create_dir(&parent).unwrap();
        let new_identity = object_identity(&parent, OrdinaryKind::Directory).unwrap();
        assert_ne!(old_identity, new_identity);
        let error = session
            .save_with_decision(
                doc,
                Some(&target),
                check.clone().decide(SavePolicy::Cancel, false),
            )
            .unwrap_err();
        let crate::Error::SaveDecisionStale(fresh) = error else {
            panic!("expected stale receipt")
        };
        assert_eq!(fresh.scope.publication.as_ref().unwrap().1, new_identity);
        assert!(!fresh.same_scope(&check));
        assert!(
            !session
                .consume_save_check(&check.decide(SavePolicy::Cancel, false))
                .unwrap()
        );
        assert_eq!(std::fs::read(target).unwrap(), b">gene\nACGT\n");
        // This tests the lower Session boundary only: RecoveryAttempt separately
        // rejects the changed pinned parent, rather than adopting fresh identity.
    }
}
