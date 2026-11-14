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
    /// D25: the alignment linked to a tree is not the file that was linked.
    #[error(
        "{} changed since it was linked to this tree. Link alignment again to use the current file, or choose another alignment",
        path.display()
    )]
    AlignmentChanged { path: PathBuf },
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
    /// D14: a save would drop fields a later basecraft wrote, in each of
    /// these files. Save As, or save again after `Session::drop_newer_fields`.
    #[error(
        "{} Basecraft {} does not know these fields, so saving would drop them. \
         Save as another file instead, or save anyway to drop them.",
        newer_files(.0),
        basecraft_protocol::VERSION
    )]
    NewerFieldsDropped(Vec<(PathBuf, basecraft_protocol::records::UnknownFields)>),
    /// Bad input that fits no other kind.
    #[error("invalid operation: {0}")]
    Invalid(String),
    /// A choice the request needs and has no default for: what to choose.
    #[error("{0}")]
    MissingChoice(String),
    /// The destination already holds something basecraft will not replace.
    #[error("{0}")]
    OutputConflict(String),
    /// An earlier step has to happen first (save, export, review, relink).
    #[error("{0}")]
    NotReady(String),
    /// What the request was prepared from has changed; prepare it again.
    #[error("{0}")]
    Outdated(String),
    /// The submission package was written, but the document recording the
    /// export could not be saved.
    #[error(
        "package written but export identities were not saved: {0}; keep this document open and retry saving before importing accessions"
    )]
    PackageNotRecorded(Box<Error>),
    #[error(transparent)]
    Preferences(#[from] basecraft_platform::settings::Error),
    #[error(transparent)]
    AppPaths(#[from] basecraft_platform::paths::PathError),
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
    /// Sequences that cannot join an alignment, and why.
    #[error(transparent)]
    Addition(#[from] crate::alignment_add::AdditionError),
    #[error(transparent)]
    DisplayTree(#[from] crate::delimitation::DisplayTreeError),
    #[error("This project has no specimen metadata")]
    NoMetadata,
    /// The work named needs a chosen project and there is none.
    #[error("{0} needs a project; choose one (on the command line, --project)")]
    NoProject(&'static str),
    #[error("{0} is not a label field; give specimen, genus, species, accession or column:NAME")]
    NotALabelField(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// "a.json was written by basecraft 0.6.0, with fields x, y." for each file.
fn newer_files(files: &[(PathBuf, basecraft_protocol::records::UnknownFields)]) -> String {
    files
        .iter()
        .map(|(path, unknown)| {
            format!(
                "{} was written by basecraft {}, with fields {}.",
                path.display(),
                unknown.writer,
                unknown.fields.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

pub(crate) fn damaged(path: impl Into<PathBuf>, detail: impl Into<String>) -> Error {
    Error::Damaged {
        path: path.into(),
        detail: detail.into(),
    }
}

impl Error {
    /// The single mapping shared by CLI, MCP, and registered operation jobs.
    pub fn operation_code(&self) -> basecraft_protocol::operation::OperationErrorCode {
        use basecraft_protocol::operation::OperationErrorCode as Code;
        match self {
            Self::StaleRevision { .. } | Self::Outdated(_) => Code::StaleRevision,
            Self::ChangedOnDisk { .. } | Self::AlignmentChanged { .. } => Code::ChangedOnDisk,
            Self::NotFound { .. } | Self::FolderUnavailable { .. } | Self::NoMetadata => {
                Code::NotFound
            }
            Self::Damaged { .. }
            | Self::Preferences(basecraft_platform::settings::Error::Invalid(_)) => Code::Damaged,
            Self::Preferences(basecraft_platform::settings::Error::Busy) => Code::Busy,
            Self::Preferences(basecraft_platform::settings::Error::Io(e)) => io_code(e),
            Self::AppPaths(_) => Code::Unsupported,
            Self::MissingChoice(_) => Code::MissingChoice,
            Self::OutputConflict(_) => Code::OutputConflict,
            Self::NotReady(_) => Code::NotReady,
            Self::PackageNotRecorded(_) => Code::PublicationIncomplete,
            Self::Limit { .. } => Code::Limit,
            Self::Unsupported(_) => Code::Unsupported,
            Self::RecordNamesRepeat(_) => Code::RecordNamesRepeat,
            Self::NewerFieldsDropped(_) => Code::NewerFieldsDropped,
            Self::Busy(_) => Code::Busy,
            Self::UnknownDocument { .. } => Code::UnknownDocument,
            Self::WrongKind { .. } => Code::WrongDocumentKind,
            Self::Access(crate::access::AccessError::Denied(_)) => Code::PermissionDenied,
            Self::Access(crate::access::AccessError::DestinationExists) => Code::OutputConflict,
            Self::Access(crate::access::AccessError::Io(e))
            | Self::Io(e)
            | Self::Format(basecraft_io::Error::Io(e)) => io_code(e),
            Self::Format(basecraft_io::Error::TooLarge(_)) => Code::Limit,
            Self::Format(_) => Code::FormatError,
            Self::Json(_)
            | Self::Invalid(_)
            | Self::NoFileName(_)
            | Self::ReviewInput(_)
            | Self::Addition(_)
            | Self::DisplayTree(
                crate::delimitation::DisplayTreeError::NotText(_)
                | crate::delimitation::DisplayTreeError::NotOneTree { .. },
            )
            | Self::NotALabelField(_) => Code::InvalidArguments,
            Self::NoProject(_)
            | Self::DisplayTree(crate::delimitation::DisplayTreeError::NoneAttached) => {
                Code::MissingChoice
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

fn io_code(error: &std::io::Error) -> basecraft_protocol::operation::OperationErrorCode {
    use basecraft_protocol::operation::OperationErrorCode as Code;
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        Code::PermissionDenied
    } else {
        Code::IoError
    }
}
