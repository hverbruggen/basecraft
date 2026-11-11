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
        let content = Loaded::Sequence(Opened {
            doc: SequenceDoc::new(
                name.clone(),
                Arc::new(residues.as_bytes().to_vec()),
                vec![],
                false,
            ),
            format: files::Format::GenBank,
            fasta: None,
            others: files::OtherRecords::default(),
        });
        self.docs.push(Slot::new(
            key.into(),
            content,
            crate::records::provenance::bundle_for_result(
                &self.resolve(key),
                self.project.as_ref(),
            ),
            basecraft_protocol::provenance::FileStamp {
                size: 0,
                modified_seconds: 0,
                modified_nanos: 0,
                device: None,
                inode: None,
            },
        ));
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
        // Unlike an open, creation holds residues that exist nowhere else.
        if let Err(error) = self.track(id).and_then(|()| self.write_entries(&[])) {
            self.pending_baselines.retain(|entry| entry.doc != key);
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
        let anchor = review.storage_anchor().to_path_buf();
        if review.ensure_sources(&anchor).is_err() {
            // A read-only source folder must not prevent review. Durable private
            // evidence outlives the disposable session recovery directory.
            std::fs::create_dir_all(&self.local_provenance_root)?;
            review.ensure_sources(&self.local_provenance_root.join(&key))?;
        }
        let snapshot = review.session_snapshot(&self.resolve(&key))?;
        let record = self.retain_review_record(&snapshot)?;
        let id = self.create_sanger_review_with_key(&key, &record)?;
        if let Some(current) = self.docs[id.0].content_mut().sanger_review_mut() {
            current.share_captures(&review);
        }
        Ok(id)
    }

    pub(crate) fn create_sanger_review_with_key(
        &mut self,
        key: &str,
        record: &basecraft_protocol::sanger_records::ReviewRecord,
    ) -> Result<DocId> {
        let snapshot = crate::sanger_review::read_journal_record(record)?;
        let record = self.retain_review_record(&snapshot)?;
        let review = crate::sanger_review::SangerReview::from_session_snapshot(
            &snapshot,
            &self.resolve(key),
        )?;
        self.docs.push(Slot::new(
            key.into(),
            Loaded::SangerReview(Box::new(review)),
            crate::records::provenance::bundle_for_result(
                &self.resolve(key),
                self.project.as_ref(),
            ),
            basecraft_protocol::provenance::FileStamp {
                size: 0,
                modified_seconds: 0,
                modified_nanos: 0,
                device: None,
                inode: None,
            },
        ));
        let id = DocId(self.docs.len() - 1);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::CreateSangerReview {
                review: record.clone(),
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

    pub(super) fn retain_review_record(
        &self,
        review: &basecraft_protocol::sanger_records::SavedCallSets,
    ) -> Result<basecraft_protocol::sanger_records::ReviewRecord> {
        let bytes = serde_json::to_vec(review)?;
        let hash = basecraft_io::hash_bytes(&bytes);
        let directory = crate::session::objects_dir(&self.storage_root);
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(hash.trim_start_matches("b3:"));
        if !path.exists() {
            basecraft_io::write_atomic(&path, |w| w.write_all(&bytes))?;
        }
        Ok(basecraft_protocol::sanger_records::ReviewRecord { path, hash })
    }

    pub fn export_sanger_consensus(
        &mut self,
        id: DocId,
        path: &std::path::Path,
        label: &str,
    ) -> Result<()> {
        let review = self.sanger_review(id)?;
        let call_set = review.draft_sha256.clone();
        let sha256 = review
            .write_consensus(path, label)
            .map_err(super::Error::Invalid)?;
        self.apply(
            id,
            Op::SangerReview {
                call_set,
                edit: Box::new(basecraft_protocol::sanger::ReviewOp::Export {
                    path: path.into(),
                    sha256,
                }),
            },
        )?;
        self.docs[id.0]
            .content_mut()
            .sanger_review_mut()
            .ok_or(super::Error::WrongKind {
                id,
                expected: files::Kind::SangerReview,
            })?
            .publish_consensus(path)
    }

    /// Record an explicit analysis continuation without creating a parallel history.
    pub fn update_sanger_review(
        &mut self,
        id: DocId,
        review: &crate::sanger_review::SangerReview,
    ) -> Result<()> {
        self.sanger_review(id)?;
        let snapshot = review.session_snapshot(&self.resolve(self.doc_key(id)?))?;
        let record = self.retain_review_record(&snapshot)?;
        self.apply(id, Op::SangerUpdate { review: record })?;
        if let Some(current) = self.docs[id.0].content_mut().sanger_review_mut() {
            current.share_captures(review);
        }
        Ok(())
    }
}

impl Slot {
    pub(super) fn new(
        key: String,
        content: Loaded,
        origin_bundle: std::path::PathBuf,
        disk_stamp: basecraft_protocol::provenance::FileStamp,
    ) -> Self {
        Self {
            key,
            content,
            origin_bundle,
            disk_stamp,
            generation: 0,
            pending_check: super::save_consent::PendingSaveCheck::default(),
            undo: vec![],
            redo: vec![],
            baseline: None,
            baseline_logged: false,
            accepted_hashes: vec![],
            recovery_boundary: None,
            saved_at: None,
            source_stamps: Vec::new(),
            recovery_changed: false,
            saved_target: None,
            absent_sidecars: vec![],
            origin: None,
            origin_issue: None,
        }
    }
}
