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
    fn revision(&self, generation: u64) -> u64 {
        generation
    }

    fn fingerprint(&self) -> DocState {
        DocState {
            len: self.metadata.specimens.rows.len() as u64,
            features: self.metadata.identifications.len() as u64,
            rows: Some(self.metadata.links.len() as u64),
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let Op::MetadataEdit { edit } = op else {
            return Err(invalid(format!(
                "{} can't be applied to the project metadata",
                op.name()
            )));
        };
        // Retain the existing snapshot inverse. Edit-sized metadata inverses
        // remain separate debt; this migration does not change snapshot policy.
        let before = Box::new(self.metadata.clone());
        basecraft_docs::metadata::apply(&mut self.metadata, edit).map_err(|e| invalid(e.0))?;
        Ok((Inverse::Metadata(before), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Metadata(before) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        self.metadata = (**before).clone();
        Ok(())
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
    ) -> Result<Vec<(PathBuf, String)>> {
        let mut hashes = Vec::new();
        if format != Format::Metadata {
            return Err(invalid(format!(
                "project metadata is saved as {}",
                crate::metadata::MANIFEST
            )));
        }
        let encoded = crate::metadata::encode(&self.metadata)?;
        let paths = crate::metadata::files(target);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Tables first, manifest last: the manifest records their hashes.
        let mut replaced = Vec::new();
        for (path, bytes) in paths.iter().zip(&encoded).rev() {
            if let Err(error) = basecraft_io::write_atomic(path, |w| w.write_all(bytes)) {
                if replaced.is_empty() {
                    return Err(error.into());
                }
                return Err(invalid(format!(
                    "partial metadata save: replaced {}; failed at {}: {error}. Metadata generation is incomplete. Keep the open document and retry saving after fixing the destination, or save to a new folder. Temporary recovery, when enabled, is at {}",
                    replaced.join(", "),
                    path.display(),
                    recovery_root.display()
                )));
            }
            replaced.push(path.display().to_string());
        }
        for (path, bytes) in paths.iter().zip(&encoded) {
            hashes.push((path.clone(), basecraft_io::hash_bytes(bytes)));
        }
        Ok(hashes)
    }

    fn saved_hashes(&self, _target: &Path) -> Result<Vec<String>> {
        Ok(crate::metadata::encode(&self.metadata)?
            .iter()
            .map(|bytes| basecraft_io::hash_bytes(bytes))
            .collect())
    }
}
