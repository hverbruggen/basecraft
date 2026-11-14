use crate::Result;
use basecraft_ops::{records::ArtifactRun, records::Mutation, sanger_review::CapturedInput};
use basecraft_protocol::provenance::FileStamp;
use std::path::Path;

/// Copy `source` for the run and return its hash, with the stamp it had
/// when read (a captured input's, when it was captured).
pub(super) fn freeze(
    source: &Path,
    destination: &Path,
    captures: &[CapturedInput],
    history: Option<&mut basecraft_ops::records::DocumentRecord>,
    project: Option<&basecraft_ops::project::Project>,
) -> Result<(String, Option<FileStamp>)> {
    let canonical = source.canonicalize()?;
    if let Some(capture) = captures.iter().find(|c| c.path() == canonical) {
        capture.write(destination)?;
        Ok((capture.hash().into(), Some(capture.stamp().clone())))
    } else {
        let (hash, stamp) = crate::file_tool::freeze_file(source, destination)?;
        if let Some(history) = history {
            // A file basecraft cannot open has no history to carry.
            match basecraft_ops::records::DocumentRecord::from_file(source, Some(&hash), project) {
                Ok(record) => history.include_input(&record),
                Err(basecraft_ops::Error::ChangedOnDisk { .. }) => {
                    return Err(crate::Error::Protocol(
                        "source changed while capturing its history".into(),
                    ));
                }
                Err(_) => {}
            }
        }
        Ok((hash, Some(stamp)))
    }
}

/// The input record of one `frozen` input, referencing its source. The
/// stamp it had when frozen shows whether it changed since.
pub(super) fn register(
    mutation: &Mutation,
    anchor: &Path,
    frozen: &super::prepare::FrozenSource,
    origin: &str,
    project: Option<&basecraft_ops::project::Project>,
) -> Result<ArtifactRun> {
    Ok(mutation.reference_input_with_dependencies(
        anchor,
        &frozen.source,
        &frozen.hash,
        frozen.stamp.as_ref(),
        &[],
        origin,
        project,
    )?)
}
