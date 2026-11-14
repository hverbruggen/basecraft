//! Adapt the review owner's fields to the pure document edit implementation.
use super::{Change, ReviewOp, SangerReview};
use crate::error::{invalid, unsupported};
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

impl SangerReview {
    fn call_set_mut(&mut self, id: &str) -> crate::Result<&mut Self> {
        if self.draft_sha256 == id {
            return Ok(self);
        }
        self.retained
            .iter_mut()
            .find(|set| set.draft_sha256 == id)
            .ok_or_else(|| invalid("Unknown Sanger call set"))
    }

    pub(crate) fn apply_session_edit(
        &mut self,
        id: &str,
        op: &ReviewOp,
    ) -> crate::Result<crate::session::Inverse> {
        use crate::Error::Invalid;
        let set = self.call_set_mut(id)?;
        let change = set.prepare_change(op).map_err(Invalid)?;
        let events = set.events.len();
        set.install_change(&change, true).map_err(Invalid)?;
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
            return Err(unsupported("undo of the wrong kind of edit"));
        };
        let set = self.call_set_mut(call_set)?;
        set.install_change(change, false)
            .map_err(crate::Error::Unsupported)?;
        set.events.truncate(*events);
        Ok(())
    }
}
