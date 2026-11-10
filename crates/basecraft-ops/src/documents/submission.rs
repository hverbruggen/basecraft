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
    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Submission
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

    fn prepare_edit(
        &self,
        op: &Op,
        root: &Path,
        live: bool,
    ) -> Result<Option<super::PreparedEdit>> {
        if live
            && let Op::SubmissionEdit { edit } = op
            && let basecraft_protocol::submission::SubmissionEdit::ApplyInclusion { plan } =
                edit.as_ref()
        {
            crate::submission::verify_inclusion(root, self, plan)?;
        }
        Ok(None)
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let Op::SubmissionEdit { edit } = op else {
            return Err(invalid(format!(
                "{} can't be applied to a submission",
                op.name()
            )));
        };
        // Preserve the existing whole-payload inverse. Submission carries
        // settings and record decisions, not sequence data. Replacing this
        // snapshot with edit-sized inverses is a separate migration.
        let before = Box::new(self.clone());
        basecraft_docs::submission::apply(self, edit).map_err(|e| invalid(e.0))?;
        Ok((Inverse::Submission(before), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Submission(before) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        *self = (**before).clone();
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
