//! Immutable owned file bytes. A source can be shared by parsers and recovery
//! without copying; external truncation cannot invalidate its address space.

use std::fs::File;
use std::path::Path;

use crate::Result;

#[derive(Clone)]
pub struct Source(std::sync::Arc<Vec<u8>>);

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        Self::read(path, |_| Ok(()))
    }

    /// Hash each block as it arrives, without a second pass over the full file.
    pub fn open_hashed(path: &Path) -> Result<(Self, String)> {
        let mut hasher = crate::Hasher::new();
        let source = Self::read(path, |bytes| {
            hasher.update(bytes);
            Ok(())
        })?;
        Ok((source, hasher.finish()))
    }

    /// Bounded capture with BLAKE3 and SHA-256 computed in the same read.
    pub fn open_hashed_with_sha256(path: &Path, limit: u64) -> Result<(Vec<u8>, String, String)> {
        use sha2::{Digest, Sha256};
        let mut hasher = crate::Hasher::new();
        let mut sha = Sha256::new();
        let mut total = 0_u64;
        let source = Self::read(path, |bytes| {
            total += bytes.len() as u64;
            if total > limit {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "input exceeds capture limit",
                )
                .into());
            }
            hasher.update(bytes);
            sha.update(bytes);
            Ok(())
        })?;
        let bytes = std::sync::Arc::try_unwrap(source.0).unwrap_or_else(|shared| (*shared).clone());
        Ok((bytes, hasher.finish(), format!("{:x}", sha.finalize())))
    }

    /// Read, hash and index each FASTA block while it is hot in memory.
    pub fn open_fasta(path: &Path) -> Result<(Self, crate::fasta::FastaIndex, String)> {
        let mut hasher = crate::Hasher::new();
        let mut index = crate::fasta::IndexBuilder::default();
        let source = Self::read(path, |bytes| {
            hasher.update(bytes);
            index.push(bytes)
        })?;
        Ok((source, index.finish()?, hasher.finish()))
    }

    fn read(path: &Path, mut consume: impl FnMut(&[u8]) -> Result<()>) -> Result<Self> {
        use std::io::Read;
        let mut file = File::open(path)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| std::io::Error::other("input is too large for this platform"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(std::io::Error::other)?;
        // Read straight into the buffer, a block at a time, so `consume`
        // sees each block while it is still in cache.
        loop {
            let start = bytes.len();
            if (&mut file).take(1 << 20).read_to_end(&mut bytes)? == 0 {
                break;
            }
            consume(&bytes[start..])?;
        }
        crate::counters::record_read(path, bytes.len() as u64);
        Ok(Self(std::sync::Arc::new(bytes)))
    }

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Source(std::sync::Arc::new(bytes))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hashed_source_matches_owned_bytes_after_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source");
        for bytes in [b"".as_slice(), b"ACGT".as_slice()] {
            std::fs::write(&path, bytes).unwrap();
            crate::counters::reset();
            let (source, hash) = Source::open_hashed(&path).unwrap();
            assert_eq!(hash, crate::hash_bytes(bytes));
            #[cfg(feature = "test-counters")]
            {
                let counts = crate::counters::snapshot();
                assert_eq!(counts.unresolved_reads, 0);
                assert_eq!(
                    counts.reads[&path.canonicalize().unwrap()],
                    crate::counters::Reads {
                        full_reads: 1,
                        bytes: bytes.len() as u64,
                    }
                );
            }
            std::fs::write(&path, b"replacement").unwrap();
            assert_eq!(source.bytes(), bytes);
            assert_eq!(hash, crate::hash_bytes(source.bytes()));
        }
        assert!(Source::open_hashed(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn externally_truncated_large_input_keeps_safe_owned_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large");
        let threshold = 4 << 20;
        std::fs::write(&path, vec![b'A'; threshold + 1]).unwrap();
        let source = Source::open(&path).unwrap();
        std::fs::write(path, b"").unwrap();
        assert_eq!(source.bytes().len(), threshold + 1);
        assert_eq!(source.bytes()[threshold], b'A');
    }
}
