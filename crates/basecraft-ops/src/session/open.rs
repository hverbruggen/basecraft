//! Document loading, baseline tracking and retained originals.
use super::{DocId, Op, OpResult, Path, PathBuf, Result, Slot, files, invalid, objects_dir};
use super::{Session, save_authority};

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
        self.open_inner(path)
    }

    pub(super) fn open_inner(&mut self, path: &Path) -> Result<(DocId, files::Kind)> {
        if crate::sanger_review::evidence_kind(path).is_some() {
            return Err(invalid(
                "Saved Sanger reviews and drafts are edited only in the Basecraft application; read a saved Sanger review's history, methods and provenance with the history, methods and provenance commands",
            ));
        }
        self.flush()?;
        let disk_stamp = crate::input_evidence::stamp(path)?;
        let format = files::Format::from_path(path)?;
        let captured = files::capture_load(path, format)?;
        let content = captured.content;
        let key = self.relative(path);
        let retain_companions = content.as_document().retains_companions();
        let hashes: Vec<_> = captured
            .files
            .iter()
            .enumerate()
            .filter(|(i, _)| *i == 0 || retain_companions)
            .map(|(_, (path, hash))| (self.relative(path), hash.clone()))
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
            .map(|(path, hash)| {
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
            recovery_boundary: None,
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

    /// First edit retains an inode when possible. Later edits check only stamps;
    /// a changed original requires a snapshot of the live document before editing.
    pub(super) fn track(&mut self, id: DocId) -> Result<()> {
        if !self.tracked {
            return Ok(());
        }
        let slot = self
            .docs
            .get(id.0)
            .ok_or(crate::Error::UnknownDocument { id })?;
        if self.source_changed(id) {
            if slot
                .baseline
                .as_ref()
                .is_some_and(|entry| matches!(entry.op, Op::Open { .. }))
                && !slot.baseline_logged
            {
                return Err(crate::Error::ChangedOnDisk {
                    path: self.resolve(&slot.key),
                });
            }
            if self.log.is_some() {
                self.flush()?;
                let entry = self.recovery_checkpoint(id)?;
                self.docs[id.0].recovery_boundary = Some(entry.id.clone());
                self.pending_baselines.push(entry);
                self.refresh_source_stamps(id);
                self.docs[id.0].origin_issue = Some("The original changed on disk. Your current edits have been preserved in recovery; save them to a new file.".into());
                self.docs[id.0].baseline = None;
            }
            return Ok(());
        }
        if let Some(entry) = self.docs[id.0].baseline.clone() {
            if self.store_objects {
                let directory = objects_dir(&self.storage_root);
                std::fs::create_dir_all(&directory)?;
                let stage = tempfile::tempdir_in(&directory)?;
                for (key, hash) in &entry.result.hashes {
                    let source = self.resolve(key);
                    let target = directory.join(hash.trim_start_matches("b3:"));
                    let link = stage.path().join("input");
                    match std::fs::hard_link(&source, &link) {
                        // Replace any old link whose inode was modified externally.
                        Ok(()) => std::fs::rename(&link, &target)?,
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::CrossesDevices
                                    | std::io::ErrorKind::Unsupported
                                    | std::io::ErrorKind::PermissionDenied
                            ) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                for reference in &entry.source.as_ref().expect("baseline source").files {
                    let source = self.resolve(&reference.path);
                    if crate::input_evidence::stamp(&source)? != reference.stamp {
                        return Err(crate::Error::ChangedOnDisk { path: source });
                    }
                }
                // Hard linking changes ctime. Retain the new stamps without reading bytes.
                self.refresh_source_stamps(id);
            }
            if !self.docs[id.0].baseline_logged {
                self.pending_baselines.push(entry);
            }
            self.docs[id.0].baseline = None;
        }
        Ok(())
    }

    pub(super) fn source_changed(&self, id: DocId) -> bool {
        self.docs[id.0].source_stamps.iter().any(|(path, before)| {
            !files::same_metadata(before.as_ref(), std::fs::metadata(path).ok().as_ref())
        })
    }

    pub(super) fn refresh_source_stamps(&mut self, id: DocId) {
        for (path, stamp) in &mut self.docs[id.0].source_stamps {
            *stamp = std::fs::metadata(path).ok();
        }
    }

    /// An exceptional checkpoint: external modification or undo across a save.
    /// Write each saved form once, then move it into the content-addressed store.
    pub(super) fn recovery_checkpoint(&self, id: DocId) -> Result<basecraft_protocol::LogEntry> {
        if let Some(message) = self.docs[id.0].content.as_document().save_refusal() {
            return Err(crate::Error::Unsupported(message.into()));
        }
        let directory = objects_dir(&self.storage_root);
        std::fs::create_dir_all(&directory)?;
        let stage = tempfile::tempdir_in(&directory)?;
        let key = &self.docs[id.0].key;
        let local = crate::replay::scratch_key(key).map_err(|detail| crate::Error::Damaged {
            path: self.storage_root.clone(),
            detail,
        })?;
        let target = stage.path().join(&local);
        let original = self.resolve(key);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let publication = crate::documents::WritePublication::Recovery {
            stage: target.parent().expect("checkpoint parent").to_path_buf(),
            destination: original.parent().expect("source parent").to_path_buf(),
            stores: std::sync::Mutex::default(),
        };
        let written = self.docs[id.0].content.as_document().write(
            &target,
            files::Format::from_path(&target)?,
            &self.storage_root,
            &publication,
        )?;
        let mut hashes = Vec::new();
        for (path, hash) in written {
            let name = path.strip_prefix(stage.path()).map_err(|_| {
                crate::Error::Unsupported("Recovery writer escaped its staging folder".into())
            })?;
            let name = if Path::new(key).is_absolute() {
                original.parent().expect("source parent").join(
                    path.strip_prefix(target.parent().expect("checkpoint parent"))
                        .map_err(|_| {
                            crate::Error::Unsupported(
                                "Recovery writer escaped its document folder".into(),
                            )
                        })?,
                )
            } else {
                name.to_path_buf()
            };
            hashes.push((name.to_string_lossy().into_owned(), hash.clone()));
            std::fs::rename(&path, directory.join(hash.trim_start_matches("b3:")))?;
        }
        let mut entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key.clone() },
            OpResult {
                hashes,
                feature: None,
            },
        );
        entry.source.as_mut().expect("checkpoint source").checkpoint = true;
        Ok(entry)
    }

    pub(crate) fn restore_recovery_ids(
        &mut self,
        id: DocId,
        source: &basecraft_protocol::ops::SourceRecord,
    ) -> Result<()> {
        let result = match self.docs[id.0].content_mut() {
            files::Loaded::Sequence(opened) => {
                opened.doc.restore_feature_slots(&source.feature_slots)
            }
            files::Loaded::Tree(opened) => opened.doc.restore_node_ids(&source.node_ids),
            _ => Ok(()),
        };
        result.map_err(|error| crate::Error::Damaged {
            path: self.root.clone(),
            detail: error.to_string(),
        })
    }

    pub fn recovery_warning(&self, id: DocId) -> Option<&str> {
        self.docs
            .get(id.0)?
            .origin_issue
            .as_deref()
            .filter(|text| text.starts_with("The original changed on disk."))
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
        if self.store_objects {
            let dest = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
            if !dest.exists() {
                std::fs::create_dir_all(objects_dir(&self.storage_root))?;
                basecraft_io::write_atomic(&dest, |w| w.write_all(bytes))?;
            }
        }
        Ok(hash)
    }
}
