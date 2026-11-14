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
    pub(super) fn load(path: &Path) -> crate::Result<Self> {
        let mut owner = Session::recoverable(path.parent().unwrap())?;
        let id = owner.open(path)?;
        Ok(Self { owner, id })
    }
    pub(super) fn apply(&mut self, edit: ReviewOp) -> crate::Result<()> {
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
    }
    pub(super) fn select_call_set(&mut self, call_set: &str) -> crate::Result<()> {
        self.owner
            .apply(
                self.id,
                Op::SangerSelect {
                    call_set: call_set.into(),
                },
            )
            .map(|_| ())
    }
    pub(super) fn retain_call_set(&mut self, other: &Self) -> crate::Result<()> {
        let mut review = (**self).clone();
        review.retain_call_set((*other).clone())?;
        self.owner
            .update_sanger_review(self.id, &review)
    }
    pub(super) fn save(&mut self, path: &Path) -> crate::Result<()> {
        self.owner
            .save_new(self.id, Some(path))
    }
    pub(super) fn save_with_policy(
        &mut self,
        path: &Path,
        policy: crate::records::input_evidence::SavePolicy,
    ) -> crate::Result<()> {
        let check = self
            .owner
            .check_save(self.id, Some(path))?;
        self.owner
            .save_with_decision(self.id, Some(path), check.decide(policy, false))
            .map(|_| ())
    }
    pub(super) fn export_consensus(&mut self, path: &Path, label: &str) -> crate::Result<()> {
        self.owner
            .export_sanger_consensus(self.id, path, label)
    }
    pub(super) fn attach_run(&mut self, run: &super::storage::SangerRun) -> crate::Result<()> {
        let mut review = (**self).clone();
        review.attach_run(run)?;
        self.owner
            .update_sanger_review(self.id, &review)
    }
    pub(super) fn attach_placement(
        &mut self,
        run: &super::storage::SangerRun,
        path: &Path,
    ) -> crate::Result<()> {
        let mut review = (**self).clone();
        review.attach_placement(run, path)?;
        self.owner
            .update_sanger_review(self.id, &review)
    }
}
