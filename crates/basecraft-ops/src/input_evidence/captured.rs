//! Owned bytes shared by trace parsing, evidence registration and tool submission.
use super::{FileStamp, stamp};
use crate::{Error, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct CapturedInput {
    path: PathBuf,
    stamp: FileStamp,
    bytes: Arc<[u8]>,
    hash: String,
    sha256: String,
}
impl CapturedInput {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let before = stamp(&path)?;
        if before.size > 128 * 1024 * 1024 {
            return Err(Error::Limit {
                what: "captured trace bytes".into(),
                limit: 128 * 1024 * 1024,
            });
        }
        let (bytes, hash, sha256) =
            basecraft_io::source::read_fingerprinted(&path, 128 * 1024 * 1024)?;
        if stamp(&path)? != before {
            return Err(Error::ChangedOnDisk { path });
        }
        Ok(Self {
            path,
            stamp: before,
            bytes: bytes.into(),
            hash,
            sha256,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn stamp(&self) -> &FileStamp {
        &self.stamp
    }
    pub fn check_current(&self) -> Result<()> {
        if stamp(&self.path)? != self.stamp {
            return Err(Error::ChangedOnDisk {
                path: self.path.clone(),
            });
        }
        Ok(())
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.check_current()?;
        basecraft_io::write_atomic(path, |writer| writer.write_all(&self.bytes))?;
        Ok(())
    }
}
