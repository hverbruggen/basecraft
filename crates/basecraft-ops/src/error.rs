use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Access(#[from] crate::access::AccessError),
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
    #[error("unknown document {id:?}")]
    UnknownDocument { id: crate::DocId },
    #[error("document {id:?} requires kind {expected:?}")]
    WrongKind {
        id: crate::DocId,
        expected: crate::files::Kind,
    },
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
    #[error(transparent)]
    PublishedWithFailure(Box<crate::replay::PublishedWithFailure>),
    #[error("nothing to {0}")]
    NothingTo(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

impl Error {
    /// Stable category for callers; message text is for people, not dispatch.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownDocument { .. } => "unknown_document",
            Self::WrongKind { .. } => "wrong_document_kind",
            Self::Access(crate::access::AccessError::Denied(_)) => "permission_denied",
            Self::Access(crate::access::AccessError::DestinationExists) => "output_conflict",
            Self::Access(crate::access::AccessError::Io(_)) | Self::Io(_) => "io",
            Self::Format(_) => "format",
            Self::Json(_) => "json",
            Self::UnknownFormat(_) => "unknown_format",
            Self::Empty(_) => "empty_document",
            Self::Invalid(_) => "invalid_operation",
            Self::EvidenceConflict(_) => "evidence_conflict",
            Self::EvidenceCheckIncomplete(_) | Self::SaveCheckRequired(_) => {
                "evidence_check_incomplete"
            }
            Self::SaveDecisionStale(_) => "save_decision_stale",
            Self::PublishedWithFailure(_) => "published_with_failure",
            Self::NothingTo(_) => "nothing_to_do",
        }
    }
}
