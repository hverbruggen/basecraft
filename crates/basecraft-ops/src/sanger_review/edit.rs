//! Adapt the review owner's fields to the pure document edit implementation.
use super::{Change, ReviewOp, SangerReviewSession};
use basecraft_docs::sanger::review::{ReviewState, ReviewStateMut};

impl SangerReviewSession {
    pub(super) fn prepare_change(&self, op: &ReviewOp) -> Result<Change, String> {
        ReviewState {
            decisions: &self.decisions,
            manual: &self.manual,
            mask: &self.mask,
            settings: &self.settings,
            reference: &self.reference,
            initial: self.events.is_empty(),
        }
        .prepare(op)
    }

    pub(super) fn install_change(&mut self, change: &Change, forward: bool) -> Result<(), String> {
        change.install(
            &mut ReviewStateMut {
                decisions: &mut self.decisions,
                manual: &mut self.manual,
                mask: &mut self.mask,
                settings: &mut self.settings,
                reference: &mut self.reference,
            },
            forward,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanger_review::ReviewChoice;
    use std::path::Path;

    #[test]
    fn prepared_review_edit_does_not_touch_history_or_inactive_call_sets() {
        crate::isolated_profile::isolate!();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-legacy/multi.json");
        let mut review = SangerReviewSession::load(&path).unwrap();
        let before = review.document_report(false).unwrap().raw;
        let undo = serde_json::to_value(&review.undo).unwrap();
        let redo = serde_json::to_value(&review.redo).unwrap();
        let events = review.events.clone();
        let retained: Vec<_> = review
            .retained
            .iter()
            .map(|set| set.document_report(false).unwrap().raw)
            .collect();
        let edit = ReviewOp::Set {
            columns: vec![0],
            choice: ReviewChoice::Edit('N'),
        };
        let change = review.prepare_change(&edit).unwrap();
        assert_eq!(review.document_report(false).unwrap().raw, before);
        review.install_change(&change, true).unwrap();
        assert_eq!(review.decisions[0], ReviewChoice::Edit('N'));
        assert!(review.manual[0]);
        assert_eq!(review.events, events);
        assert_eq!(serde_json::to_value(&review.undo).unwrap(), undo);
        assert_eq!(serde_json::to_value(&review.redo).unwrap(), redo);
        assert_eq!(
            review
                .retained
                .iter()
                .map(|set| set.document_report(false).unwrap().raw)
                .collect::<Vec<_>>(),
            retained
        );
        review.install_change(&change, false).unwrap();
        assert_eq!(review.document_report(false).unwrap().raw, before);
    }

    #[test]
    fn invalid_preparation_preserves_the_existing_redo_and_decisions() {
        crate::isolated_profile::isolate!();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-legacy/single.json");
        let mut review = SangerReviewSession::load(&path).unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('N'),
            })
            .unwrap();
        review.apply(ReviewOp::Undo).unwrap();
        let before = review.document_report(false).unwrap().raw;
        let redo = serde_json::to_value(&review.redo).unwrap();
        for edit in [
            ReviewOp::Set {
                columns: vec![0, 0],
                choice: ReviewChoice::Accept,
            },
            ReviewOp::Set {
                columns: vec![usize::MAX],
                choice: ReviewChoice::Accept,
            },
            ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Auto('A'),
            },
            ReviewOp::AutoAccept { columns: vec![0] },
        ] {
            assert!(review.prepare_change(&edit).is_err());
            assert_eq!(review.document_report(false).unwrap().raw, before);
            assert_eq!(serde_json::to_value(&review.redo).unwrap(), redo);
        }
        review.apply(ReviewOp::Redo).unwrap();
        assert_eq!(review.decisions[0], ReviewChoice::Edit('N'));
    }
}
