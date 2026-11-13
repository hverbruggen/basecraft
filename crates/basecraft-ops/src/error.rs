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
    #[error("proposal target changed since revision {revision}")]
    StaleRevision { id: crate::DocId, revision: u64 },
    #[error("{path} changed on disk")]
    ChangedOnDisk { path: PathBuf },
    #[error("{path} was not found")]
    NotFound { path: PathBuf },
    #[error("{path}: {detail}")]
    Damaged { path: PathBuf, detail: String },
    #[error("{what} exceeds the limit of {limit}")]
    Limit { what: String, limit: u64 },
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Busy(String),
    /// Edits name their record, so a file whose records share names can't
    /// be edited until they are numbered or renamed.
    #[error(
        "records in this file share the names {}; number them (.1, .2) or rename them by hand before editing",
        .0.join(", ")
    )]
    RecordNamesRepeat(Vec<String>),
    #[error("invalid operation: {0}")]
    Invalid(String),
    #[error("{0}")]
    EvidenceConflict(Box<crate::records::input_evidence::EvidenceConflict>),
    #[error("evidence_check_incomplete: could not check all analysis evidence")]
    EvidenceCheckIncomplete(Box<crate::records::input_evidence::EvidenceCheck>),
    #[error("evidence_check_incomplete: explicit save decision required")]
    SaveCheckRequired(Box<crate::session::SaveCheck>),
    #[error("save_decision_stale: review the refreshed save check")]
    SaveDecisionStale(Box<crate::session::SaveCheck>),
    #[error(transparent)]
    PublishedWithFailure(Box<crate::records::replay::PublishedWithFailure>),
    #[error("nothing to {0}")]
    NothingTo(&'static str),
    /// D13: the first edit needs recovery in the folder's `.basecraft`.
    #[error(
        "{} is read-only for basecraft ({reason}), so unsaved edits could not be recovered. \
         Copy the file to a folder you can write to (for example with `cp` or your file manager) and edit the copy.",
        folder.display()
    )]
    ReadOnlyFolder { folder: PathBuf, reason: String },
    /// A file a request names whose folder cannot be found or read.
    #[error("{}: {reason}", folder.display())]
    FolderUnavailable { folder: PathBuf, reason: String },
    #[error("{} names no file", .0.display())]
    NoFileName(PathBuf),
    /// A Sanger decision or column as a person wrote it, not understood.
    #[error(transparent)]
    ReviewInput(#[from] crate::sanger_commands::InputError),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

impl Error {
    /// The single mapping shared by CLI, MCP, and registered operation jobs.
    pub fn operation_code(&self) -> basecraft_protocol::operation::OperationErrorCode {
        use basecraft_protocol::operation::OperationErrorCode as Code;
        match self {
            Self::StaleRevision { .. } => Code::StaleRevision,
            Self::ChangedOnDisk { .. } => Code::ChangedOnDisk,
            Self::NotFound { .. } | Self::FolderUnavailable { .. } => Code::NotFound,
            Self::Damaged { .. } => Code::Damaged,
            Self::Limit { .. } => Code::Limit,
            Self::Unsupported(_) => Code::Unsupported,
            Self::RecordNamesRepeat(_) => Code::RecordNamesRepeat,
            Self::Busy(_) => Code::Busy,
            Self::UnknownDocument { .. } => Code::UnknownDocument,
            Self::WrongKind { .. } => Code::WrongDocumentKind,
            Self::Access(crate::access::AccessError::Denied(_)) => Code::PermissionDenied,
            Self::Access(crate::access::AccessError::DestinationExists) => Code::OutputConflict,
            Self::Access(crate::access::AccessError::Io(e))
            | Self::Io(e)
            | Self::Format(basecraft_io::Error::Io(e)) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Code::PermissionDenied
                } else {
                    Code::IoError
                }
            }
            Self::Format(_) => Code::FormatError,
            Self::Json(_) | Self::Invalid(_) | Self::NoFileName(_) | Self::ReviewInput(_) => {
                Code::InvalidArguments
            }
            Self::UnknownFormat(_) => Code::UnknownFormat,
            Self::Empty(_) => Code::EmptyDocument,
            Self::EvidenceConflict(_) => Code::AnalysisInputConflict,
            Self::EvidenceCheckIncomplete(_) | Self::SaveCheckRequired(_) => {
                Code::EvidenceCheckIncomplete
            }
            Self::SaveDecisionStale(_) => Code::SaveDecisionStale,
            Self::PublishedWithFailure(_) => Code::PublishedWithFailure,
            Self::NothingTo(_) => Code::NothingToDo,
            Self::ReadOnlyFolder { .. } => Code::ReadOnlyFolder,
        }
    }

    /// Stable wire spelling; messages are for people, not dispatch.
    pub fn code(&self) -> &'static str {
        self.operation_code().as_str()
    }
}
