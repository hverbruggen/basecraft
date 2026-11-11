//! Immutable owned file bytes. A source can be shared by parsers and recovery
//! without copying; external truncation cannot invalidate its address space.

use std::fs::File;
use std::path::Path;

use crate::Result;

#[derive(Clone)]
pub struct Source(std::sync::Arc<Vec<u8>>);

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        Self::read(path, |_| {})
    }

    /// Hash each block as it arrives, without a second pass over the full file.
    pub fn open_hashed(path: &Path) -> Result<(Self, String)> {
        let mut hasher = crate::Hasher::new();
        let source = Self::read(path, |bytes| hasher.update(bytes))?;
        Ok((source, hasher.finish()))
    }

    fn read(path: &Path, mut consume: impl FnMut(&[u8])) -> Result<Self> {
        use std::io::Read;
        let mut file = File::open(path)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| std::io::Error::other("input is too large for this platform"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(std::io::Error::other)?;
        let mut block = [0; 16 * 1024];
        loop {
            let count = match file.read(&mut block) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            bytes
                .try_reserve_exact(count)
                .map_err(std::io::Error::other)?;
            consume(&block[..count]);
            bytes.extend_from_slice(&block[..count]);
        }
        #[cfg(feature = "test-counters")]
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
            #[cfg(feature = "test-counters")]
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
