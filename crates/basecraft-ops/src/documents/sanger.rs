//! Sanger decisions use the same journal, undo stacks and save guards as other kinds.
use crate::error::unsupported;
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
        matches!(
            op,
            Op::SangerReview { .. } | Op::SangerUpdate { .. } | Op::SangerSelect { .. }
        )
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
    fn validate(&self, op: &Op, _root: &Path, live: bool) -> Result<()> {
        if live
            && let Op::SangerReview { edit, .. } = op
            && let basecraft_protocol::sanger::ReviewOp::Export { path, sha256 } = edit.as_ref()
            && basecraft_io::sha256_file(path)? != *sha256
        {
            return Err(Error::ChangedOnDisk { path: path.clone() });
        }
        Ok(())
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        if let Op::SangerSelect { call_set } = op {
            let before = self.draft_sha256.clone();
            self.select_call_set(call_set)?;
            return Ok((Inverse::SangerSelect(before), None));
        }
        if let Op::SangerUpdate { review } = op {
            let review = crate::sanger_review::read_journal_record(review)?;
            let mut replacement =
                SangerReview::from_session_snapshot(&review, self.storage_anchor())?;
            replacement.share_captures(self);
            return Ok((
                Inverse::SangerUpdate(Box::new(std::mem::replace(self, replacement))),
                None,
            ));
        }
        let Op::SangerReview { call_set, edit } = op else {
            return Err(unsupported(format!(
                "{} cannot be applied to a Sanger review",
                op.name()
            )));
        };
        self.apply_session_edit(call_set, edit)
            .map(|inverse| (inverse, None))
    }
    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        if let Inverse::SangerSelect(before) = inverse {
            return self.select_call_set(before);
        }
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
    fn prepare_write(
        &self,
        project: Option<&crate::project::Project>,
        target: &Path,
        format: Format,
        _recovery: &Path,
        publication: &WritePublication,
    ) -> Result<super::PreparedWrite> {
        if format != Format::SangerReview {
            return Err(unsupported("a Sanger review is saved as JSON"));
        }
        self.prepare_session_review(project, target, &publication.logical(target)?, publication)
    }
    fn finish_write(
        &self,
        project: Option<&crate::project::Project>,
        target: &Path,
        _hashes: &[(PathBuf, String)],
        run: Option<&crate::records::analysis_store::ArtifactRun>,
        _publication: &WritePublication,
    ) -> Result<()> {
        run.map_or(Ok(()), |run| {
            SangerReview::bind_session_review(project, target, run)
        })
    }
    fn saved_hashes(&self, target: &Path, publication: &WritePublication) -> Result<Vec<String>> {
        Ok(vec![basecraft_io::hash_bytes(
            &self.session_bytes(&publication.logical(target)?)?,
        )])
    }
}
