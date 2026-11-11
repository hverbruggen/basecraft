//! Document loading, baseline tracking and retained originals.
use super::{DocId, Op, OpResult, Path, PathBuf, Result, Slot, files, invalid, objects_dir};
use super::{OBJECT_STORE_LIMIT, Session, save_authority};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SavedMark(pub(super) Option<String>);

impl Slot {
    pub(super) fn mark(&self) -> super::SavedMark {
        super::SavedMark(self.undo.last().map(|entry| entry.entry_id.clone()))
    }

    pub(super) fn is_clean(&self) -> bool {
        self.saved_at.as_ref() == Some(&self.mark())
    }

    pub(super) fn content_mut(&mut self) -> &mut files::Loaded {
        &mut self.content
    }
}

impl Session {
    pub fn open(&mut self, path: &Path) -> Result<DocId> {
        self.open_classified(path).map(|(id, _)| id)
    }

    /// Open a live file and return the kind of the admitted document payload.
    /// Unlike a separate [`files::kind`] probe, this does not reread the main
    /// file to classify it. Existing companion validation still applies.
    pub fn open_classified(&mut self, path: &Path) -> Result<(DocId, files::Kind)> {
        self.open_inner(path, false)
    }

    pub(crate) fn open_replay_snapshot(&mut self, path: &Path) -> Result<DocId> {
        self.open_inner(path, true).map(|(id, _)| id)
    }

    pub(super) fn open_inner(
        &mut self,
        path: &Path,
        verified_snapshot: bool,
    ) -> Result<(DocId, files::Kind)> {
        if crate::sanger_review::evidence_kind(path).is_some() {
            return Err(invalid(
                "Saved Sanger reviews and drafts are edited only in the Basecraft application; read a saved Sanger review's history, methods and provenance with the history, methods and provenance commands",
            ));
        }
        self.flush()?;
        let disk_stamp = crate::input_evidence::stamp(path)?;
        let format = files::Format::from_path(path)?;
        let captured =
            files::capture_load(path, format, verified_snapshot, self.cache_root.as_deref())?;
        let content = captured.content;
        let key = self.relative(path);
        let retain_companions = content.as_document().retains_companions();
        let hashes: Vec<_> = captured
            .files
            .iter()
            .enumerate()
            .filter(|(i, _)| *i == 0 || retain_companions)
            .map(|(_, (path, _, hash))| (self.relative(path), hash.clone()))
            .collect();
        let absent_sidecars = captured
            .stamps
            .iter()
            .filter(|(_, stamp)| stamp.is_none())
            .map(|(path, _)| self.relative(path))
            .collect();
        let (origin, mut origin_issue) = crate::document_record::read_origin(path, &hashes);
        if let Some(warning) = content.as_document().opening_warning() {
            origin_issue = Some(
                origin_issue
                    .map_or_else(|| warning.to_owned(), |issue| format!("{issue}\n{warning}")),
            );
        }
        let kind = content.kind();
        let source_files = captured
            .files
            .iter()
            .map(|(path, _, hash)| {
                Ok(basecraft_protocol::provenance::InputReference {
                    path: self.relative(path),
                    stamp: crate::input_evidence::stamp(path)?,
                    hash: hash.clone(),
                    extra: std::collections::BTreeMap::new(),
                })
            })
            .collect::<Result<_>>()?;
        self.docs.push(Slot {
            key: key.clone(),
            content,
            generation: 0,
            save_authority: save_authority::PendingSaveAuthority::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            baseline: None,
            baseline_logged: false,
            accepted_hashes: hashes.clone(),
            saved_at: Some(super::SavedMark(None)),
            source_stamps: captured.stamps,
            absent_sidecars,
            origin,
            origin_issue,
            origin_bundle: crate::provenance::bundle_for_result(path),
            disk_stamp,
        });
        let id = DocId(self.docs.len() - 1);
        let mut entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );
        entry.source.as_mut().expect("open source").files = source_files;
        self.docs[id.0].baseline = Some(entry);
        Ok((id, kind))
    }

    /// Promote a read-only load before its first durable action. Validate and
    /// retain the accepted originals before any edit or replacement can occur.
    pub(super) fn track(&mut self, id: DocId) -> Result<()> {
        let Some(entry) = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?
            .baseline
            .clone()
        else {
            return Ok(());
        };
        if self.tracked {
            for (path, before) in &self.docs[id.0].source_stamps {
                let after = std::fs::metadata(path).ok();
                if !files::same_metadata(before.as_ref(), after.as_ref()) {
                    return Err(crate::Error::ChangedOnDisk { path: path.clone() });
                }
            }
        }
        if !self.docs[id.0].baseline_logged {
            self.pending_baselines.push(entry);
        }
        self.docs[id.0].baseline = None;
        Ok(())
    }

    /// Cheap change token for readers; saves and job outcomes also invalidate a report.
    pub fn record_revision(&self, id: DocId) -> Result<String> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        Ok(format!(
            "{}:{}:{}:{}",
            self.id,
            id.0,
            slot.generation,
            self.events.len()
        ))
    }

    /// Read-only evidence for this session's exact current revision, including pending edits.
    pub fn document_record(&self, id: DocId) -> Result<crate::document_record::DocumentRecord> {
        use basecraft_protocol::provenance::EditActivity;
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let mut record = slot.origin.as_ref().map_or_else(
            || crate::document_record::DocumentRecord::empty(&slot.key),
            crate::document_record::DocumentRecord::from_publication,
        );
        if let Some(origin) = &slot.origin {
            record
                .origins
                .push((slot.origin_bundle.clone(), origin.clone()));
        }
        record.document.clone_from(&slot.key);
        record.revision = format!("{}:{}:{}", self.id, id.0, slot.generation);
        record.unsaved = !slot.is_clean();
        if let Some(issue) = &slot.origin_issue {
            record.warnings.push(issue.clone());
        }
        if !slot.baseline_logged
            && let Some(baseline) = &slot.baseline
        {
            record.events.push(baseline.clone());
        }
        record
            .events
            .extend(self.events.iter().filter(|e| e.doc == slot.key).cloned());
        record.events.extend(
            self.pending_baselines
                .iter()
                .filter(|e| e.doc == slot.key)
                .cloned(),
        );
        if let Some(pending) = &self.pending
            && pending.doc == id
        {
            record.events.push(pending.entry.clone());
        }
        record
            .effective
            .extend(slot.undo.iter().map(|a| EditActivity {
                id: a.entry_id.clone(),
                operation: a.op.clone(),
            }));
        record.deduplicate();
        crate::contributions::project(&record.events, &mut record.effective);
        Ok(record)
    }

    pub(super) fn reference_baseline(&mut self, id: DocId) -> Result<()> {
        if !self.docs[id.0].baseline_logged {
            if let Some(entry) = self.docs[id.0].baseline.clone() {
                self.write(&entry)?;
            }
            self.docs[id.0].baseline_logged = true;
        }
        Ok(())
    }

    /// The saved file and its accepted hash when the document is clean and the
    /// file is unchanged since opening; `None` when there are unsaved edits.
    /// Checks metadata and reuses the verified open hash, never rereads disk bytes.
    pub fn saved_source(
        &self,
        id: DocId,
    ) -> Result<Option<(PathBuf, basecraft_protocol::provenance::FileStamp, String)>> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        if !slot.is_clean() {
            return Ok(None);
        }
        let path = self.resolve(&slot.key).canonicalize()?;
        let current = crate::input_evidence::stamp(&path)?;
        if current != slot.disk_stamp {
            return Err(invalid(
                "source changed since opening; reopen it before analysis",
            ));
        }
        let hash = slot
            .accepted_hashes
            .first()
            .ok_or_else(|| invalid("source identity unavailable"))?
            .1
            .clone();
        Ok(Some((path, current, hash)))
    }

    /// Use an accepted disk revision only for a clean, unchanged document.
    pub fn referenced_source(
        &self,
        id: DocId,
        bundle: &Path,
    ) -> Result<Option<basecraft_protocol::provenance::InputReference>> {
        self.saved_source(id)?
            .map(|(path, stamp, hash)| {
                Ok(basecraft_protocol::provenance::InputReference {
                    path: crate::input_evidence::relative(bundle, &path)?,
                    stamp,
                    hash,
                    extra: std::collections::BTreeMap::new(),
                })
            })
            .transpose()
    }

    pub fn snapshot_file(&self, path: &Path) -> Result<String> {
        #[cfg(feature = "test-counters")]
        {
            let bytes = std::fs::read(path)?;
            basecraft_io::counters::record_read(path, bytes.len() as u64);
            self.snapshot_bytes(&bytes)
        }
        #[cfg(not(feature = "test-counters"))]
        self.snapshot_bytes(&std::fs::read(path)?)
    }

    /// Hash and retain the exact immutable buffer consumed by an analysis.
    pub fn snapshot_bytes(&self, bytes: &[u8]) -> Result<String> {
        let hash = basecraft_io::hash_bytes(bytes);
        if self.tracked && bytes.len() as u64 > OBJECT_STORE_LIMIT {
            return Err(crate::Error::Limit {
                what: "exact input retention".into(),
                limit: OBJECT_STORE_LIMIT,
            });
        }
        if self.store_objects && bytes.len() as u64 <= OBJECT_STORE_LIMIT {
            let dest = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
            if !dest.exists() {
                std::fs::create_dir_all(objects_dir(&self.storage_root))?;
                basecraft_io::write_atomic(&dest, |w| w.write_all(bytes))?;
            }
        }
        Ok(hash)
    }
}
