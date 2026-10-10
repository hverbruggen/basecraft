//! The index cache in `~/.cache/basecraft/index/`.
//!
//! Entries are keyed by canonical path, size, modification time and inode,
//! which costs one `stat` to check. Hashing the content instead would read
//! the whole file and defeat the point of an index. Content hashes for the
//! operation log are computed separately, when they're needed.
//!
//! The cache is never authoritative and can be deleted at any time.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fasta::FastaIndex;
use crate::{Result, Source};

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BASECRAFT_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("basecraft")
}

fn key(path: &Path) -> Result<String> {
    let canonical = fs::canonicalize(path)?;
    let meta = fs::metadata(&canonical)?;
    let mtime = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0u64;
    let mut h = blake3::Hasher::new();
    h.update(canonical.as_os_str().as_encoded_bytes());
    h.update(&meta.len().to_le_bytes());
    h.update(&mtime.to_le_bytes());
    h.update(&inode.to_le_bytes());
    Ok(h.finalize().to_hex()[..32].to_string())
}

/// An open, indexed FASTA file.
pub struct IndexedFasta {
    pub source: Source,
    pub index: FastaIndex,
    /// Whether the index came from the cache.
    pub cached: bool,
}

impl IndexedFasta {
    pub fn bytes(&self) -> &[u8] {
        self.source.bytes()
    }
}

/// Open a FASTA file, loading its index from the cache or building and
/// caching it.
pub fn open_fasta(path: &Path) -> Result<IndexedFasta> {
    open_fasta_in(path, &cache_dir())
}

pub fn open_fasta_in(path: &Path, cache: &Path) -> Result<IndexedFasta> {
    let source = Source::open(path)?;
    let entry = cache.join("index").join(format!("{}.fai2", key(path)?));
    if let Ok(bytes) = fs::read(&entry)
        && let Some(index) = FastaIndex::from_bytes(&bytes)
    {
        return Ok(IndexedFasta {
            source,
            index,
            cached: true,
        });
    }
    let index = FastaIndex::build(source.bytes())?;
    // A failed cache write only costs a rebuild next time.
    if fs::create_dir_all(entry.parent().unwrap_or(cache)).is_ok() {
        let _ = crate::write_atomic(&entry, |w| w.write_all(&index.to_bytes()));
    }
    Ok(IndexedFasta {
        source,
        index,
        cached: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_open_uses_cache_and_edit_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let f = dir.path().join("a.fasta");
        fs::write(&f, ">a\nACGT\nAC\n").unwrap();
        assert!(!open_fasta_in(&f, &cache).unwrap().cached);
        assert!(open_fasta_in(&f, &cache).unwrap().cached);
        // Atomic replace gives a new inode, so the key changes even if the
        // size and mtime happen to match.
        crate::write_atomic(&f, |w| w.write_all(b">a\nACGT\nGG\n")).unwrap();
        let reopened = open_fasta_in(&f, &cache).unwrap();
        assert!(!reopened.cached);
        let r = &reopened.index.records[0];
        assert_eq!(r.read(reopened.bytes(), 4, 6).unwrap(), b"GG");
    }
}
