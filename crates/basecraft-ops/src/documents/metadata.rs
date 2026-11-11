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

    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != Format::Metadata {
            return Err(crate::Error::Unsupported(format!(
                "project metadata is saved as {}",
                crate::metadata::MANIFEST
            )));
        }
        let encoded = crate::metadata::encode(&self.metadata)?;
        let paths = crate::metadata::files(target);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Tables first, manifest last. Keep every completed file on failure so
        // the shared save owner can accept matching hashes and safely retry.
        for index in (0..paths.len()).rev() {
            if let Err(error) = basecraft_io::write_atomic(&paths[index], |writer| {
                writer.write_all(&encoded[index])
            }) {
                if index + 1 == paths.len() {
                    return Err(error.into());
                }
                let replaced = paths[index + 1..]
                    .iter()
                    .rev()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(crate::Error::Damaged {
                    path: target.into(),
                    detail: format!(
                        "partial metadata save: replaced {replaced}; failed at {}: {error}. Metadata generation is incomplete. Keep the open document and retry saving after fixing the destination, or save to a new folder. Temporary recovery, when enabled, is at {}",
                        paths[index].display(),
                        recovery_root.display()
                    ),
                });
            }
        }
        Ok(paths
            .into_iter()
            .zip(encoded.iter().map(|bytes| basecraft_io::hash_bytes(bytes)))
            .collect())
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

#[cfg(test)]
mod publication_tests;
