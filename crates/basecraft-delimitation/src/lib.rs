//! Qt-independent species delimitation engines and preparation.
pub mod asap;
pub mod gmyc;
pub mod haplotypes;
pub mod time_tree;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;
