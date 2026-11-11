//! New sequences share the ordinary session, typed history, save and recovery owners.
use super::{
    DocId, Loaded, Op, OpResult, Opened, Result, SequenceDoc, Session, Slot, files, invalid,
};
use std::sync::Arc;

impl Session {
    /// Validate plain DNA text before allocating a document or writing recovery.
    /// Sequence documents currently use the DNA GenBank writer; protein/RNA
    /// input must not silently acquire a DNA label.
    pub fn validate_new_sequence(name: &str, text: &str) -> Result<(String, String)> {
        if text.trim_start().starts_with('>') {
            return Err(invalid(
                "Enter plain DNA sequence text, without a FASTA header",
            ));
        }
        let rows = crate::alignment_add::parse_sequences(text, name).map_err(invalid)?;
        let (name, residues) = rows
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Enter a sequence"))?;
        if name
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-' | '.'))
        {
            return Err(invalid(
                "Use a sequence name containing letters, digits, underscores, hyphens or dots",
            ));
        }
        if let Some(base) = residues.chars().find(|c| !"ACGTRYSWKMBDHVN".contains(*c)) {
            return Err(invalid(format!(
                "Invalid DNA residue '{base}'. Use IUPAC DNA letters (A, C, G, T and ambiguity codes); RNA, protein and gaps are not supported for new sequence documents"
            )));
        }
        Ok((name, residues))
    }

    pub fn create_sequence(&mut self, name: &str, text: &str) -> Result<DocId> {
        let (name, residues) = Self::validate_new_sequence(name, text)?;
        let key = format!("new-sequence-{}.gb", self.next_id());
        self.create_sequence_with_key(&key, &name, &residues)
    }

    pub(crate) fn create_sequence_with_key(
        &mut self,
        key: &str,
        name: &str,
        text: &str,
    ) -> Result<DocId> {
        let (name, residues) = Self::validate_new_sequence(name, text)?;
        self.flush()?;
        let content = Loaded::Sequence(Opened {
            save_refusal: None,
            doc: SequenceDoc::new(
                name.clone(),
                Arc::new(residues.as_bytes().to_vec()),
                vec![],
                false,
            ),
            format: files::Format::GenBank,
            fasta: None,
        });
        self.docs.push(Slot {
            key: key.into(),
            content,
            generation: 0,
            save_authority: super::save_authority::PendingSaveAuthority::default(),
            undo: vec![],
            redo: vec![],
            baseline: None,
            baseline_logged: false,
            accepted_hashes: vec![],
            recovery_boundary: None,
            saved_at: None,
            source_stamps: Vec::new(),
            absent_sidecars: vec![],
            origin: None,
            origin_issue: None,
            origin_bundle: crate::provenance::bundle_for_result(&self.resolve(key)),
            disk_stamp: basecraft_protocol::provenance::FileStamp {
                size: 0,
                modified_seconds: 0,
                modified_nanos: 0,
                device: None,
                inode: None,
            },
        });
        let id = DocId(self.docs.len() - 1);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::CreateSequence { name, residues },
            OpResult {
                hashes: vec![],
                feature: None,
            },
        );
        self.docs[id.0].baseline = Some(entry);
        if let Err(error) = self.track(id) {
            self.docs.pop();
            return Err(error);
        }
        Ok(id)
    }
}
