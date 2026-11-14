//! Bounded in-memory ZIP access and deterministic `OpenDocument` ZIP writing.
//! Container parsing, compression and checksum validation belong to `zip`.
use std::io::{Cursor, Read, Write};

use crate::{Error, Result};

pub(crate) const MEMBER_LIMIT: u64 = 256 << 20;
const ENTRY_LIMIT: usize = 100_000;

fn unreadable(message: impl Into<String>) -> Error {
    Error::Table(message.into())
}

fn too_large(message: impl Into<String>) -> Error {
    Error::TooLarge(message.into())
}

fn zip_error(error: impl std::fmt::Display) -> Error {
    unreadable(format!("spreadsheet ZIP container: {error}"))
}

pub struct Archive<'a> {
    inner: zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Archive<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        let inner = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
        if inner.len() > ENTRY_LIMIT {
            return Err(too_large("the spreadsheet has too many parts"));
        }
        Ok(Self { inner })
    }

    /// Check every part before giving the same snapshot to the workbook reader.
    /// Bound actual decompression as well as declared sizes and verify CRCs.
    pub fn validate(&mut self) -> Result<()> {
        let mut total = 0u64;
        for index in 0..self.inner.len() {
            let mut member = self.inner.by_index(index).map_err(zip_error)?;
            let name = member.name().map_err(zip_error)?.into_owned();
            let size = member.size();
            total = total.saturating_add(size);
            if size > MEMBER_LIMIT
                || member.compressed_size() > MEMBER_LIMIT
                || total > MEMBER_LIMIT
            {
                return Err(too_large(
                    "the spreadsheet's ZIP contents are too large to import",
                ));
            }
            let actual =
                std::io::copy(&mut member.by_ref().take(size + 1), &mut std::io::sink())
                    .map_err(|e| unreadable(format!("spreadsheet part {name} is damaged: {e}")))?;
            if actual != size {
                return Err(unreadable(format!(
                    "spreadsheet part {name} has an incorrect size"
                )));
            }
        }
        Ok(())
    }

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let mut member = match self.inner.by_name(name) {
            Ok(member) => member,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(error) => return Err(zip_error(error)),
        };
        let size = member.size();
        if size > MEMBER_LIMIT || member.compressed_size() > MEMBER_LIMIT {
            return Err(too_large(format!("{name} is too large to read")));
        }
        let mut out = Vec::new();
        member
            .by_ref()
            .take(size + 1)
            .read_to_end(&mut out)
            .map_err(|e| unreadable(format!("spreadsheet part {name} is damaged: {e}")))?;
        if out.len() as u64 != size {
            return Err(unreadable(format!("{name} has an incorrect size")));
        }
        Ok(Some(out))
    }
}

pub struct Writer {
    inner: zip::ZipWriter<Cursor<Vec<u8>>>,
}

impl Default for Writer {
    fn default() -> Self {
        Self {
            inner: zip::ZipWriter::new(Cursor::new(Vec::new())),
        }
    }
}

impl Writer {
    pub fn add(&mut self, name: &str, data: &[u8], store: bool) -> Result<()> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(if store {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            })
            .last_modified_time(zip::DateTime::default());
        self.inner.start_file(name, options).map_err(zip_error)?;
        self.inner.write_all(data)?;
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        self.inner
            .finish()
            .map(std::io::Cursor::into_inner)
            .map_err(zip_error)
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
        let mut archive = Archive::new(&bytes).unwrap();
        assert_eq!(archive.read("mimetype").unwrap().unwrap(), b"text/plain");
        assert_eq!(
            archive.read("dir/content.xml").unwrap().unwrap(),
            big.as_bytes()
        );
        assert!(archive.read("absent").unwrap().is_none());
        archive.validate().unwrap();
        drop(archive);
        // Flip a stored byte: the checksum catches it.
        let at = bytes.windows(10).position(|w| w == b"text/plain").unwrap();
        bytes[at] = b'T';
        assert!(Archive::new(&bytes).unwrap().read("mimetype").is_err());
        assert!(Archive::new(b"not a zip").is_err());
    }

    #[test]
    fn validation_detects_damage_in_unread_members_and_truncation() {
        let mut writer = Writer::default();
        writer.add("unused.xml", b"payload", true).unwrap();
        let mut bytes = writer.finish().unwrap();
        let at = bytes.windows(7).position(|w| w == b"payload").unwrap();
        bytes[at] ^= 1;
        assert!(Archive::new(&bytes).unwrap().validate().is_err());
        bytes.truncate(bytes.len() - 12);
        assert!(Archive::new(&bytes).is_err());
    }

    #[test]
    fn declared_member_limit_is_checked_before_decompression() {
        let mut writer = Writer::default();
        writer.add("content.xml", b"small", false).unwrap();
        let mut bytes = writer.finish().unwrap();
        let directory = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        let size = u32::try_from(MEMBER_LIMIT + 1).unwrap().to_le_bytes();
        bytes[directory + 24..directory + 28].copy_from_slice(&size);
        assert!(
            Archive::new(&bytes)
                .and_then(|mut archive| archive.validate())
                .is_err()
        );
    }

    #[test]
    fn stored_first_member_is_reproducible_without_extra_fields() {
        let build = || {
            let mut writer = Writer::default();
            writer.add("mimetype", b"text/plain", true).unwrap();
            writer.add("content.xml", b"<content/>", false).unwrap();
            writer.finish().unwrap()
        };
        let bytes = build();
        assert_eq!(bytes, build());
        assert_eq!(&bytes[8..10], &[0, 0]); // Stored compression.
        assert_eq!(&bytes[28..30], &[0, 0]); // No local extra fields.
        assert!(bytes[30..].starts_with(b"mimetypetext/plain"));
    }
}
