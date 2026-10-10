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
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < MAP_THRESHOLD {
            return Ok(Source::Owned(std::fs::read(path)?));
        }
        // SAFETY: the mapping is read-only. If another program truncates the
        // file while it's mapped, reads past the new end raise SIGBUS.
        // basecraft-platform will install a handler that turns that into an
        // error; until then this is the documented risk in the design notes.
        let map = unsafe { Mmap::map(&file)? };
        #[cfg(unix)]
        {
            let _ = map.advise(memmap2::Advice::Random);
        }
        Ok(Source::Mapped(map))
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
