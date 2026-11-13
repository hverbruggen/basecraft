//! What the window asks after an edit, such as a reroot, leaves clade labels
//! on other groups of tips (D11): one label at a time, or every label at once,
//! move it to the clade that holds exactly its tips (when one does), move it
//! to the smallest clade holding all of them, remove it, or undo the reroot.
//! Dismissing leaves the labels where they are; the tree's status line names
//! them.
use basecraft_protocol::tree::{DisplacedLabel, LabelPlacement};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Exact,
    Smallest,
    Remove,
    UndoReroot,
}

impl Answer {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "exact" => Self::Exact,
            "smallest" => Self::Smallest,
            "remove" => Self::Remove,
            "undo" => Self::UndoReroot,
            _ => return None,
        })
    }
}

/// What the controller does with an answer.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Place these labels as `choice` says (`tree.place_label`); one undo step.
    Place {
        labels: Vec<u32>,
        choice: LabelPlacement,
    },
    /// Undo this many steps: the placements made so far, then the reroot.
    Undo(usize),
}

/// The question the dialog shows.
#[derive(Debug, PartialEq, Serialize)]
pub struct Question {
    pub label: String,
    /// Which of the labels this edit displaced, from 1.
    pub number: usize,
    pub total: usize,
    /// "Move to exact clade" is offered for this label alone.
    pub exact: bool,
    /// ... and for every label still to place, when the answer applies to all.
    pub exact_for_all: bool,
}

#[derive(Debug)]
pub struct Questions {
    asked: Vec<DisplacedLabel>,
    pending: Vec<DisplacedLabel>,
    placed: usize,
}

impl Questions {
    /// Ask about the labels an edit displaced: those displaced `after` it
    /// that were not displaced `before`. None when there are none.
    pub fn after_edit(before: &[DisplacedLabel], after: Vec<DisplacedLabel>) -> Option<Self> {
        let asked: Vec<_> = after
            .into_iter()
            .filter(|label| before.iter().all(|old| old.id != label.id))
            .collect();
        (!asked.is_empty()).then(|| Self {
            pending: asked.clone(),
            asked,
            placed: 0,
        })
    }

    pub fn question(&self) -> Option<Question> {
        let current = self.pending.first()?;
        Some(Question {
            label: current.label.clone(),
            number: self.asked.len() - self.pending.len() + 1,
            total: self.asked.len(),
            exact: current.exact,
            exact_for_all: self.pending.iter().all(|label| label.exact),
        })
    }

    /// The step for `answer`, to the current label or with `all` to every
    /// label still to place. None when the answer is not offered.
    pub fn answer(&self, answer: Answer, all: bool) -> Option<Step> {
        let current = self.pending.first()?;
        let exact = if all {
            self.pending.iter().all(|l| l.exact)
        } else {
            current.exact
        };
        let choice = match answer {
            // Back past the placements made so far, then the reroot itself.
            Answer::UndoReroot => return Some(Step::Undo(self.placed + 1)),
            Answer::Exact if !exact => return None,
            Answer::Exact => LabelPlacement::Exact,
            Answer::Smallest => LabelPlacement::Smallest,
            Answer::Remove => LabelPlacement::Remove,
        };
        let labels = if all {
            self.pending.iter().map(|label| label.id).collect()
        } else {
            vec![current.id]
        };
        Some(Step::Place { labels, choice })
    }

    /// The placement edit for `labels` was applied: one more undo step, and
    /// those labels are no longer asked about. Not called when it failed.
    pub fn placed(&mut self, labels: &[u32]) {
        self.placed += 1;
        self.pending.retain(|label| !labels.contains(&label.id));
    }

    /// Keep asking only about labels the tree still reports as displaced,
    /// with what it now says about them.
    pub fn refresh(&mut self, displaced: &[DisplacedLabel]) {
        self.pending = self
            .pending
            .iter()
            .filter_map(|label| displaced.iter().find(|now| now.id == label.id).cloned())
            .collect();
    }

    pub fn done(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(id: u32, name: &str, exact: bool) -> DisplacedLabel {
        DisplacedLabel {
            id,
            label: name.into(),
            exact,
        }
    }
    fn place(labels: &[u32], choice: LabelPlacement) -> Step {
        Step::Place {
            labels: labels.to_vec(),
            choice,
        }
    }

    #[test]
    fn only_labels_the_edit_displaced_are_asked_about() {
        let earlier = [label(1, "Left alone", false)];
        assert!(Questions::after_edit(&earlier, earlier.to_vec()).is_none());
        let asked = Questions::after_edit(
            &earlier,
            vec![label(1, "Left alone", false), label(2, "AB", true)],
        )
        .unwrap();
        assert_eq!(asked.question().unwrap().label, "AB");
        assert_eq!(asked.question().unwrap().total, 1);
    }

    /// Answer, and confirm as the controller does once the edit applied.
    fn apply(q: &mut Questions, answer: Answer, all: bool) -> Option<Step> {
        let step = q.answer(answer, all);
        if let Some(Step::Place { labels, .. }) = &step {
            q.placed(labels);
        }
        step
    }

    #[test]
    fn each_label_is_asked_in_turn_and_exact_only_where_a_clade_matches() {
        let mut q =
            Questions::after_edit(&[], vec![label(1, "All", true), label(2, "AB", false)]).unwrap();
        let first = q.question().unwrap();
        assert_eq!(
            (first.label.as_str(), first.number, first.total),
            ("All", 1, 2)
        );
        assert!(first.exact && !first.exact_for_all);
        // Exact for every label is refused while one has no exact clade.
        assert_eq!(q.answer(Answer::Exact, true), None);
        assert_eq!(
            apply(&mut q, Answer::Exact, false),
            Some(place(&[1], LabelPlacement::Exact))
        );
        let second = q.question().unwrap();
        assert_eq!(
            (second.label.as_str(), second.number, second.exact),
            ("AB", 2, false)
        );
        assert_eq!(q.answer(Answer::Exact, false), None);
        assert_eq!(
            apply(&mut q, Answer::Remove, false),
            Some(place(&[2], LabelPlacement::Remove))
        );
        assert!(q.done() && q.question().is_none());
    }

    #[test]
    fn one_answer_can_place_every_label_left() {
        let mut q = Questions::after_edit(
            &[],
            vec![
                label(1, "All", true),
                label(2, "AB", false),
                label(3, "CD", false),
            ],
        )
        .unwrap();
        apply(&mut q, Answer::Remove, false);
        assert_eq!(
            apply(&mut q, Answer::Smallest, true),
            Some(place(&[2, 3], LabelPlacement::Smallest))
        );
        assert!(q.done());
    }

    #[test]
    fn undoing_the_reroot_also_undoes_the_placements_made_since() {
        let mut q =
            Questions::after_edit(&[], vec![label(1, "All", true), label(2, "AB", false)]).unwrap();
        assert_eq!(q.answer(Answer::UndoReroot, false), Some(Step::Undo(1)));
        apply(&mut q, Answer::Smallest, false);
        assert_eq!(q.answer(Answer::UndoReroot, false), Some(Step::Undo(2)));
    }

    #[test]
    fn a_placement_that_failed_counts_nothing_and_asks_again() {
        let q =
            Questions::after_edit(&[], vec![label(1, "All", true), label(2, "AB", false)]).unwrap();
        // The controller's edit failed, so it never confirms the placement.
        assert!(q.answer(Answer::Smallest, false).is_some());
        assert_eq!(q.question().unwrap().label, "All");
        assert_eq!(q.answer(Answer::UndoReroot, false), Some(Step::Undo(1)));
    }

    #[test]
    fn a_placement_that_settles_another_label_drops_its_question() {
        let mut q =
            Questions::after_edit(&[], vec![label(1, "All", true), label(2, "AB", false)]).unwrap();
        apply(&mut q, Answer::Exact, false);
        q.refresh(&[label(2, "AB", true)]);
        assert!(
            q.question().unwrap().exact,
            "the tree's current answer counts"
        );
        q.refresh(&[]);
        assert!(q.done());
    }
}
