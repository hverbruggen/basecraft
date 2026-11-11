//! Adapt the review owner's fields to the pure document edit implementation.
use super::{Change, ReviewOp, SangerReview};
use basecraft_docs::sanger::review::{ReviewState, ReviewStateMut};

impl SangerReview {
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-review/multi.json");
        let mut review = SangerReview::load(&path).unwrap();
        let before = review.document_report(false).unwrap().raw;
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-review/single.json");
        let mut review = SangerReview::load(&path).unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('N'),
            })
            .unwrap();
        let before = review.document_report(false).unwrap().raw;
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
        }
        assert_eq!(review.decisions[0], ReviewChoice::Edit('N'));
    }
}

impl SangerReview {
    fn call_set_mut(&mut self, id: &str) -> crate::Result<&mut Self> {
        if self.draft_sha256 == id {
            return Ok(self);
        }
        self.retained
            .iter_mut()
            .find(|set| set.draft_sha256 == id)
            .ok_or_else(|| crate::Error::Unsupported("Unknown Sanger call set".into()))
    }

    pub(crate) fn apply_session_edit(
        &mut self,
        id: &str,
        op: &ReviewOp,
    ) -> crate::Result<crate::session::Inverse> {
        use crate::Error::Unsupported;
        // Live history belongs to Session.
        if matches!(
            op,
            ReviewOp::Undo | ReviewOp::Redo | ReviewOp::Export { .. }
        ) {
            return Err(Unsupported(
                "use the shared history or export operation".into(),
            ));
        }
        let set = self.call_set_mut(id)?;
        let change = set.prepare_change(op).map_err(Unsupported)?;
        let events = set.events.len();
        set.install_change(&change, true).map_err(Unsupported)?;
        set.events.push(op.clone());
        Ok(crate::session::Inverse::SangerReview {
            call_set: id.into(),
            change: Box::new(change),
            events,
        })
    }

    pub(crate) fn revert_session_edit(
        &mut self,
        inverse: &crate::session::Inverse,
    ) -> crate::Result<()> {
        let crate::session::Inverse::SangerReview {
            call_set,
            change,
            events,
        } = inverse
        else {
            return Err(crate::Error::Unsupported(
                "undo of the wrong kind of edit".into(),
            ));
        };
        let set = self.call_set_mut(call_set)?;
        set.install_change(change, false)
            .map_err(crate::Error::Unsupported)?;
        set.events.truncate(*events);
        Ok(())
    }
}
