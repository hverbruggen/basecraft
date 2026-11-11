//! Document loading, baseline tracking and retained originals.
#[cfg(test)]
use super::retention_tests;
use super::{DocId, Op, OpResult, Path, PathBuf, Result, Slot, files, invalid, objects_dir};
use super::{OBJECT_STORE_LIMIT, Session, save_authority};

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
        self.flush()?;
        let disk_stamp = crate::input_evidence::stamp(path)?;
        let format = files::Format::from_path(path)?;
        // Files read with the main one: a sidecar, or a metadata manifest's tables.
        let companions = match format {
            files::Format::SangerReview => Vec::new(),
            files::Format::Metadata => crate::metadata::files(path).split_off(1),
            files::Format::Tree | files::Format::Delimitation => vec![files::tree_sidecar(path)],
            files::Format::GenBank | files::Format::Fasta => vec![files::gff_sidecar(path)],
            files::Format::Submission => crate::documents::submission_companions(path),
        };
        // A captured loader couples parser bytes and hashes, but companions
        // still require final content checks before admission.
        let captured = files::capture_load(path, format)?;
        let owned_main = captured.is_none()
            && matches!(
                format,
                files::Format::Fasta | files::Format::GenBank | files::Format::Tree
            )
            && !companions.iter().any(|c| c.exists());
        let hash_companions = || -> Result<Vec<Option<String>>> {
            companions
                .iter()
                .map(|c| {
                    if c.exists() {
                        basecraft_io::hash_file(c).map(Some).map_err(Into::into)
                    } else {
                        Ok(None)
                    }
                })
                .collect()
        };
        let (content, accepted_hash, companion_hashes) = if let Some(captured) = captured {
            (
                captured.content,
                captured.main_hash,
                captured.companion_hashes,
            )
        } else {
            let (source, accepted_hash) = if owned_main {
                let (source, hash) = basecraft_io::Source::open_hashed(path)?;
                (Some(source), hash)
            } else {
                (None, basecraft_io::hash_file(path)?)
            };
            let companion_hashes = hash_companions()?;
            #[cfg(test)]
            files::admission_tests::boundary("acquired");
            let content = files::load_snapshot(path, source, format, verified_snapshot)?;
            (content, accepted_hash, companion_hashes)
        };
        #[cfg(test)]
        files::admission_tests::boundary("parsed");
        if (!owned_main && basecraft_io::hash_file(path)? != accepted_hash)
            || hash_companions()? != companion_hashes
            || crate::input_evidence::stamp(path)? != disk_stamp
        {
            return Err(invalid(
                "source or sidecar changed while opening; retry with stable files",
            ));
        }
        let key = self.relative(path);
        let mut hashes = vec![(key.clone(), accepted_hash)];
        let absent_sidecars = companions
            .iter()
            .zip(&companion_hashes)
            .filter(|(_, hash)| hash.is_none())
            .map(|(c, _)| self.relative(c))
            .collect();
        if content.as_document().retains_companions() {
            for (companion, hash) in companions.iter().zip(companion_hashes) {
                if let Some(hash) = hash {
                    hashes.push((self.relative(companion), hash));
                }
            }
        }
        let (origin, mut origin_issue) = crate::document_record::read_origin(path, &hashes);
        if let Some(warning) = content.as_document().opening_warning() {
            origin_issue = Some(
                origin_issue
                    .map_or_else(|| warning.to_owned(), |issue| format!("{issue}\n{warning}")),
            );
        }
        let kind = content.kind();
        self.docs.push(Slot {
            key: key.clone(),
            content,
            current_saved_hashes: std::sync::OnceLock::new(),
            generation: 0,
            save_authority: save_authority::PendingSaveAuthority::default(),
            undo: vec![],
            redo: vec![],
            baseline: None,
            baseline_logged: false,
            accepted_hashes: hashes.clone(),
            saved_state_hashes: Vec::new(),
            saved_target: None,
            absent_sidecars,
            origin,
            origin_issue,
            origin_bundle: crate::provenance::bundle_for_result(path),
            disk_stamp,
        });
        let id = DocId(self.docs.len() - 1);
        self.docs[id.0].saved_state_hashes = self.save_hashes(id)?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );
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
            for (key, hash) in &entry.result.hashes {
                let source = self.resolve(key);
                if self.store_objects {
                    self.store_object(&source, hash)?;
                } else {
                    if basecraft_io::hash_file(&source)? != *hash {
                        return Err(invalid(
                            "source changed since opening; reopen it before editing",
                        ));
                    }
                    self.snapshot_file(&source)?;
                }
            }
        }
        if self.log.is_some() {
            let baseline = serde_json::to_value(&entry)?;
            let manifest = serde_json::to_vec_pretty(&serde_json::to_value(
                basecraft_protocol::records::RecoveryManifest {
                    schema_version: 1,
                    session: self.id.clone(),
                    source_root: self.root.clone(),
                    status: basecraft_protocol::records::RecoveryStatus::Unsaved,
                    baseline,
                    object_policy_limit: OBJECT_STORE_LIMIT,
                    extra: std::collections::BTreeMap::new(),
                },
            )?)?;
            std::fs::create_dir_all(&self.storage_root)?;
            basecraft_io::write_atomic(&self.storage_root.join("recovery.json"), |w| {
                w.write_all(&manifest)
            })?;
        }
        if !self.docs[id.0].baseline_logged {
            self.write(&entry)?;
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
        record.unsaved = self.save_hashes(id)? != slot.saved_state_hashes;
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
        if let Some(pending) = &self.pending
            && pending.doc == id
        {
            record.events.push(pending.entry.clone());
        }
        record.effective.extend(
            slot.undo
                .iter()
                .filter(|a| record.events.iter().any(|event| event.id == a.entry_id))
                .map(|a| EditActivity {
                    id: a.entry_id.clone(),
                    operation: a.op.clone(),
                }),
        );
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
        if self.save_hashes(id)? != slot.saved_state_hashes {
            return Ok(None);
        }
        if let Some((path, stamp, hash)) = &slot.saved_target {
            let current = crate::input_evidence::stamp(path)?;
            if current != *stamp {
                return Err(crate::Error::ChangedOnDisk { path: path.clone() });
            }
            return Ok(Some((path.clone(), current, hash.clone())));
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
            return Err(invalid(format!(
                "exact input retention exceeds the {OBJECT_STORE_LIMIT}-byte policy limit"
            )));
        }
        if self.tracked && !self.store_objects {
            self.memory_objects
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(hash.clone())
                .or_insert_with(|| bytes.to_vec());
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

    pub(super) fn store_object(&self, path: &Path, hash: &str) -> Result<()> {
        let dest = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
        // These probes select an acquisition strategy, never establish identity.
        // Keep streaming verification (and its errors) for known/oversize inputs.
        let capture =
            !dest.exists() && std::fs::metadata(path).is_ok_and(|m| m.len() <= OBJECT_STORE_LIMIT);
        #[cfg(test)]
        retention_tests::boundary(retention_tests::Stage::BeforeRead, path);
        let (captured, current_hash) = if capture {
            let (source, current_hash) = basecraft_io::Source::open_hashed(path)?;
            (Some(source), current_hash)
        } else {
            (None, basecraft_io::hash_file(path)?)
        };
        if current_hash != hash {
            return Err(invalid(
                "source changed since opening; reopen it before editing",
            ));
        }
        #[cfg(test)]
        retention_tests::boundary(retention_tests::Stage::AfterRead, path);
        let size = std::fs::metadata(path)?.len();
        if size > OBJECT_STORE_LIMIT {
            return Err(invalid(format!(
                "original input is {size} bytes; exact recovery retention currently supports at most {OBJECT_STORE_LIMIT} bytes"
            )));
        }
        if let Some(source) = &captured {
            let size = source.bytes().len() as u64;
            if size > OBJECT_STORE_LIMIT {
                return Err(invalid(format!(
                    "original input is {size} bytes; exact recovery retention currently supports at most {OBJECT_STORE_LIMIT} bytes"
                )));
            }
        }
        if !dest.exists() {
            std::fs::create_dir_all(objects_dir(&self.storage_root))?;
            let fallback;
            let bytes = if let Some(source) = &captured {
                source.bytes()
            } else {
                fallback = std::fs::read(path)?;
                #[cfg(feature = "test-counters")]
                basecraft_io::counters::record_read(path, fallback.len() as u64);
                if basecraft_io::hash_bytes(&fallback) != hash {
                    return Err(invalid("source changed while retaining its snapshot"));
                }
                &fallback
            };
            basecraft_io::write_atomic(&dest, |w| w.write_all(bytes))?;
        }
        Ok(())
    }
}
