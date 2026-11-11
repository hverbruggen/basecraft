use crate::Result;
use basecraft_ops::{
    analysis_store::ArtifactRun, input_evidence::Mutation, sanger_review::CapturedInput,
};
use std::path::Path;

pub(super) fn freeze(
    source: &Path,
    destination: &Path,
    captures: &[CapturedInput],
    history: Option<&mut basecraft_ops::document_record::DocumentRecord>,
) -> Result<String> {
    let canonical = source.canonicalize()?;
    if let Some(capture) = captures.iter().find(|c| c.path() == canonical) {
        capture.write(destination)?;
        Ok(capture.hash().into())
    } else {
        let hash = crate::file_tool::freeze_file(source, destination)?;
        if let Some(history) = history
            && let Ok(record) = basecraft_ops::document_record::DocumentRecord::from_file(source)
        {
            history.include_input(&record);
            if basecraft_io::hash_file(source).map_err(basecraft_ops::Error::from)? != hash {
                return Err(crate::Error::Protocol(
                    "source changed while capturing its history".into(),
                ));
            }
        }
        Ok(hash)
    }
}
pub(super) fn register(
    mutation: &Mutation,
    anchor: &Path,
    source: &Path,
    hash: &str,
    captures: &[CapturedInput],
    origin: &str,
) -> Result<ArtifactRun> {
    let canonical = source.canonicalize()?;
    Ok(
        if let Some(capture) = captures.iter().find(|c| c.path() == canonical) {
            if capture.hash() != hash {
                return Err(crate::Error::Protocol(
                    "captured input identity differs".into(),
                ));
            }
            mutation.reference_captured(anchor, capture, &[], origin)?
        } else {
            mutation.reference_input_with_dependencies(anchor, source, hash, &[], origin)?
        },
    )
}
