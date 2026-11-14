//! Document loading and baseline tracking.
use super::Session;
use super::{DocId, Op, OpResult, Path, PathBuf, Result, Slot, files, invalid};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SavedMark(pub(super) Option<String>);

impl Slot {
    pub(super) fn mark(&self) -> super::SavedMark {
        let last = self.undo.last().map(|entry| &entry.entry_id);
        super::SavedMark(last.or(self.settled.last().map(|edit| &edit.id)).cloned())
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
        let format = files::Format::from_path(path)?;
        let captured = files::capture_load(self.companion_project(), path, format)?;
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
        let (origin, mut origin_issue) =
            crate::records::document_record::read_origin(path, &hashes, self.project.as_ref());
        for warning in content
            .as_document()
            .opening_warning()
            .into_iter()
            .chain(captured.warning.as_deref())
        {
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
                    stamp: captured
                        .stamps
                        .iter()
                        .find(|(file, _)| file == path)
                        .and_then(|(_, stamp)| stamp.clone())
                        .expect("captured file stamp"),
                    hash: hash.clone(),
                    extra: std::collections::BTreeMap::new(),
                })
            })
            .collect::<Result<_>>()?;
        let disk_stamp = captured.stamps[0].1.clone().expect("main file stamp");
        let mut slot = Slot::new(
            key.clone(),
            content,
            crate::records::provenance::bundle_for_result(path, self.project.as_ref()),
            disk_stamp,
        );
        slot.accepted_hashes.clone_from(&hashes);
        slot.absent_sidecars = absent_sidecars;
        if let Some((origin, history)) = origin {
            slot.origin = Some(origin);
            slot.origin_history = history;
        }
        slot.origin_issue = origin_issue;
        slot.saved_at = Some(super::SavedMark(None));
        slot.source_stamps = captured.stamps;
        self.docs.push(slot);
        let id = DocId(self.docs.len() - 1);
        let mut entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key },
            OpResult {
                notes: Vec::new(),
                hashes,
                feature: None,
            },
        );
        entry.source.as_mut().expect("open source").files = source_files;
        self.docs[id.0].baseline = Some(entry);
        Ok((id, kind))
    }

    /// Every edit checks the original's stamps; a changed original requires a
    /// snapshot of the live document before editing.
    pub(super) fn track(&mut self, id: DocId) -> Result<()> {
        if !self.tracked {
            return Ok(());
        }
        self.start_recovery()?;
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
                if let Some(entry) = self.recovery_checkpoint(id)? {
                    self.docs[id.0].recovery_boundary = Some(entry.id.clone());
                    self.pending_baselines.push(entry);
                }
                self.refresh_source_stamps(id);
                self.note_changed_on_disk(id);
                self.docs[id.0].baseline = None;
            }
            return Ok(());
        }
        if let Some(entry) = self.docs[id.0].baseline.take() {
            // A logged baseline is already durable.
            if !self.docs[id.0].baseline_logged {
                self.pending_baselines.push(entry);
            }
        }
        Ok(())
    }

    pub(super) fn source_changed(&self, id: DocId) -> bool {
        self.docs[id.0]
            .source_stamps
            .iter()
            .any(|(path, before)| *before != crate::records::input_evidence::stamp(path).ok())
    }

    pub(super) fn refresh_source_stamps(&mut self, id: DocId) {
        for (path, stamp) in &mut self.docs[id.0].source_stamps {
            *stamp = crate::records::input_evidence::stamp(path).ok();
        }
    }

    /// An exceptional checkpoint: external modification or undo across a save.
    /// Write each saved form once, then move it into the content-addressed store.
    pub(super) fn recovery_checkpoint(
        &self,
        id: DocId,
    ) -> Result<Option<basecraft_protocol::LogEntry>> {
        let directory = objects_dir(&self.storage_root);
        recovery_directory(&directory)?;
        let stage = tempfile::tempdir_in(&directory)?;
        let key = &self.docs[id.0].key;
        let local =
            crate::records::replay::scratch_key(key).map_err(|detail| crate::Error::Damaged {
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
        // A review's format is recognised from content, which the target lacks yet.
        let format = if self.docs[id.0].content.kind() == files::Kind::SangerReview {
            files::Format::SangerReview
        } else {
            files::Format::from_path(&target)?
        };
        // The checkpoint stage is a loose folder: its companion sits in the
        // stage's own .basecraft.
        let written = self.docs[id.0].content.as_document().write(
            None,
            &target,
            format,
            &self.storage_root,
            &publication,
        )?;
        let mut hashes = Vec::new();
        for (path, hash) in written {
            let name = basecraft_platform::files::inside(&path, stage.path()).ok_or_else(|| {
                crate::Error::Unsupported("Recovery writer escaped its staging folder".into())
            })?;
            let name = if Path::new(key).is_absolute() {
                original.parent().expect("source parent").join(
                    basecraft_platform::files::inside(
                        &path,
                        target.parent().expect("checkpoint parent"),
                    )
                    .ok_or_else(|| {
                        crate::Error::Unsupported(
                            "Recovery writer escaped its document folder".into(),
                        )
                    })?,
                )
            } else {
                name
            };
            hashes.push((name.to_string_lossy().into_owned(), hash.clone()));
            std::fs::rename(&path, directory.join(hash.trim_start_matches("b3:")))?;
        }
        let mut entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key.clone() },
            OpResult {
                notes: Vec::new(),
                hashes,
                feature: None,
            },
        );
        entry.source.as_mut().expect("checkpoint source").checkpoint = true;
        basecraft_platform::files::sync_directory(&directory)?;
        Ok(Some(entry))
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

    pub(super) fn note_changed_on_disk(&mut self, id: DocId) {
        self.docs[id.0].recovery_notice = Some(
            "The original changed on disk. Your current edits have been preserved in recovery; save them to a new file.".into(),
        );
    }

    pub fn recovery_warning(&self, id: DocId) -> Option<&str> {
        let slot = self.docs.get(id.0)?;
        slot.recovery_notice.as_deref()
    }

    /// Note recovery that an earlier session, for example one that crashed,
    /// left for this document in the recovery folders of `roots`. The notice
    /// says where it is; recovering it stays the user's choice.
    pub fn find_earlier_recovery(&mut self, id: DocId, roots: &[PathBuf]) -> Result<()> {
        let path = self.resolve(&self.slot(id)?.key);
        let path = path.canonicalize().unwrap_or(path);
        for folder in crate::records::recovery_sessions(roots)? {
            let Ok(entries) = crate::log::read_all(&folder) else {
                continue;
            };
            // The session's root holds `.basecraft/recovery/<session>`.
            let root = folder.ancestors().nth(3).unwrap_or(&folder);
            let names = |doc: &str| {
                let named = root.join(doc);
                named.canonicalize().unwrap_or(named) == path
            };
            if folder != self.storage_root && entries.iter().any(|entry| names(&entry.doc)) {
                self.docs[id.0].recovery_notice = Some(format!(
                    "Unsaved edits to this file from an earlier session are kept in recovery at {}. Recover them before you edit here.",
                    folder.display()
                ));
                break;
            }
        }
        Ok(())
    }

    /// Make the recovery folder before the first edit is recorded. Recovery
    /// lives only in the folder's `.basecraft` (D13): where that cannot be
    /// written, the edit is refused with an error that names the folder.
    pub(super) fn start_recovery(&self) -> Result<()> {
        if self.log.is_none() || self.storage_root == self.root || self.storage_root.is_dir() {
            return Ok(());
        }
        let foreign = crate::records::input_evidence::foreign_basecraft(&self.root);
        let reason = if let Some(reason) = foreign {
            reason.to_owned()
        } else {
            match recovery_directory(&self.storage_root) {
                Ok(()) => return Ok(()),
                Err(error) => error.to_string(),
            }
        };
        Err(crate::Error::ReadOnlyFolder {
            folder: self.root.clone(),
            reason,
        })
    }

    /// Cheap change token for readers; saves and job outcomes also invalidate a report.
    pub fn record_revision(&self, id: DocId) -> Result<String> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        Ok(format!(
            "{}:{}:{}:{}",
            self.id, id.0, slot.generation, self.journaled
        ))
    }

    /// Read-only evidence for this session's exact current revision.
    pub fn document_record(
        &self,
        id: DocId,
    ) -> Result<crate::records::document_record::DocumentRecord> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let mut record = match &slot.origin {
            Some(origin) => crate::records::document_record::DocumentRecord::from_history(
                origin,
                slot.origin_history.clone(),
            ),
            None => crate::records::document_record::DocumentRecord::empty(&slot.key),
        };
        if let Some(origin) = &slot.origin {
            record
                .origins
                .push((slot.origin_bundle.clone(), origin.clone()));
        }
        record.document.clone_from(&slot.key);
        record.run_root = Some(self.run_base(id)?);
        record.revision = format!("{}:{}:{}", self.id, id.0, slot.generation);
        record.unsaved = !slot.is_clean();
        if let Some(warning) = self.recovery_warning(id) {
            record.warnings.push(warning.into());
        }
        if let Some(issue) = &slot.origin_issue {
            record.warnings.push(issue.clone());
        }
        if !slot.baseline_logged
            && let Some(baseline) = &slot.baseline
        {
            record.events.push(baseline.clone());
        }
        record.events.extend(slot.recorded.iter().cloned());
        record.events.extend(slot.unsaved.iter().cloned());
        record.events.extend(
            self.pending_baselines
                .iter()
                .filter(|e| e.doc == slot.key)
                .cloned(),
        );
        record.effective.extend(slot.effective());
        record.deduplicate();
        crate::records::contributions::project(&record.data.events, &mut record.data.effective);
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
        if let Some((path, stamp, hash)) = &slot.saved_target {
            let current = crate::records::input_evidence::stamp(path)?;
            if current != *stamp {
                return Err(crate::Error::ChangedOnDisk { path: path.clone() });
            }
            return Ok(Some((path.clone(), current, hash.clone())));
        }
        let path = self.resolve(&slot.key).canonicalize()?;
        let current = crate::records::input_evidence::stamp(&path)?;
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

    pub fn snapshot_file(&self, path: &Path) -> Result<String> {
        let bytes = std::fs::read(path)?;
        basecraft_io::counters::record_read(path, bytes.len() as u64);
        self.snapshot_bytes(&bytes)
    }

    /// Hash and retain the exact immutable buffer consumed by an analysis.
    pub fn snapshot_bytes(&self, bytes: &[u8]) -> Result<String> {
        let hash = basecraft_io::hash_bytes(bytes);
        if self.store_objects {
            self.start_recovery()?;
        }
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

/// Checkpoints and other exact bytes a session keeps, by hash.
pub fn objects_dir(root: &Path) -> PathBuf {
    root.join(".basecraft").join("objects")
}

/// Where recovery of unsaved edits lives for sessions rooted at `root`.
pub fn recovery_folder(root: &Path) -> PathBuf {
    root.join(".basecraft").join("recovery")
}

/// Publish newly created directory entries before a synced journal can name them.
fn recovery_directory(path: &Path) -> std::io::Result<()> {
    let missing: Vec<_> = path.ancestors().take_while(|dir| !dir.exists()).collect();
    std::fs::create_dir_all(path)?;
    for directory in missing.into_iter().rev() {
        if let Some(parent) = directory.parent() {
            basecraft_platform::files::sync_directory(parent)?;
        }
    }
    Ok(())
}
