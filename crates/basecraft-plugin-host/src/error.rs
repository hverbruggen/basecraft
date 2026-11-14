#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error(transparent)]
    Ops(#[from] basecraft_ops::Error),
    #[error("plugin manifest {path}: {message}")]
    Manifest { path: String, message: String },
    #[error("plugin {0} exited before connecting")]
    Exited(String),
    #[error("plugin {0} didn't connect in time")]
    Timeout(String),
    #[error("plugin error {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("plugin protocol: {0}")]
    Protocol(String),
    #[error("plugin protocol: {0}")]
    Contract(#[from] crate::contract::ContractError),
    /// A refusal callers act on by its code; the message says why, for people.
    #[error("{}", if message.is_empty() { code.as_str() } else { message.as_str() })]
    Refused {
        code: basecraft_protocol::operation::OperationErrorCode,
        message: String,
    },
}

impl Error {
    pub fn refused(
        code: basecraft_protocol::operation::OperationErrorCode,
        message: String,
    ) -> Self {
        Self::Refused { code, message }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<basecraft_io::Error> for Error {
    fn from(error: basecraft_io::Error) -> Self {
        Self::Ops(error.into())
    }
}
