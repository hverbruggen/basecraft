//! Minimal ZIP container support for spreadsheet files (`.ods`, `.xlsx`).
//!
//! Reading looks up named members in memory only; nothing is extracted to
//! disk. Only stored and deflated members are supported, and sizes are
//! bounded so a crafted archive cannot exhaust memory. Writing produces
//! stored or deflated members with CRC-32, enough for `OpenDocument`.
use std::io::{Read, Write};

use crate::{Error, Result};

/// Largest member read, uncompressed.
const MEMBER_LIMIT: u64 = 256 << 20;
const ENTRY_LIMIT: usize = 100_000;

fn invalid(message: impl Into<String>) -> Error {
    Error::Table(message.into())
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| invalid("the spreadsheet's ZIP container is truncated"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| invalid("the spreadsheet's ZIP container is truncated"))
}

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
}

/// A ZIP archive held in memory.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

impl<'a> Archive<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        // The end-of-central-directory record is in the last 64 KiB + 22 bytes.
        let start = bytes.len().saturating_sub(65_557);
        let end = (start..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| bytes[i..].starts_with(b"PK\x05\x06"))
            .ok_or_else(|| invalid("not a ZIP-based spreadsheet"))?;
        let count = usize::from(u16_at(bytes, end + 10)?);
        let mut at = u32_at(bytes, end + 16)? as usize;
        if count > ENTRY_LIMIT {
            return Err(invalid("the spreadsheet has too many parts"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(bytes, at)? != 0x0201_4b50 {
                return Err(invalid("the spreadsheet's ZIP directory is damaged"));
            }
            let name_len = usize::from(u16_at(bytes, at + 28)?);
            let extra_len = usize::from(u16_at(bytes, at + 30)?);
            let comment_len = usize::from(u16_at(bytes, at + 32)?);
            let name = bytes
                .get(at + 46..at + 46 + name_len)
                .ok_or_else(|| invalid("the spreadsheet's ZIP directory is truncated"))?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                method: u16_at(bytes, at + 10)?,
                crc: u32_at(bytes, at + 16)?,
                compressed: u64::from(u32_at(bytes, at + 20)?),
                size: u64::from(u32_at(bytes, at + 24)?),
                offset: u64::from(u32_at(bytes, at + 42)?),
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { bytes, entries })
    }

    /// The uncompressed bytes of member `name`, or `None` if absent.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.entries.iter().find(|e| e.name == name) else {
            return Ok(None);
        };
        if entry.size > MEMBER_LIMIT || entry.compressed > MEMBER_LIMIT {
            return Err(invalid(format!("{name} is too large to read")));
        }
        let header = usize::try_from(entry.offset).map_err(|_| invalid("bad member offset"))?;
        if u32_at(self.bytes, header)? != 0x0403_4b50 {
            return Err(invalid(format!("{name} has a damaged header")));
        }
        let data = header
            + 30
            + usize::from(u16_at(self.bytes, header + 26)?)
            + usize::from(u16_at(self.bytes, header + 28)?);
        #[allow(clippy::cast_possible_truncation)] // Bounded by MEMBER_LIMIT.
        let raw = self
            .bytes
            .get(data..data + entry.compressed as usize)
            .ok_or_else(|| invalid(format!("{name} is truncated")))?;
        let out = match entry.method {
            0 => raw.to_vec(),
            8 => {
                let mut out = Vec::new();
                flate2::read::DeflateDecoder::new(raw)
                    .take(MEMBER_LIMIT + 1)
                    .read_to_end(&mut out)?;
                out
            }
            other => {
                return Err(invalid(format!(
                    "{name} uses ZIP compression method {other}, which Basecraft does not read"
                )));
            }
        };
        if out.len() as u64 != entry.size || crc32fast::hash(&out) != entry.crc {
            return Err(invalid(format!(
                "{name} is damaged (size or checksum differs)"
            )));
        }
        Ok(Some(out))
    }
}

/// Builds a ZIP archive in memory.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
    directory: Vec<u8>,
    count: u16,
}

impl Writer {
    /// Add a member, deflated unless `store` is set.
    pub fn add(&mut self, name: &str, data: &[u8], store: bool) -> Result<()> {
        let body = if store {
            data.to_vec()
        } else {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data)?;
            encoder.finish()?
        };
        let too_big = || invalid("spreadsheet part too large");
        let offset = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let compressed = u32::try_from(body.len()).map_err(|_| too_big())?;
        let name_len = u16::try_from(name.len()).map_err(|_| too_big())?;
        let method: u16 = if store { 0 } else { 8 };
        let crc = crc32fast::hash(data);
        // Version 2.0, no flags, fixed 1980-01-01 timestamp for reproducible files.
        let common = |w: &mut Vec<u8>| {
            w.extend_from_slice(&20u16.to_le_bytes());
            w.extend_from_slice(&0u16.to_le_bytes());
            w.extend_from_slice(&method.to_le_bytes());
            w.extend_from_slice(&0u16.to_le_bytes());
            w.extend_from_slice(&0x21u16.to_le_bytes());
            w.extend_from_slice(&crc.to_le_bytes());
            w.extend_from_slice(&compressed.to_le_bytes());
            w.extend_from_slice(&size.to_le_bytes());
            w.extend_from_slice(&name_len.to_le_bytes());
            w.extend_from_slice(&0u16.to_le_bytes());
        };
        self.out.extend_from_slice(b"PK\x03\x04");
        common(&mut self.out);
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(&body);
        self.directory.extend_from_slice(b"PK\x01\x02");
        self.directory.extend_from_slice(&20u16.to_le_bytes());
        common(&mut self.directory);
        self.directory.extend_from_slice(&[0; 8]);
        self.directory.extend_from_slice(&0u32.to_le_bytes());
        self.directory.extend_from_slice(&offset.to_le_bytes());
        self.directory.extend_from_slice(name.as_bytes());
        self.count = self.count.checked_add(1).ok_or_else(too_big)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
        let too_big = || invalid("spreadsheet too large");
        let start = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let length = u32::try_from(self.directory.len()).map_err(|_| too_big())?;
        self.out.extend_from_slice(&self.directory);
        self.out.extend_from_slice(b"PK\x05\x06");
        self.out.extend_from_slice(&[0; 4]);
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&length.to_le_bytes());
        self.out.extend_from_slice(&start.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_archives_read_back_and_damage_is_detected() {
        let mut writer = Writer::default();
        writer.add("mimetype", b"text/plain", true).unwrap();
        let big = "row\n".repeat(1000);
        writer
            .add("dir/content.xml", big.as_bytes(), false)
            .unwrap();
        let mut bytes = writer.finish().unwrap();
        let archive = Archive::new(&bytes).unwrap();
        assert_eq!(archive.read("mimetype").unwrap().unwrap(), b"text/plain");
        assert_eq!(
            archive.read("dir/content.xml").unwrap().unwrap(),
            big.as_bytes()
        );
        assert!(archive.read("absent").unwrap().is_none());
        // Flip a stored byte: the checksum catches it.
        let at = bytes.windows(10).position(|w| w == b"text/plain").unwrap();
        bytes[at] = b'T';
        assert!(Archive::new(&bytes).unwrap().read("mimetype").is_err());
        assert!(Archive::new(b"not a zip").is_err());
    }
}
