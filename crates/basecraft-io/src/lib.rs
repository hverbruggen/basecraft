//! Format readers and writers, indexes and memory mapping.

pub mod cache;
pub mod fasta;
pub mod genbank;
pub mod gff3;
pub mod source;

mod error;

pub use error::{Error, Result};
pub use source::Source;

use std::io::Write;
use std::path::Path;

/// Write a file atomically: stream into a temporary file next to it, then
/// rename. Readers and sync tools never see a half-written file.
pub fn write_atomic(
    path: &Path,
    f: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    {
        let mut w = std::io::BufWriter::with_capacity(1 << 20, tmp.as_file_mut());
        f(&mut w)?;
        w.flush()?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

/// BLAKE3 hash of a file's content, as `b3:<hex>`.
pub fn hash_file(path: &Path) -> Result<String> {
    let source = Source::open(path)?;
    Ok(hash_bytes(source.bytes()))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("b3:{}", blake3::hash(bytes).to_hex())
}
