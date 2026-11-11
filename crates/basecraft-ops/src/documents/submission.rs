use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::submission::Submission;
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::Format;
use crate::session::Inverse;

pub(crate) fn companions(_target: &Path) -> Vec<PathBuf> {
    Vec::new()
}

impl Document for Submission {
    fn accepts(&self, op: &Op) -> bool {
        matches!(op, Op::SubmissionEdit { .. })
    }

    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Submission
    }

    fn retains_companions(&self) -> bool {
        true
    }
    fn revision(&self, generation: u64) -> u64 {
        generation
    }

    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Submission {
            records: self.rows.len() as u64,
            frames: self.frames.len() as u64,
            accessions: self.accessions.len() as u64,
        })
    }

    fn validate(&self, op: &Op, root: &Path, live: bool) -> Result<()> {
        if live
            && let Op::SubmissionEdit { edit } = op
            && let basecraft_protocol::submission::SubmissionEdit::ApplyInclusion { plan } =
                edit.as_ref()
        {
            crate::submission::verify_inclusion(root, self, plan)?;
        }
        Ok(())
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let Op::SubmissionEdit { edit } = op else {
            return Err(invalid(format!(
                "{} can't be applied to a submission",
                op.name()
            )));
        };
        let before = self.clone();
        basecraft_docs::submission::apply(self, edit).map_err(|e| invalid(e.0))?;
        let undo = crate::session::SubmissionUndo::between(before, self);
        Ok((Inverse::Submission(Box::new(undo)), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Submission(undo) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        undo.undo(self);
        Ok(())
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != Format::Submission {
            return Err(invalid(format!(
                "a submission is saved as *{}",
                crate::submission::SUFFIX
            )));
        }
        let bytes = crate::submission::encode(self)?;
        basecraft_io::write_atomic(target, |w| w.write_all(&bytes))?;
        Ok(vec![(
            target.to_path_buf(),
            basecraft_io::hash_bytes(&bytes),
        )])
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        Ok(vec![basecraft_io::hash_bytes(&crate::submission::encode(
            self,
        )?)])
    }
}
