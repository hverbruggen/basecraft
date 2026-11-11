//! Sanger decisions use the same journal, undo stacks and save guards as other kinds.
use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use super::{Document, WritePublication};
use crate::sanger_review::SangerReview;
use crate::{
    Error, Result,
    files::{Format, Kind},
    session::Inverse,
};

impl Document for SangerReview {
    fn accepts(&self, op: &Op) -> bool {
        op.name().starts_with("sanger.")
    }

    fn kind(&self) -> Kind {
        Kind::SangerReview
    }
    fn revision(&self, generation: u64) -> u64 {
        generation
    }
    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::SangerReview {
            columns: self.draft.columns.len() as u64,
            unresolved: self.unresolved() as u64,
            call_sets: self.call_sets().len() as u64,
        })
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        if let Op::SangerUpdate { review } = op {
            let mut replacement =
                SangerReview::from_session_snapshot(review, self.storage_anchor())?;
            replacement.share_captures(self);
            return Ok((
                Inverse::SangerUpdate(Box::new(std::mem::replace(self, replacement))),
                None,
            ));
        }
        let Op::SangerReview { call_set, edit } = op else {
            return Err(Error::Unsupported(format!(
                "{} cannot be applied to a Sanger review",
                op.name()
            )));
        };
        self.apply_session_edit(call_set, edit)
            .map(|inverse| (inverse, None))
    }
    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        if let Inverse::SangerUpdate(before) = inverse {
            *self = (**before).clone();
            return Ok(());
        }
        self.revert_session_edit(inverse)
    }
    fn proposal_candidate(&self) -> Result<crate::files::Loaded> {
        Ok(crate::files::Loaded::SangerReview(Box::new(self.clone())))
    }
    fn apply_proposal(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        Document::apply(self, op)
    }
    fn rebase_replay_paths(&mut self, from: &Path, to: &Path) {
        self.rebase_session_paths(from, to);
    }
    fn write(
        &self,
        target: &Path,
        format: Format,
        _recovery: &Path,
        publication: &WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != Format::SangerReview {
            return Err(Error::Unsupported(
                "a Sanger review is saved as JSON".into(),
            ));
        }
        self.write_session_review(target, &publication.logical(target)?, publication)
    }
    fn saved_hashes(&self, target: &Path, publication: &WritePublication) -> Result<Vec<String>> {
        Ok(vec![basecraft_io::hash_bytes(
            &self.session_bytes(&publication.logical(target)?)?,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_target_created_after_admission_is_preserved() {
        let Some(root) = crate::isolated_profile::enter(
            "documents::sanger::tests::a_new_target_created_after_admission_is_preserved",
        ) else {
            return;
        };
        let review = SangerReview::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/sanger-review/multi.json"),
        )
        .unwrap();
        let target = root.join("review.json");
        std::fs::write(&target, b"another writer's file").unwrap();
        let _lock = crate::input_evidence::lock().unwrap();
        assert!(
            review
                .write(
                    &target,
                    Format::SangerReview,
                    &root,
                    &WritePublication::Create
                )
                .is_err()
        );
        assert_eq!(std::fs::read(target).unwrap(), b"another writer's file");
    }
}
