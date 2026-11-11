//! Test fixtures exercise edits and saves through the public Session API.
use super::{ReviewOp, SangerReview};
use crate::{DocId, Session};
use basecraft_protocol::Op;
use std::path::Path;

pub(super) struct ReviewSession {
    owner: Session,
    id: DocId,
}
impl std::ops::Deref for ReviewSession {
    type Target = SangerReview;
    fn deref(&self) -> &Self::Target {
        self.owner.sanger_review(self.id).unwrap()
    }
}
impl ReviewSession {
    pub(super) fn new(review: &SangerReview) -> Self {
        let mut owner = Session::recoverable(review.storage_anchor().parent().unwrap()).unwrap();
        let id = owner.create_sanger_review(review).unwrap();
        Self { owner, id }
    }
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let mut owner = Session::recoverable(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let id = owner.open(path).map_err(|e| e.to_string())?;
        Ok(Self { owner, id })
    }
    pub(super) fn apply(&mut self, edit: ReviewOp) -> Result<(), String> {
        let call_set = self.draft_sha256.clone();
        self.owner
            .apply(
                self.id,
                Op::SangerReview {
                    call_set,
                    edit: Box::new(edit),
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub(super) fn select_call_set(&mut self, call_set: &str) -> Result<(), String> {
        self.owner
            .apply(
                self.id,
                Op::SangerSelect {
                    call_set: call_set.into(),
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub(super) fn retain_call_set(&mut self, other: &Self) -> Result<(), String> {
        let mut review = (**self).clone();
        review.retain_call_set((*other).clone())?;
        self.owner
            .update_sanger_review(self.id, &review)
            .map_err(|e| e.to_string())
    }
    pub(super) fn save(&mut self, path: &Path) -> Result<(), String> {
        self.owner
            .save_new(self.id, Some(path))
            .map_err(|e| e.to_string())
    }
    pub(super) fn save_with_policy(
        &mut self,
        path: &Path,
        policy: crate::input_evidence::SavePolicy,
    ) -> Result<(), String> {
        let check = self
            .owner
            .check_save(self.id, Some(path))
            .map_err(|e| e.to_string())?;
        self.owner
            .save_with_decision(self.id, Some(path), check.decide(policy, false))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub(super) fn export_consensus(&mut self, path: &Path, label: &str) -> Result<(), String> {
        self.owner
            .export_sanger_consensus(self.id, path, label)
            .map_err(|e| e.to_string())
    }
    pub(super) fn attach_run(&mut self, run: &super::storage::SangerRun) -> Result<(), String> {
        let mut review = (**self).clone();
        review.attach_run(run)?;
        self.owner
            .update_sanger_review(self.id, &review)
            .map_err(|e| e.to_string())
    }
    pub(super) fn attach_placement(
        &mut self,
        run: &super::storage::SangerRun,
        path: &Path,
    ) -> Result<(), String> {
        let mut review = (**self).clone();
        review.attach_placement(run, path)?;
        self.owner
            .update_sanger_review(self.id, &review)
            .map_err(|e| e.to_string())
    }
}
