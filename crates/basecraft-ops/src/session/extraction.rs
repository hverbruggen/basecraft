//! Derived sequence ancestry uses the ordinary publication and input-evidence owners.
use super::{DocId, LogEntry, Path, Result, Session, files};

impl Session {
    fn publish_extraction(&self, id: DocId, path: &Path, activity: &LogEntry) -> Result<()> {
        self.history(id)?.extraction(
            path,
            activity,
            &self.document_record(id)?,
            self.saved_source(id)?,
            || {
                let mut bytes = Vec::new();
                files::write_main(self.sequence_of(id)?, &mut bytes)?;
                Ok(bytes)
            },
        )
    }
}

use super::{Op, OpResult, invalid};
use crate::files::HashingWriter;

impl Session {
    /// Extract residues `start..end` with their annotations to a new
    /// GenBank file at `path`, and log it.
    pub fn extract(
        &mut self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
        path: &Path,
    ) -> Result<()> {
        self.doc(id)?;
        let _reserved = self.derived_target(path)?;
        self.track(id)?;
        let doc = &self.sequence_of(id)?.doc;
        if start >= end || end > doc.len() {
            return Err(invalid(format!(
                "range {start}..{end} outside 0..{}",
                doc.len()
            )));
        }
        if segments.iter().any(|(a, b)| a >= b || *b > doc.len()) {
            return Err(invalid(
                "extraction segments are outside the source sequence",
            ));
        }
        // Check the accepted source before publishing any derived bytes.
        self.saved_source(id)?;
        let rec = files::extract_record(doc, start, end, segments, reverse);
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            basecraft_io::genbank::write(&mut hw, &rec)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Extract {
                start,
                end,
                path: key.clone(),
                segments: segments.to_vec(),
                reverse,
            },
            OpResult {
                hashes: vec![(key, hash)],
                feature: None,
            },
        );
        self.write(&entry)?;
        self.publish_extraction(id, path, &entry).map_err(|e| invalid(format!("extracted data saved; provenance incomplete: {e}. Keep the source session and retry to a new destination")))
    }

    /// Write a derived file, such as a figure, and log it.
    pub fn export(
        &mut self,
        id: DocId,
        path: &Path,
        format: &str,
        f: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
    ) -> Result<()> {
        let _reserved = self.derived_target(path)?;
        self.track(id)?;
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            f(&mut hw)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Export {
                path: key.clone(),
                format: format.to_string(),
            },
            OpResult {
                hashes: vec![(key, hash)],
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// The hash of the file an extract would write, without writing it.
    pub fn extract_hash(
        &self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
    ) -> Result<String> {
        let rec = files::extract_record(&self.sequence_of(id)?.doc, start, end, segments, reverse);
        let mut hw = HashingWriter::new(std::io::sink());
        basecraft_io::genbank::write(&mut hw, &rec)?;
        Ok(hw.hash())
    }
}
