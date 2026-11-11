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
        Self::read(path, u64::MAX, false, false).map(|(bytes, _, _)| Self::Owned(bytes))
    }
    /// Read and hash the exact owned bytes returned to the parser.
    pub fn open_hashed(path: &Path) -> Result<(Self, String)> {
        let (bytes, hash, _) = Self::read(path, u64::MAX, true, false)?;
        Ok((Self::Owned(bytes), hash.expect("requested BLAKE3")))
    }
    /// Bounded capture with both digests computed in the same read pass.
    pub fn open_hashed_with_sha256(path: &Path, limit: u64) -> Result<(Vec<u8>, String, String)> {
        let (bytes, hash, sha) = Self::read(path, limit, true, true)?;
        Ok((
            bytes,
            hash.expect("requested BLAKE3"),
            sha.expect("requested SHA-256"),
        ))
    }
    fn read(
        path: &Path,
        limit: u64,
        hashes: bool,
        sha256: bool,
    ) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        if size > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "input exceeds capture limit",
            )
            .into());
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(usize::try_from(size).map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
        let mut b3 = hashes.then(crate::Hasher::new);
        let mut sha = sha256.then(Sha256::new);
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            if bytes.len() as u64 + count as u64 > limit {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "input exceeds capture limit",
                )
                .into());
            }
            let chunk = &buffer[..count];
            if let Some(b3) = &mut b3 {
                b3.update(chunk);
            }
            if let Some(sha) = &mut sha {
                sha.update(chunk);
            }
            bytes.extend_from_slice(chunk);
        }
        #[cfg(feature = "test-counters")]
        crate::counters::record_read(path, bytes.len() as u64);
        Ok((
            bytes,
            b3.map(|hasher| hasher.finish()),
            sha.map(|sha| format!("{:x}", sha.finalize())),
        ))
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
