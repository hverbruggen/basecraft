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
            saved_state_hashes: vec![],
            saved_target: None,
            current_saved_hashes: std::sync::OnceLock::new(),
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

impl Session {
    /// Adopt a prepared review into the shared journal. The embedded drafts make recovery self-contained.
    pub fn create_sanger_review(
        &mut self,
        review: &crate::sanger_review::SangerReview,
    ) -> Result<DocId> {
        let key = format!("new-review-{}.json", self.next_id());
        let mut review = review.clone();
        review.ensure_sources(review.storage_anchor().to_path_buf().as_path())?;
        let snapshot = review.session_snapshot(&self.resolve(&key))?;
        self.create_sanger_review_with_key(&key, &snapshot)
    }

    pub(crate) fn create_sanger_review_with_key(
        &mut self,
        key: &str,
        snapshot: &basecraft_protocol::sanger_records::SavedCallSets,
    ) -> Result<DocId> {
        self.flush()?;
        let review = crate::sanger_review::SangerReview::from_session_snapshot(
            snapshot,
            &self.resolve(key),
        )?;
        self.docs.push(Slot {
            key: key.into(),
            content: Loaded::SangerReview(Box::new(review)),
            generation: 0,
            save_authority: super::save_authority::PendingSaveAuthority::default(),
            undo: vec![],
            redo: vec![],
            baseline: None,
            baseline_logged: false,
            accepted_hashes: vec![],
            saved_state_hashes: vec![],
            saved_target: None,
            current_saved_hashes: std::sync::OnceLock::new(),
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
            Op::CreateSangerReview {
                review: Box::new(snapshot.clone()),
            },
            OpResult::default(),
        );
        self.docs[id.0].baseline = Some(entry);
        if let Err(error) = self.track(id) {
            self.docs.pop();
            return Err(error);
        }
        Ok(id)
    }

    /// Record an explicit analysis continuation without creating a parallel history.
    pub fn update_sanger_review(
        &mut self,
        id: DocId,
        review: &crate::sanger_review::SangerReview,
    ) -> Result<()> {
        self.sanger_review(id)?;
        let snapshot = review.session_snapshot(&self.resolve(self.doc_key(id)))?;
        self.apply(
            id,
            Op::SangerUpdate {
                review: Box::new(snapshot),
            },
        )?;
        Ok(())
    }
}

impl Session {
    // A prior Save As target remains owned by this session without changing the
    // journal's stable document key or discarding its undo stack.
    pub(super) fn check_previous_save_target(
        &self,
        id: DocId,
        target: &std::path::Path,
    ) -> Result<bool> {
        let Some(entry) = self.events.iter().rev().find(|entry| entry.doc == self.docs[id.0].key
            && matches!(&entry.op, Op::Save { path } if crate::save_guard::same_target(&self.resolve(path), target))) else { return Ok(false); };
        for (path, hash) in &entry.result.hashes {
            crate::save_guard::check_hash(&self.resolve(path), hash)?;
        }
        Ok(true)
    }
}
