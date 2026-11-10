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
    fn revision(&self, generation: u64) -> u64 {
        generation
    }

    fn fingerprint(&self) -> DocState {
        DocState {
            len: self.rows.len() as u64,
            features: self.frames.len() as u64,
            rows: Some(self.accessions.len() as u64),
        }
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

    fn saved_hashes(&self, _target: &Path) -> Result<Vec<String>> {
        Ok(vec![basecraft_io::hash_bytes(&crate::submission::encode(
            self,
        )?)])
    }
}
