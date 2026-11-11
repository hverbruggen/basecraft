//! Edit-sized Sanger review changes. The operation owner supplies whether this
//! is initial triage and owns history, replay, recovery and all file access.
use basecraft_protocol::sanger::{
    MaskSettings, ReviewChoice, ReviewOp, ReviewReference, ReviewSettings,
};
use serde::Serialize;

/// A borrowed view of the biological decisions; preparation never mutates it.
pub struct ReviewState<'a> {
    pub decisions: &'a [ReviewChoice],
    pub manual: &'a [bool],
    pub mask: &'a MaskSettings,
    pub settings: &'a ReviewSettings,
    pub reference: &'a Option<ReviewReference>,
    pub initial: bool,
}

/// Disjoint mutable fields supplied by the document owner when applying a delta.
pub struct ReviewStateMut<'a> {
    pub decisions: &'a mut [ReviewChoice],
    pub manual: &'a mut [bool],
    pub mask: &'a mut MaskSettings,
    pub settings: &'a mut ReviewSettings,
    pub reference: &'a mut Option<ReviewReference>,
}

/// A private in-memory delta, not an on-disk record or an undo stack. The owner
/// retains it for undo; columns address this one immutable source draft.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewChange {
    columns: Vec<usize>,
    before: Vec<ReviewChoice>,
    after: Vec<ReviewChoice>,
    manual_before: Vec<bool>,
    manual_after: Vec<bool>,
    triage: Option<(
        ReviewSettings,
        Option<ReviewReference>,
        ReviewSettings,
        Option<ReviewReference>,
    )>,
    mask: Option<(MaskSettings, MaskSettings)>,
}

impl ReviewState<'_> {
    pub fn prepare(&self, op: &ReviewOp) -> Result<ReviewChange, String> {
        if self.manual.len() != self.decisions.len() {
            return Err("Review decisions and manual flags have different lengths".into());
        }
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
                Ok(ReviewChange {
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
                Ok(ReviewChange {
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
                Ok(ReviewChange {
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
                if !self.initial || columns.iter().any(|&i| i >= self.decisions.len()) {
                    return Err("Automatic triage is only allowed before manual review".into());
                }
                Ok(ReviewChange {
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
}

impl ReviewChange {
    /// Install a delta prepared for this payload. The owner controls its stack
    /// and audit event; preparing an edit never changes either.
    pub fn install(&self, state: &mut ReviewStateMut<'_>, forward: bool) -> Result<(), String> {
        if state.manual.len() != state.decisions.len()
            || self
                .columns
                .iter()
                .any(|&column| column >= state.decisions.len())
        {
            return Err("Review edit does not match this draft's columns".into());
        }
        if let Some((before, after)) = &self.mask {
            state.mask.clone_from(if forward { after } else { before });
        }
        let choices = if forward { &self.after } else { &self.before };
        let manual = if forward {
            &self.manual_after
        } else {
            &self.manual_before
        };
        for ((&column, choice), &manual) in self.columns.iter().zip(choices).zip(manual) {
            state.decisions[column] = choice.clone();
            state.manual[column] = manual;
        }
        if let Some((before, reference_before, after, reference_after)) = &self.triage {
            state
                .settings
                .clone_from(if forward { after } else { before });
            state.reference.clone_from(if forward {
                reference_after
            } else {
                reference_before
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask() -> MaskSettings {
        MaskSettings {
            start: 0,
            end: 2,
            forward: String::new(),
            reverse: String::new(),
            minimum_overlap: 12,
            error_percent: 25,
            evidence: String::new(),
        }
    }

    #[test]
    fn delta_rejects_incompatible_columns_before_changing_any_field() {
        let mut decisions = vec![ReviewChoice::Pending; 2];
        let mut manual = vec![false; 2];
        let mut mask = mask();
        let mut settings = ReviewSettings::default();
        let mut reference = None;
        let op = ReviewOp::Set {
            columns: vec![0, 1],
            choice: ReviewChoice::Edit('N'),
        };
        let change = ReviewState {
            decisions: &decisions,
            manual: &manual,
            mask: &mask,
            settings: &settings,
            reference: &reference,
            initial: true,
        }
        .prepare(&op)
        .unwrap();
        // The first column is valid but the second is not. No partial edit.
        decisions.truncate(1);
        manual.truncate(1);
        let before = (
            decisions.clone(),
            manual.clone(),
            mask.clone(),
            settings.clone(),
            reference.clone(),
        );
        assert!(
            change
                .install(
                    &mut ReviewStateMut {
                        decisions: &mut decisions,
                        manual: &mut manual,
                        mask: &mut mask,
                        settings: &mut settings,
                        reference: &mut reference,
                    },
                    true
                )
                .is_err()
        );
        assert_eq!((decisions, manual, mask, settings, reference), before);
    }

    #[test]
    fn preparation_rejects_inconsistent_manual_flags() {
        let mask = mask();
        let settings = ReviewSettings::default();
        let state = ReviewState {
            decisions: &[ReviewChoice::Pending, ReviewChoice::Pending],
            manual: &[false],
            mask: &mask,
            settings: &settings,
            reference: &None,
            initial: true,
        };
        assert!(
            state
                .prepare(&ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Accept
                })
                .is_err()
        );
    }
}
