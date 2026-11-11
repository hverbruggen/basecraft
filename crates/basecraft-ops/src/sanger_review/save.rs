//! Sanger participation in the shared one-attempt save decision.
use super::{Change, SangerReviewSession, storage::ReviewEvidence};
use crate::session::save_authority::{
    PendingSaveAuthority, SaveCheck, SaveDecision, SaveScope, target_revision,
};
use crate::{Result, error::invalid, input_evidence::SavePolicy};
use basecraft_docs::sanger::{ReviewReference, ReviewSettings, SangerDraft};
use basecraft_protocol::sanger::{MaskSettings, ReviewChoice, ReviewOp};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) struct RootAuthority {
    origin: String,
    // None permanently closes this owner's advisory route on exhaustion/failure.
    epoch: Option<u64>,
    pending: PendingSaveAuthority,
}

impl RootAuthority {
    pub(super) fn new() -> Self {
        Self {
            origin: ulid::Ulid::generate().to_string(),
            epoch: Some(0),
            pending: PendingSaveAuthority::default(),
        }
    }
}

impl Clone for SangerReviewSession {
    fn clone(&self) -> Self {
        let mut copy = self.clone_without_authority();
        copy.save_authority = Some(RootAuthority::new());
        copy
    }
}

impl SangerReviewSession {
    fn clone_without_authority(&self) -> Self {
        Self {
            save_authority: None,
            retained: self
                .retained
                .iter()
                .map(Self::clone_without_authority)
                .collect(),
            draft: self.draft.clone(),
            draft_path: self.draft_path.clone(),
            draft_sha256: self.draft_sha256.clone(),
            draft_json: self.draft_json.clone(),
            decisions: self.decisions.clone(),
            mask: self.mask.clone(),
            settings: self.settings.clone(),
            reference: self.reference.clone(),
            manual: self.manual.clone(),
            events: self.events.clone(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
            evidence: self.evidence.clone(),
            anchor: self.anchor.clone(),
            recovery: self.recovery.clone(),
            accepted: self.accepted.clone(),
            stored_extra: self.stored_extra.clone(),
        }
    }

    pub(super) fn demote_save_authority(&mut self) {
        self.save_authority = None;
        for child in &mut self.retained {
            child.demote_save_authority();
        }
    }

    pub(super) fn invalidate_save_authority(&mut self) {
        if let Some(owner) = &mut self.save_authority {
            owner.epoch = owner.epoch.and_then(|epoch| epoch.checked_add(1));
            if owner.pending.invalidate().is_err() {
                owner.epoch = None;
            }
        }
    }

    fn authority(&self) -> Result<(&RootAuthority, u64)> {
        let owner = self
            .save_authority
            .as_ref()
            .ok_or_else(|| invalid("inactive Sanger call set has no save authority"))?;
        let epoch = owner
            .epoch
            .ok_or_else(|| invalid("Sanger save authority is unavailable"))?;
        Ok((owner, epoch))
    }

    fn save_scope(&self, path: &Path) -> Result<SaveCheck> {
        self.save_scope_for_state(path, &self.state_digest()?)
    }

    fn save_scope_for_state(&self, path: &Path, state: &str) -> Result<SaveCheck> {
        let (owner, epoch) = self.authority()?;
        crate::save_guard::Revision::check(self.accepted.as_ref(), path)?;
        let target = crate::save_guard::target_path(path)?;
        let scope = SaveScope {
            evidence: crate::input_evidence::check(&target)?,
            additional_evidence: Vec::new(),
            publication: None,
            finalization: false,
            session: owner.origin.clone(),
            // Root-local diagnostic index only; this is not a Session DocId/Slot.
            document: 0,
            document_key: "sanger-review".into(),
            generation: 0,
            revision: epoch,
            accepted_hashes: Vec::new(),
            target_revision: target_revision(&target)?,
            target,
        };
        SaveCheck::from_scope_with_participant(scope, Some(("sanger-working-state-v1", state)))
    }

    /// Inspect and publish one advisory receipt without writing records or saving.
    /// No guard is retained across the caller's interaction. Saving consumes
    /// this receipt and checks the live document and destination again.
    pub fn check_save(&self, path: &Path) -> Result<SaveCheck> {
        let check = self.save_scope(path)?;
        self.authority()?.0.pending.publish(check)
    }

    /// Bind a token to the live root's pending check, like `Session::decide_save`.
    /// Saving consumes the origin on entry and compares a fresh guarded scope,
    /// including public-field edits.
    pub fn decide_save(
        &self,
        path: &Path,
        token: &str,
        known: SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<SaveDecision> {
        if let Some(decision) =
            self.authority()?
                .0
                .pending
                .decide(token, known, acknowledge_incomplete)?
        {
            return Ok(decision);
        }
        Err(crate::Error::SaveDecisionStale(Box::new(
            self.check_save(path)?,
        )))
    }

    pub fn save_with_decision(
        &mut self,
        path: &Path,
        decision: SaveDecision,
    ) -> Result<crate::session::SaveOutcome> {
        let policy = decision.known;
        self.save_attempt(path, policy, Some(decision))
    }

    pub(super) fn consume_save_decision(&self, decision: &SaveDecision) -> Result<bool> {
        let (owner, _) = self.authority()?;
        if PendingSaveAuthority::origin(&owner.origin, decision) != Some(0) {
            return Ok(false);
        }
        owner.pending.consume(decision)
    }

    pub(super) fn validate_save_decision(
        &self,
        path: &Path,
        decision: &SaveDecision,
        consumed: bool,
    ) -> Result<String> {
        let state = self.state_digest()?;
        self.authority()?.0.pending.validate_decision(
            &self.save_scope_for_state(path, &state)?,
            decision,
            consumed,
        )?;
        Ok(state)
    }

    pub(super) fn authorize_save(
        &self,
        path: &Path,
        policy: SavePolicy,
        decision: Option<(SaveDecision, bool)>,
        state_before_preparation: Option<&str>,
    ) -> Result<Vec<crate::session::SaveWarning>> {
        // The exclusive &mut save has already validated public working state.
        // Internal source retention/recovery allocation may change its digest;
        // live target and relevant evidence must still match the displayed check.
        let fresh = match state_before_preparation {
            Some(state) => self.save_scope_for_state(path, state)?,
            None => self.save_scope(path)?,
        };
        if decision.is_none() && fresh.evidence().is_complete() {
            crate::input_evidence::resolve(&fresh.evidence().conflict, policy)?;
            return Ok(Vec::new());
        }
        self.authority()?
            .0
            .pending
            .authorize_write(
                &fresh,
                decision,
                policy,
                || crate::save_guard::Revision::check(self.accepted.as_ref(), path).map(|_| ()),
                None,
            )
            .or_else(|error| match error {
                crate::Error::SaveDecisionStale(_) => Err(crate::Error::SaveDecisionStale(
                    Box::new(self.check_save(path)?),
                )),
                crate::Error::SaveCheckRequired(_) => Err(crate::Error::SaveCheckRequired(
                    Box::new(self.check_save(path)?),
                )),
                error => Err(error),
            })
    }

    // JSON encodes every non-finite float as null. Reject these public working
    // states before hashing so distinct values cannot share a participant digest.
    // This is private binding validation, not a change to saved draft formats.
    fn validate_binding_floats(&self) -> Result<()> {
        if !self.draft.minimum_identity.is_finite()
            || self
                .draft
                .overlaps
                .iter()
                .any(|overlap| !overlap.identity.is_finite())
        {
            return Err(invalid(
                "Sanger save binding requires finite draft and overlap identities",
            ));
        }
        for child in &self.retained {
            child.validate_binding_floats()?;
        }
        Ok(())
    }

    fn state_digest(&self) -> Result<String> {
        self.validate_binding_floats()?;
        let mut sink = DigestSink(basecraft_io::Hasher::new());
        serde_json::to_writer(&mut sink, &State::from(self))?;
        Ok(sink.0.finish())
    }
}

// Stream a borrowed typed view directly to the digest: no cloned document,
// serde_json::Value tree or retained serialization buffer. Tokens are excluded.
struct DigestSink(basecraft_io::Hasher);
impl std::io::Write for DigestSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct State<'a> {
    retained: Children<'a>,
    draft: &'a SangerDraft,
    draft_path: &'a Path,
    draft_sha256: &'a str,
    draft_json: &'a str,
    decisions: &'a [ReviewChoice],
    mask: &'a MaskSettings,
    settings: &'a ReviewSettings,
    reference: &'a Option<ReviewReference>,
    manual: &'a [bool],
    events: &'a [ReviewOp],
    undo: &'a [Change],
    redo: &'a [Change],
    evidence: &'a ReviewEvidence,
    anchor: &'a Path,
    recovery: &'a Option<PathBuf>,
    accepted: Option<(
        &'a Path,
        &'a str,
        &'a basecraft_protocol::provenance::FileStamp,
    )>,
    stored_extra: &'a BTreeMap<String, serde_json::Value>,
}
impl<'a> From<&'a SangerReviewSession> for State<'a> {
    fn from(s: &'a SangerReviewSession) -> Self {
        Self {
            retained: Children(&s.retained),
            draft: &s.draft,
            draft_path: &s.draft_path,
            draft_sha256: &s.draft_sha256,
            draft_json: &s.draft_json,
            decisions: &s.decisions,
            mask: &s.mask,
            settings: &s.settings,
            reference: &s.reference,
            manual: &s.manual,
            events: &s.events,
            undo: &s.undo,
            redo: &s.redo,
            evidence: &s.evidence,
            anchor: &s.anchor,
            recovery: &s.recovery,
            accepted: s
                .accepted
                .as_ref()
                .map(crate::save_guard::Revision::snapshot),
            stored_extra: &s.stored_extra,
        }
    }
}
struct Children<'a>(&'a [SangerReviewSession]);
impl Serialize for Children<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for child in self.0 {
            sequence.serialize_element(&State::from(child))?;
        }
        sequence.end()
    }
}

#[cfg(test)]
mod tests;
