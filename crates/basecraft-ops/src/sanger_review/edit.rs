//! Pure review edit preparation and installation. No journal, checkpoint,
//! source read, consent or recovery side effect occurs here.
use super::{Change, ReviewChoice, ReviewOp, SangerReviewSession};

impl SangerReviewSession {
    pub(super) fn prepare_change(&self, op: &ReviewOp) -> Result<Change, String> {
        match op {
            ReviewOp::Evaluate {
                version,
                settings,
                calls,
                reference,
            } => {
                settings.validate()?;
                if ![1, 2].contains(version)
                    || calls.len() != self.decisions.len()
                    || calls.iter().flatten().any(|b| !"ACGT".contains(*b))
                    || reference.as_ref().is_some_and(|r| {
                        r.counts.len() != calls.len()
                            || r.source_sha256.len() != 64
                            || !r.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                {
                    return Err("Invalid automatic review evidence".into());
                }
                let columns: Vec<usize> = (0..calls.len()).filter(|&i| !self.manual[i]).collect();
                let before = columns.iter().map(|&i| self.decisions[i].clone()).collect();
                let after: Vec<_> = columns
                    .iter()
                    .map(|&i| calls[i].map_or(ReviewChoice::Pending, ReviewChoice::Auto))
                    .collect();
                Ok(Change {
                    manual_before: vec![false; columns.len()],
                    manual_after: vec![false; columns.len()],
                    columns,
                    before,
                    after,
                    mask: None,
                    triage: Some((
                        self.settings.clone(),
                        self.reference.clone(),
                        settings.clone(),
                        reference.clone(),
                    )),
                })
            }
            ReviewOp::Mask { settings } => {
                if settings.start >= settings.end
                    || settings.end > self.decisions.len()
                    || !(8..=100).contains(&settings.minimum_overlap)
                    || settings.error_percent > 40
                    || settings.forward.len() > 200
                    || settings.reverse.len() > 200
                    || !settings
                        .forward
                        .bytes()
                        .chain(settings.reverse.bytes())
                        .all(|b| b"ACGTNRYKMSWBDHV".contains(&b))
                {
                    return Err("Invalid terminal mask or primer settings".into());
                }
                Ok(Change {
                    columns: Vec::new(),
                    before: Vec::new(),
                    after: Vec::new(),
                    manual_before: Vec::new(),
                    manual_after: Vec::new(),
                    triage: None,
                    mask: Some((self.mask.clone(), settings.clone())),
                })
            }
            ReviewOp::Set { columns, choice } => {
                if matches!(choice, ReviewChoice::Auto(_)) {
                    return Err("Automatic calls require a recorded evaluation".into());
                }
                if columns.is_empty() || columns.len() > self.decisions.len() {
                    return Err("Select one or more consensus columns".into());
                }
                if let ReviewChoice::Edit(base) = choice
                    && (!base.is_ascii() || !b"ACGTNRYKMSWBDHV-".contains(&(*base as u8)))
                {
                    return Err("Manual base must be an IUPAC DNA symbol or gap".into());
                }
                let mut sorted = columns.clone();
                sorted.sort_unstable();
                sorted.dedup();
                if sorted.len() != columns.len()
                    || sorted.iter().any(|&column| column >= self.decisions.len())
                {
                    return Err("Invalid or repeated consensus column".into());
                }
                let before = columns
                    .iter()
                    .map(|&column| self.decisions[column].clone())
                    .collect();
                let manual_before = columns.iter().map(|&i| self.manual[i]).collect();
                Ok(Change {
                    columns: columns.clone(),
                    before,
                    after: vec![choice.clone(); columns.len()],
                    manual_before,
                    manual_after: vec![true; columns.len()],
                    triage: None,
                    mask: None,
                })
            }
            ReviewOp::AutoAccept { columns } => {
                if !self.events.is_empty() || columns.iter().any(|&i| i >= self.decisions.len()) {
                    return Err("Automatic triage is only allowed before manual review".into());
                }
                Ok(Change {
                    columns: columns.clone(),
                    before: columns.iter().map(|&i| self.decisions[i].clone()).collect(),
                    after: vec![ReviewChoice::Accept; columns.len()],
                    manual_before: columns.iter().map(|&i| self.manual[i]).collect(),
                    manual_after: columns.iter().map(|&i| self.manual[i]).collect(),
                    triage: None,
                    mask: None,
                })
            }
            ReviewOp::Undo | ReviewOp::Redo | ReviewOp::Export { .. } => {
                Err("History and export are owned by the review session".into())
            }
        }
    }

    /// Install a delta prepared for this payload. The owner controls its stack
    /// and audit event; preparing an edit never changes either.
    pub(super) fn install_change(&mut self, change: &Change, forward: bool) {
        if let Some((before, after)) = &change.mask {
            self.mask.clone_from(if forward { after } else { before });
        }
        let choices = if forward {
            &change.after
        } else {
            &change.before
        };
        let manual = if forward {
            &change.manual_after
        } else {
            &change.manual_before
        };
        for ((&column, choice), &manual) in change.columns.iter().zip(choices).zip(manual) {
            self.decisions[column] = choice.clone();
            self.manual[column] = manual;
        }
        if let Some((before, reference_before, after, reference_after)) = &change.triage {
            self.settings
                .clone_from(if forward { after } else { before });
            self.reference.clone_from(if forward {
                reference_after
            } else {
                reference_before
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn prepared_review_edit_does_not_touch_history_or_inactive_call_sets() {
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
        review.install_change(&change, true);
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
        review.install_change(&change, false);
        assert_eq!(review.document_report(false).unwrap().raw, before);
    }

    #[test]
    fn invalid_preparation_preserves_the_existing_redo_and_decisions() {
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
