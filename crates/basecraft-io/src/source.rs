//! File contents as a byte slice: memory-mapped for large files, read into
//! memory for small ones.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

use crate::Result;

/// Files below this size are read into memory instead of mapped. Copying a
/// few MB is cheap, and an owned copy can't fault if the file is truncated.
pub const MAP_THRESHOLD: u64 = 4 << 20;

pub enum Source {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        // User-owned files may be truncated by another process. Never map them:
        // a read-only mmap can still raise SIGBUS and cannot yield a Rust error.
        // Fallible reservation also reports oversized inputs instead of aborting.
        use std::io::Read;
        let mut file = File::open(path)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| std::io::Error::other("input is too large for this platform"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|e| std::io::Error::other(format!("cannot allocate input buffer: {e}")))?;
        file.read_to_end(&mut bytes)?;
        #[cfg(feature = "test-counters")]
        crate::counters::record_read(path, bytes.len() as u64);
        Ok(Source::Owned(bytes))
    }

    /// Read once and hash the exact owned bytes returned to the caller.
    pub fn open_hashed(path: &Path) -> Result<(Self, String)> {
        let source = Self::open(path)?;
        let mut hasher = crate::Hasher::new();
        hasher.update(source.bytes());
        let hash = hasher.finish();
        Ok((source, hash))
    }

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Source::Owned(bytes)
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Source::Mapped(m) => m,
            Source::Owned(v) => v,
        }
    }

    pub fn is_mapped(&self) -> bool {
        matches!(self, Source::Mapped(_))
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
            #[cfg(feature = "test-counters")]
            crate::counters::reset();
            let (source, hash) = Source::open_hashed(&path).unwrap();
            assert!(!source.is_mapped());
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
        let threshold = usize::try_from(MAP_THRESHOLD).unwrap();
        std::fs::write(&path, vec![b'A'; threshold + 1]).unwrap();
        let source = Source::open(&path).unwrap();
        assert!(!source.is_mapped());
        std::fs::write(path, b"").unwrap();
        assert_eq!(source.bytes().len(), threshold + 1);
        assert_eq!(source.bytes()[threshold], b'A');
    }
}
