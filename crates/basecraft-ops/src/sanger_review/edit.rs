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
            .join("../../testdata/fixtures/sanger-legacy/multi.json");
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
            .join("../../testdata/fixtures/sanger-legacy/single.json");
        let mut review = SangerReview::load(&path).unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('N'),
            })
            .unwrap();
        review.apply(ReviewOp::Undo).unwrap();
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
        review.apply(ReviewOp::Redo).unwrap();
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
        // Legacy control events remain readable, but live history belongs to Session.
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
        let legacy_undo = !(matches!(op, ReviewOp::AutoAccept { .. })
            || events == 0 && matches!(op, ReviewOp::Evaluate { .. }));
        set.install_change(&change, true).map_err(Unsupported)?;
        set.events.push(op.clone());
        Ok(crate::session::Inverse::SangerReview {
            call_set: id.into(),
            change: Box::new(change),
            events,
            legacy_undo,
        })
    }

    pub(crate) fn revert_session_edit(
        &mut self,
        inverse: &crate::session::Inverse,
        record: bool,
    ) -> crate::Result<()> {
        let crate::session::Inverse::SangerReview {
            call_set,
            change,
            events,
            legacy_undo,
        } = inverse
        else {
            return Err(crate::Error::Unsupported(
                "undo of the wrong kind of edit".into(),
            ));
        };
        let set = self.call_set_mut(call_set)?;
        set.install_change(change, false)
            .map_err(crate::Error::Unsupported)?;
        if record && *legacy_undo {
            set.events.push(ReviewOp::Undo);
        } else {
            set.events.truncate(*events);
        }
        Ok(())
    }
}

impl SangerReview {
    pub(crate) fn reapply_session_edit(
        &mut self,
        op: &basecraft_protocol::Op,
        inverse: &crate::session::Inverse,
        record: bool,
    ) -> crate::Result<crate::session::Inverse> {
        let crate::session::Inverse::SangerReview {
            call_set,
            change,
            events,
            legacy_undo,
        } = inverse
        else {
            return crate::documents::Document::apply(self, op).map(|(inverse, _)| inverse);
        };
        if !*legacy_undo {
            return crate::documents::Document::apply(self, op).map(|(inverse, _)| inverse);
        }
        let set = self.call_set_mut(call_set)?;
        let before = set.events.len();
        let can_record_redo = record && set.session_history()?.1.last().is_some_and(|entry| &entry.op == op && matches!(&entry.inverse, crate::session::Inverse::SangerReview { change: prior, .. } if prior == change));
        set.install_change(change, true)
            .map_err(crate::Error::Unsupported)?;
        if can_record_redo {
            set.events.push(ReviewOp::Redo);
        } else if record {
            let basecraft_protocol::Op::SangerReview { edit, .. } = op else {
                return Err(crate::Error::Unsupported(
                    "redo requires a review decision".into(),
                ));
            };
            set.events.push((**edit).clone());
        } else {
            set.events.pop();
        }
        Ok(crate::session::Inverse::SangerReview {
            call_set: call_set.clone(),
            change: change.clone(),
            events: if record { before } else { *events },
            legacy_undo: true,
        })
    }
}

impl SangerReview {
    pub(crate) fn session_history(
        &self,
    ) -> crate::Result<(Vec<crate::session::Applied>, Vec<crate::session::Applied>)> {
        use crate::{
            Error,
            session::{Applied, Inverse},
        };
        let mut state = Self::from_draft(self.draft.clone(), self.storage_anchor())
            .map_err(Error::Unsupported)?;
        let mut undo: Vec<Applied> = Vec::new();
        let mut redo: Vec<Applied> = Vec::new();
        for (index, event) in self.events.iter().enumerate() {
            match event {
                ReviewOp::Undo => {
                    let edit = undo.pop().ok_or_else(|| {
                        Error::Unsupported("Saved review undo has no decision".into())
                    })?;
                    if let Inverse::SangerReview { change, .. } = &edit.inverse {
                        state
                            .install_change(change, false)
                            .map_err(Error::Unsupported)?;
                    }
                    redo.push(edit);
                }
                ReviewOp::Redo => {
                    let edit = redo.pop().ok_or_else(|| {
                        Error::Unsupported("Saved review redo has no decision".into())
                    })?;
                    if let Inverse::SangerReview { change, .. } = &edit.inverse {
                        state
                            .install_change(change, true)
                            .map_err(Error::Unsupported)?;
                    }
                    undo.push(edit);
                }
                ReviewOp::Export { .. } => {}
                event => {
                    let change = state.prepare_change(event).map_err(Error::Unsupported)?;
                    state
                        .install_change(&change, true)
                        .map_err(Error::Unsupported)?;
                    if !matches!(event, ReviewOp::AutoAccept { .. }) {
                        if index != 0 || !matches!(event, ReviewOp::Evaluate { .. }) {
                            undo.push(Applied {
                                entry_id: format!("sanger-legacy:{}:{index}", self.draft_sha256),
                                op: basecraft_protocol::Op::SangerReview {
                                    call_set: self.draft_sha256.clone(),
                                    edit: Box::new(event.clone()),
                                },
                                inverse: Inverse::SangerReview {
                                    call_set: self.draft_sha256.clone(),
                                    change: Box::new(change),
                                    events: index,
                                    legacy_undo: true,
                                },
                                group: None,
                            });
                        }
                        redo.clear();
                    }
                }
            }
            state.events.push(event.clone());
        }
        Ok((undo, redo))
    }
}
