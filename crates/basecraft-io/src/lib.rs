//! Format readers and writers, indexes and memory mapping.

pub mod abif;
pub mod archive;
pub mod cache;
#[cfg(feature = "test-counters")]
pub mod counters;
#[cfg(feature = "test-heap")]
pub mod heap;
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
    #[cfg(feature = "test-counters")]
    counters::record_sync_all();
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

/// BLAKE3 hash of a file's content, as `b3:<hex>`.
pub fn hash_file(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    hash_reader(
        file,
        #[cfg(feature = "test-counters")]
        path,
    )
}

const HASH_BUFFER_SIZE: usize = 1 << 20;

fn hash_reader(
    mut reader: impl Read,
    #[cfg(feature = "test-counters")] path: &Path,
) -> Result<String> {
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    #[cfg(feature = "test-counters")]
    let mut bytes_read = 0;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        hasher.update(&buffer[..count]);
        #[cfg(feature = "test-counters")]
        {
            bytes_read += count as u64;
        }
    }
    #[cfg(feature = "test-counters")]
    counters::record_read(path, bytes_read);
    Ok(hasher.finish())
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
    #[cfg(feature = "test-counters")]
    let mut bytes_read = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        #[cfg(feature = "test-counters")]
        {
            bytes_read += count as u64;
        }
        digest.update(&buffer[..count]);
    }
    #[cfg(feature = "test-counters")]
    counters::record_read(path, bytes_read);
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

#[cfg(test)]
mod hash_tests {
    use super::*;
    use std::io::{self, Cursor};

    #[test]
    fn file_hash_matches_bytes_at_buffer_boundaries() {
        let file = tempfile::NamedTempFile::new().unwrap();
        for size in [
            0,
            1,
            127,
            HASH_BUFFER_SIZE - 1,
            HASH_BUFFER_SIZE,
            HASH_BUFFER_SIZE + 1,
            3 * HASH_BUFFER_SIZE + 17,
        ] {
            let bytes: Vec<u8> = (0..size).map(|i| i.to_le_bytes()[0]).collect();
            std::fs::write(file.path(), &bytes).unwrap();
            #[cfg(feature = "test-counters")]
            counters::reset();
            assert_eq!(hash_file(file.path()).unwrap(), hash_bytes(&bytes));
            #[cfg(feature = "test-counters")]
            {
                let counts = counters::snapshot();
                assert_eq!(counts.reads.len(), 1);
                assert_eq!(
                    counts.reads[&file.path().canonicalize().unwrap()],
                    counters::Reads {
                        full_reads: 1,
                        bytes: size as u64
                    }
                );
                assert_eq!(counts.unresolved_reads, 0);
            }
        }
    }

    struct InterruptedReader {
        bytes: Cursor<Vec<u8>>,
        interrupt: bool,
        fail_at_end: bool,
    }

    impl Read for InterruptedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            assert_eq!(buffer.len(), HASH_BUFFER_SIZE);
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.fail_at_end && self.bytes.position() == self.bytes.get_ref().len() as u64 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            // Exercise short reads as well as interruptions before data and EOF.
            self.bytes.read(&mut buffer[..13])
        }
    }

    #[test]
    fn interrupted_short_reads_retry_and_partial_errors_do_not_count() {
        #[cfg(feature = "test-counters")]
        let file = tempfile::NamedTempFile::new().unwrap();
        for fail_at_end in [false, true] {
            let bytes = b"deterministic content spanning several short reads".to_vec();
            let reader = InterruptedReader {
                bytes: Cursor::new(bytes.clone()),
                interrupt: false,
                fail_at_end,
            };
            #[cfg(feature = "test-counters")]
            counters::reset();
            let result = hash_reader(
                reader,
                #[cfg(feature = "test-counters")]
                file.path(),
            );
            if fail_at_end {
                assert!(matches!(result, Err(Error::Io(error))
                    if error.kind() == io::ErrorKind::PermissionDenied));
                #[cfg(feature = "test-counters")]
                assert_eq!(counters::snapshot(), counters::Counters::default());
            } else {
                assert_eq!(result.unwrap(), hash_bytes(&bytes));
                #[cfg(feature = "test-counters")]
                assert_eq!(
                    counters::snapshot().reads[&file.path().canonicalize().unwrap()],
                    counters::Reads {
                        full_reads: 1,
                        bytes: bytes.len() as u64
                    }
                );
            }
        }
    }

    #[test]
    fn missing_file_preserves_io_error() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(feature = "test-counters")]
        counters::reset();
        assert!(
            matches!(hash_file(&dir.path().join("missing")), Err(Error::Io(error))
            if error.kind() == io::ErrorKind::NotFound)
        );
        #[cfg(feature = "test-counters")]
        assert_eq!(counters::snapshot(), counters::Counters::default());
    }

    #[cfg(unix)]
    #[test]
    fn directory_read_fails_without_counting_success() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(feature = "test-counters")]
        counters::reset();
        assert!(matches!(hash_file(dir.path()), Err(Error::Io(_))));
        #[cfg(feature = "test-counters")]
        assert_eq!(counters::snapshot(), counters::Counters::default());
    }
}
