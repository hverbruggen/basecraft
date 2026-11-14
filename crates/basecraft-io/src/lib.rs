//! Format readers and writers, indexes and immutable file sources.

pub mod abif;
pub mod archive;
pub mod counters;
pub mod fasta;
pub mod genbank;
pub mod gff3;
#[cfg(feature = "test-heap")]
pub mod heap;
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
    write_atomic_observed(path, f, |_| Ok(()), || {})
}

/// The staged callback borrows the actual candidate after flush/file sync and
/// before replacement. Its error prevents persist. The trusted, infallible
/// latch runs immediately after successful persist (never after a persist error).
/// No parent sync, identity query, rollback or whole-save guarantee is added.
pub fn write_atomic_observed(
    path: &Path,
    f: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    staged: impl FnOnce(&std::fs::File) -> std::io::Result<()>,
    persisted: impl FnOnce(),
) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    {
        let mut w = std::io::BufWriter::new(tmp.as_file_mut());
        f(&mut w)?;
        w.flush()?;
    }
    counters::record_sync_all();
    tmp.as_file().sync_all()?;
    staged(tmp.as_file())?;
    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    persisted();
    Ok(())
}

/// BLAKE3 hash of a file's content, as `b3:<hex>`.
pub fn hash_file(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    hash_reader(file, path, |_| {})
}

const HASH_BUFFER_SIZE: usize = 1 << 20;

/// BLAKE3 of what `reader` yields; `also` sees each block as well.
fn hash_reader(mut reader: impl Read, path: &Path, mut also: impl FnMut(&[u8])) -> Result<String> {
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    let mut bytes_read = 0;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        hasher.update(&buffer[..count]);
        also(&buffer[..count]);
        bytes_read += count as u64;
    }
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
    Ok(hash_file_with_sha256(path)?.1)
}

/// A file's BLAKE3 hash, as [`hash_file`], and its SHA-256 as lowercase
/// hexadecimal, in one read.
pub fn hash_file_with_sha256(path: &Path) -> Result<(String, String)> {
    use sha2::{Digest, Sha256};
    let mut sha = Sha256::new();
    let hash = hash_reader(std::fs::File::open(path)?, path, |block| sha.update(block))?;
    Ok((hash, format!("{:x}", sha.finalize())))
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
        let file = tempfile::NamedTempFile::new().unwrap();
        for fail_at_end in [false, true] {
            let bytes = b"deterministic content spanning several short reads".to_vec();
            let reader = InterruptedReader {
                bytes: Cursor::new(bytes.clone()),
                interrupt: false,
                fail_at_end,
            };
            counters::reset();
            let result = hash_reader(reader, file.path(), |_| {});
            if fail_at_end {
                assert!(matches!(result, Err(Error::Io(error))
                    if error.kind() == io::ErrorKind::PermissionDenied));
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
        counters::reset();
        assert!(
            matches!(hash_file(&dir.path().join("missing")), Err(Error::Io(error))
            if error.kind() == io::ErrorKind::NotFound)
        );
        assert_eq!(counters::snapshot(), counters::Counters::default());
    }

    #[cfg(unix)]
    #[test]
    fn directory_read_fails_without_counting_success() {
        let dir = tempfile::tempdir().unwrap();
        counters::reset();
        assert!(matches!(hash_file(dir.path()), Err(Error::Io(_))));
        assert_eq!(counters::snapshot(), counters::Counters::default());
    }
}

#[cfg(test)]
mod atomic_observation_tests {
    use super::*;
    use std::{cell::RefCell, fs, io};

    #[test]
    fn callbacks_bracket_real_replacement_and_legacy_counts_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target");
        fs::write(&path, b"old").unwrap();
        let events = RefCell::new(Vec::new());
        counters::reset();
        write_atomic_observed(
            &path,
            |w| {
                events.borrow_mut().push("write");
                w.write_all(b"new")
            },
            |file| {
                events.borrow_mut().push("staged");
                assert_eq!(file.metadata()?.len(), 3);
                // This is the flushed actual candidate, not the old target.
                let mut reader = file.try_clone()?;
                std::io::Seek::rewind(&mut reader)?;
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                assert_eq!(bytes, b"new");
                assert_eq!(fs::read(&path)?, b"old");
                #[cfg(feature = "test-counters")]
                assert_eq!(counters::snapshot().sync_all, 1);
                Ok(())
            },
            || {
                events.borrow_mut().push("persisted");
                assert_eq!(fs::read(&path).unwrap(), b"new");
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["write", "staged", "persisted"]);
        #[cfg(feature = "test-counters")]
        let observed_counts = counters::snapshot();
        counters::reset();
        write_atomic(&path, |w| w.write_all(b"legacy")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"legacy");
        #[cfg(feature = "test-counters")]
        {
            assert_eq!(counters::snapshot(), observed_counts);
            assert_eq!(
                observed_counts,
                counters::Counters {
                    sync_all: 1,
                    ..Default::default()
                }
            );
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn stage_write_and_persist_errors_never_latch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target");
        fs::write(&path, b"old").unwrap();
        let held = fs::File::open(&path).unwrap();
        #[cfg(unix)]
        let code = 13;
        #[cfg(windows)]
        let code = 5;
        for fail_write in [false, true] {
            let error = write_atomic_observed(
                &path,
                |w| {
                    w.write_all(b"new")?;
                    if fail_write {
                        Err(io::Error::from_raw_os_error(code))
                    } else {
                        Ok(())
                    }
                },
                |_| {
                    assert!(!fail_write);
                    Err(io::Error::from_raw_os_error(code))
                },
                || panic!("failed write/stage latched"),
            )
            .unwrap_err();
            assert!(matches!(error, Error::Io(e) if e.raw_os_error() == Some(code)));
            assert_eq!(fs::read(&path).unwrap(), b"old");
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(
                    held.metadata().unwrap().ino(),
                    path.metadata().unwrap().ino()
                );
            }
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
        let occupied = dir.path().join("directory");
        fs::create_dir(&occupied).unwrap();
        let staged = std::cell::Cell::new(0);
        let error = write_atomic_observed(
            &occupied,
            |w| w.write_all(b"new"),
            |_| {
                staged.set(staged.get() + 1);
                Ok(())
            },
            || panic!("failed persist latched"),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Io(_)));
        assert_eq!(staged.get(), 1);
        assert!(occupied.is_dir());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
