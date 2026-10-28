//! Format readers and writers, indexes and memory mapping.

pub mod abif;
pub mod archive;
pub mod cache;
pub mod fasta;
pub mod genbank;
pub mod gff3;
pub mod ncbi;
pub mod newick;
pub mod source;
pub mod spart;
pub mod table;

mod error;

pub use error::{Error, Result};
pub use source::Source;

use std::io::{Read, Write};
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

/// SHA-256 of in-memory evidence bytes, as lowercase hexadecimal.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// SHA-256 used to verify Tracy draft provenance against its source AB1 files.
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Incremental BLAKE3, giving the same `b3:<hex>` form as [`hash_bytes`].
#[derive(Default)]
pub struct Hasher(blake3::Hasher);

impl Hasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn finish(&self) -> String {
        format!("b3:{}", self.0.finalize().to_hex())
    }
}
