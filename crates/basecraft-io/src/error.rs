use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{format} parse error at line {line}: {message}")]
    Parse {
        format: &'static str,
        line: usize,
        message: String,
    },
    #[error("no record named {0:?}")]
    NoSuchRecord(String),
    #[error("range {start}..{end} is outside record {name:?} of length {len}")]
    OutOfRange {
        name: String,
        start: u64,
        end: u64,
        len: u64,
    },
    /// A table or spreadsheet that can't be read as one.
    #[error("{0}")]
    Table(String),
    /// A table or spreadsheet over basecraft's size limits for import.
    #[error("{0}")]
    TooLarge(String),
    #[error("index cache entry {0} is unreadable")]
    BadCache(PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn parse_error(format: &'static str, line: usize, message: impl Into<String>) -> Error {
    Error::Parse {
        format,
        line,
        message: message.into(),
    }
}
