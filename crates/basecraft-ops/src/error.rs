use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Format(#[from] basecraft_io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("don't know the file format of {0}")]
    UnknownFormat(PathBuf),
    #[error("{0} contains no records")]
    Empty(PathBuf),
    #[error("invalid operation: {0}")]
    Invalid(String),
    #[error("{0}")]
    EvidenceConflict(Box<crate::input_evidence::EvidenceConflict>),
    #[error("evidence_check_incomplete: could not check all analysis evidence")]
    EvidenceCheckIncomplete(Box<crate::input_evidence::EvidenceCheck>),
    #[error("evidence_check_incomplete: explicit save decision required")]
    SaveCheckRequired(Box<crate::session::SaveCheck>),
    #[error("save_decision_stale: review the refreshed save check")]
    SaveDecisionStale(Box<crate::session::SaveCheck>),
    #[error("nothing to {0}")]
    NothingTo(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}
