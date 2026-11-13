use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::Format;
use crate::metadata::OpenedMetadata;
use crate::session::Inverse;

impl Document for OpenedMetadata {
    fn accepts(&self, op: &Op) -> bool {
        matches!(op, Op::MetadataEdit { .. })
    }

    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Metadata
    }

    fn retains_companions(&self) -> bool {
        true
    }
    fn revision(&self, generation: u64) -> u64 {
        generation
    }

    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Metadata {
            specimens: self.metadata.specimens.rows.len() as u64,
            identifications: self.metadata.identifications.len() as u64,
            links: self.metadata.links.len() as u64,
        })
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let Op::MetadataEdit { edit } = op else {
            return Err(invalid(format!(
                "{} can't be applied to the project metadata",
                op.name()
            )));
        };
        let before = self.metadata.clone();
        basecraft_docs::metadata::apply(&mut self.metadata, edit).map_err(|e| invalid(e.0))?;
        let undo = crate::session::MetadataUndo::between(before, &self.metadata);
        Ok((Inverse::Metadata(Box::new(undo)), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Metadata(undo) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        undo.undo(&mut self.metadata);
        Ok(())
    }

    fn independently_saved_files(&self, target: &Path) -> Vec<PathBuf> {
        crate::metadata::files(target)
    }

    fn prepare_write(
        &self,
        _project: Option<&crate::project::Project>,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<super::PreparedWrite> {
        if format != Format::Metadata {
            return Err(crate::Error::Unsupported(format!(
                "project metadata is saved as {}",
                crate::metadata::MANIFEST
            )));
        }
        std::fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))?;
        let mut prepared = super::PreparedWrite {
            reverse: true,
            ..Default::default()
        };
        prepared.check_record::<basecraft_protocol::metadata_records::MetadataManifest>(target);
        for (path, bytes) in crate::metadata::files(target)
            .into_iter()
            .zip(crate::metadata::encode(&self.metadata)?)
        {
            prepared.push(&path, |w| w.write_all(&bytes))?;
        }
        Ok(prepared)
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        Ok(crate::metadata::encode(&self.metadata)?
            .iter()
            .map(|bytes| basecraft_io::hash_bytes(bytes))
            .collect())
    }
}
