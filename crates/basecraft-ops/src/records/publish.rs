//! Provenance publication from a session's document history.
use crate::error::invalid;
use crate::{Error, Result};
use basecraft_platform::settings::ProvenancePolicy;
use basecraft_protocol::provenance::Publication;
use basecraft_protocol::{Actor, LogEntry, Op};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What publication reads from a session about one document.
pub(crate) struct History<'a> {
    pub(crate) session: &'a str,
    pub(crate) policy: ProvenancePolicy,
    pub(crate) tracked: bool,
    pub(crate) root: &'a Path,
    pub(crate) project: Option<&'a crate::project::Project>,
    pub(crate) storage_root: &'a Path,
    pub(crate) local_root: &'a Path,
    pub(crate) events: Vec<LogEntry>,
    pub(crate) pending: &'a [LogEntry],
    pub(crate) key: &'a str,
    /// Every edit in effect, oldest first.
    pub(crate) effective: Vec<basecraft_protocol::provenance::EditActivity>,
    pub(crate) origin: Option<&'a Publication>,
    pub(crate) origin_bundle: &'a Path,
    pub(crate) delimitation: bool,
}

impl History<'_> {
    /// Publish against the evidence frozen when the job was submitted.
    pub(crate) fn analysis(
        &self,
        target: &Path,
        inputs: &[(String, String)],
        snapshot: &crate::records::document_record::DocumentRecord,
    ) -> Result<()> {
        let _scope = crate::records::input_evidence::lock_in(self.project)?;
        if !self.tracked || self.policy == ProvenancePolicy::None {
            return Ok(());
        }
        let mut activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == self.key && matches!(e.op, Op::ToolRun { .. }))
            .cloned()
            .ok_or_else(|| invalid("analysis has no recorded activity"))?;
        activity.result.hashes.retain(|pair| !inputs.contains(pair));
        self.save(target, &activity, inputs, Some(snapshot))?
            .publish()
            .map(|_| ())
    }

    /// Stage the history publication for a save; the caller publishes it.
    /// Pending recovery baselines join the journal with the Save record, so
    /// they count as recorded events here.
    pub(crate) fn save(
        &self,
        target: &Path,
        save: &LogEntry,
        analysis_inputs: &[(String, String)],
        snapshot: Option<&crate::records::document_record::DocumentRecord>,
    ) -> Result<crate::documents::PreparedWrite> {
        use basecraft_protocol::provenance::{ArtifactRevision, Completeness, EditActivity};
        let _scope = crate::records::input_evidence::lock_in(self.project)?;
        let bundle = match self.policy {
            ProvenancePolicy::Portable => {
                if let Some(hidden) =
                    crate::records::analysis_store::ArtifactRun::for_result(target)?
                {
                    hidden
                } else if let Some(bundle) = self
                    .project
                    .and_then(|project| project.provenance_bundle(target))
                {
                    bundle
                } else {
                    crate::records::provenance::companion(target)
                }
            }
            ProvenancePolicy::LocalOnly => self.local_root.join(
                basecraft_io::hash_bytes(target.as_os_str().as_encoded_bytes())
                    .trim_start_matches("b3:"),
            ),
            ProvenancePolicy::None => {
                return Ok(crate::documents::PreparedWrite::default());
            }
        };
        std::fs::create_dir_all(bundle.join("artifacts"))?;
        let source_bundle = snapshot.and_then(|r| r.origins.first()).map_or_else(
            || self.origin_bundle.to_path_buf(),
            |(path, _)| path.clone(),
        );
        let mut previous_bundle = if bundle.join("manifest.json").exists() {
            &bundle
        } else {
            &source_bundle
        };
        let mut previous = if snapshot.is_some() {
            snapshot
                .and_then(|r| r.origins.first())
                .map(|(_, p)| p.clone())
        } else if previous_bundle.join("manifest.json").is_file() {
            let publication: Publication = serde_json::from_slice(
                &crate::records::provenance::read_record(&previous_bundle.join("manifest.json"))?,
            )?;
            if publication.schema_version != 1 {
                return Err(invalid("unsupported parent provenance version"));
            }
            Some(publication)
        } else {
            None
        };
        // A newly allocated output-support record is not the accepted document
        // history. Prefer the opened origin when the writer has just bound one.
        if snapshot.is_none()
            && previous_bundle != &source_bundle
            && previous
                .as_ref()
                .is_some_and(|p| p.events.is_empty() && p.effective_edits.is_empty())
            && source_bundle.join("manifest.json").is_file()
        {
            previous_bundle = &source_bundle;
            let publication: Publication = serde_json::from_slice(
                &crate::records::provenance::read_record(&source_bundle.join("manifest.json"))?,
            )?;
            if publication.schema_version != 1 {
                return Err(crate::Error::Unsupported(
                    "unsupported parent provenance version".into(),
                ));
            }
            previous = Some(publication);
        }
        let baseline_hash = self
            .events
            .iter()
            .chain(self.pending)
            .find(|e| e.doc == self.key && matches!(e.op, Op::Open { .. }))
            .and_then(|e| e.result.hashes.first())
            .map(|(_, hash)| hash);
        let previous = previous.filter(|p| {
            snapshot.is_some()
                || p.session == self.session
                || p.outputs.first().map(|a| &a.hash) == baseline_hash
        });
        let mut parents = previous
            .as_ref()
            .map(|p| vec![p.id.clone()])
            .unwrap_or_default();
        // Save As carries the contributing history rather than leaving dangling
        // references into the source's companion.
        if previous.is_some() && previous_bundle != &bundle {
            for subdirectory in ["artifacts", "records"] {
                copy_ancestry(
                    &previous_bundle.join(subdirectory),
                    &bundle.join(subdirectory),
                    0,
                )?;
            }
            if self.delimitation {
                // Species publications retain immutable generated files in their
                // run. Copied parent records still resolve those checked paths
                // in the new bundle; clean source references remain references.
                for entry in std::fs::read_dir(previous_bundle.join("records"))? {
                    let entry = entry?;
                    let record: Publication = serde_json::from_slice(
                        &crate::records::provenance::read_record(&entry.path())?,
                    )?;
                    for output in record.outputs {
                        let Some(name) = output.retained else {
                            continue;
                        };
                        if Path::new(&name)
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                        {
                            return Err(invalid("Unsafe retained species output path"));
                        }
                        let destination = bundle.join(&name);
                        if destination.is_file() {
                            continue;
                        }
                        let source = previous_bundle.join(&name);
                        if basecraft_io::hash_file(&source)? != output.hash {
                            return Err(invalid("Retained species output changed"));
                        }
                        if let Some(parent) = destination.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        basecraft_io::write_atomic(&destination, |w| {
                            std::io::copy(&mut std::fs::File::open(&source)?, w)?;
                            Ok(())
                        })?;
                        if basecraft_io::hash_file(&destination)? != output.hash {
                            return Err(invalid("Species output changed during Save As"));
                        }
                    }
                }
            }
        }
        // Save As keeps a live reference to the contributing run's evidence
        // decisions; it does not automatically copy unchanged source files.
        if previous.is_some()
            && previous_bundle != &bundle
            && previous_bundle.join("input.json").is_file()
        {
            let mut input = crate::records::input_evidence::read_input(previous_bundle)?;
            let mut upstream = input.upstream.clone();
            for reference in &mut upstream {
                reference.bundle = crate::records::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &previous_bundle.join(&reference.bundle).canonicalize()?,
                )?
                .into();
            }
            if bundle.join("input.json").is_file() {
                let current = crate::records::input_evidence::read_input(&bundle)?;
                if let Some(dependencies) = Some(&current.upstream) {
                    for dependency in dependencies {
                        if !upstream.contains(dependency) {
                            upstream.push(dependency.clone());
                        }
                    }
                }
            }
            if !upstream.is_empty() {
                input.upstream = upstream;
            }
            if input.source.is_some() {
                input.origin_run = Some(crate::records::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &previous_bundle.canonicalize()?,
                )?);
            } else if let Some(previous) =
                crate::records::analysis_store::submitted_input(previous_bundle)
            {
                basecraft_io::write_atomic(&bundle.join(previous.file_name().unwrap()), |w| {
                    std::io::copy(&mut std::fs::File::open(&previous)?, w)?;
                    Ok(())
                })?;
            }
            crate::records::input_evidence::write_json(&bundle.join("input.json"), &input)?;
        }
        let retain =
            |path: &Path, hash: &str, revision: String, name: String| -> Result<ArtifactRevision> {
                let bytes = std::fs::read(path)?;
                #[cfg(feature = "test-counters")]
                basecraft_io::counters::record_read(path, bytes.len() as u64);
                if basecraft_io::hash_bytes(&bytes) != hash {
                    return Err(Error::ChangedOnDisk {
                        path: path.to_path_buf(),
                    });
                }
                let relative = format!("artifacts/{}", hash.trim_start_matches("b3:"));
                let dest = bundle.join(&relative);
                if !dest.exists() {
                    basecraft_io::write_atomic(&dest, |w| w.write_all(&bytes))?;
                } else if basecraft_io::hash_file(&dest)? != hash {
                    return Err(Error::Damaged {
                        path: dest,
                        detail: "retained artifact hash differs".into(),
                    });
                }
                Ok(ArtifactRevision {
                    id: revision,
                    logical_name: name,
                    hash: hash.into(),
                    size: bytes.len() as u64,
                    retained: Some(relative),
                    reference: None,
                })
            };
        let mut inputs = Vec::new();
        for event in self.events.iter().chain(self.pending).filter(|e| {
            snapshot.is_none_or(|r| r.document == self.key)
                && e.doc == self.key
                && matches!(e.op, Op::Open { .. })
        }) {
            for (key, hash) in &event.result.hashes {
                inputs.push(ArtifactRevision {
                    id: format!("{}:input:{}", event.id, inputs.len()),
                    logical_name: name(key),
                    hash: hash.clone(),
                    size: event
                        .source
                        .as_ref()
                        .and_then(|source| source.files.iter().find(|file| file.path == *key))
                        .map_or(0, |file| file.stamp.size),
                    retained: None,
                    reference: None,
                });
            }
        }
        for event in self
            .events
            .iter()
            .chain(self.pending)
            .filter(|event| event.doc == self.key)
        {
            if let Op::CreateSangerReview { review } | Op::SangerUpdate { review } = &event.op {
                let source = crate::session::objects_dir(self.storage_root)
                    .join(review.hash.trim_start_matches("b3:"));
                inputs.push(retain(
                    &source,
                    &review.hash,
                    format!("{}:review", event.id),
                    "Sanger analysis record".into(),
                )?);
            }
        }
        for (key, hash) in analysis_inputs {
            let mut source =
                crate::session::objects_dir(self.storage_root).join(hash.trim_start_matches("b3:"));
            if !source.is_file() {
                source = self.root.join(key);
            }
            inputs.push(retain(
                &source,
                hash,
                format!("{}:submitted:{}", save.id, inputs.len()),
                name(key),
            )?);
        }
        let mut outputs = Vec::new();
        for (key, hash) in &save.result.hashes {
            let logical_name =
                if crate::records::analysis_store::ArtifactRun::for_result(target)?.is_some() {
                    crate::records::input_evidence::relative(
                        &target.parent().unwrap_or(Path::new(".")).canonicalize()?,
                        &self.root.join(key).canonicalize()?,
                    )?
                } else {
                    name(key)
                };
            outputs.push(ArtifactRevision {
                id: format!("{}:output:{}", save.id, outputs.len()),
                logical_name,
                hash: hash.clone(),
                size: save
                    .source
                    .as_ref()
                    .and_then(|source| source.files.iter().find(|file| file.path == *key))
                    .map_or(0, |file| file.stamp.size),
                retained: None,
                reference: None,
            });
        }
        // Public reports use logical names and anonymous agents. Original local
        // journals retain exact paths for recovery; those paths are not portable.
        let mut events: Vec<_> = self
            .events
            .iter()
            .chain(self.pending)
            .filter(|e| e.doc == self.key)
            .cloned()
            .collect();
        // A save's own record is staged with its history, before the journal append.
        if !events.iter().any(|event| event.id == save.id) {
            events.push(save.clone());
        }
        for event in &mut events {
            event.doc = name(&target.to_string_lossy());
            if let Some(source) = &mut event.source {
                source.root = PathBuf::from(".");
                for file in &mut source.files {
                    file.path = name(&file.path);
                }
            }
            if let Op::Open { path } | Op::Save { path, .. } = &mut event.op {
                path.clone_from(&event.doc);
            }
            portable(event);
        }
        let mut effective_edits = self.effective.clone();
        if matches!(save.op, Op::ToolRun { .. }) {
            effective_edits.push(EditActivity {
                id: save.id.clone(),
                operation: save.op.clone(),
            });
        }
        let inherited = if snapshot.is_some() {
            previous
        } else {
            self.origin.cloned().or(previous)
        };
        if let Some(previous) = inherited
            && previous.session != self.session
        {
            let mut inherited = previous.effective_edits;
            inherited.extend(effective_edits);
            effective_edits = inherited;
            let mut inherited_events = previous.events;
            inherited_events.extend(events);
            events = inherited_events;
            for input in previous.inputs {
                if !inputs.iter().any(|a| a.id == input.id) {
                    inputs.push(input);
                }
            }
        }
        if let Some(snapshot) = snapshot {
            events.clone_from(&snapshot.events);
            events.push(save.clone());
            effective_edits.clone_from(&snapshot.effective);
            effective_edits.push(EditActivity {
                id: save.id.clone(),
                operation: save.op.clone(),
            });
        }
        events.iter_mut().for_each(portable);
        for activity in &mut effective_edits {
            redact_operation_paths(&mut activity.operation);
        }
        if let Some(snapshot) = snapshot {
            for (origin_bundle, origin) in &snapshot.origins {
                if !parents.contains(&origin.id) {
                    parents.push(origin.id.clone());
                }
                for input in &origin.inputs {
                    if !inputs.iter().any(|a| a.id == input.id) {
                        inputs.push(input.clone());
                    }
                }
                if origin_bundle != &bundle {
                    for folder in ["records", "artifacts"] {
                        std::fs::create_dir_all(bundle.join(folder))?;
                        for entry in std::fs::read_dir(origin_bundle.join(folder))? {
                            let entry = entry?;
                            if !entry.file_type()?.is_file() {
                                return Err(invalid("ancestor contains a non-file record"));
                            }
                            let destination = bundle.join(folder).join(entry.file_name());
                            if !destination.exists() {
                                basecraft_io::write_atomic(&destination, |w| {
                                    std::io::copy(&mut std::fs::File::open(entry.path())?, w)?;
                                    Ok(())
                                })?;
                            }
                        }
                    }
                }
            }
        }
        let mut parent_hashes = std::collections::BTreeMap::new();
        for parent in &parents {
            let path = bundle.join("records").join(format!("{parent}.json"));
            parent_hashes.insert(parent.clone(), basecraft_io::hash_file(&path)?);
        }
        crate::records::contributions::project(&events, &mut effective_edits);
        let input_bytes_available = inputs.iter().all(|input| input.retained.is_some());
        let publication = Publication {
            schema_version: 1,
            id: save.id.clone(),
            session: self.session.into(),
            parents,
            parent_hashes,
            created: save.t.clone(),
            origin: "external history before import is unknown".into(),
            inputs,
            outputs,
            events,
            effective_edits,
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available,
                execution_specification_available: false,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        crate::records::provenance::stage(&bundle, &publication)
    }

    /// Add a captured input revision and its recorded producing activity to an
    /// existing operation inventory. The caller keeps all inputs and outputs.
    pub(crate) fn complete_operation(
        &self,
        snapshot: &crate::records::document_record::DocumentRecord,
        run: &crate::records::analysis_store::ArtifactRun,
    ) -> Result<()> {
        let _scope = crate::records::input_evidence::lock_in(self.project)?;
        let activity = self
            .events
            .iter()
            .rev()
            .find(|event| {
                event.doc == self.key
                    && matches!(&event.op,
                Op::ToolRun { params, .. } if params["run_id"].as_str() == Some(run.id()))
            })
            .ok_or_else(|| invalid("analysis has no recorded activity"))?
            .clone();
        snapshot.publish_operation_activity(run, &activity)
    }

    /// Publish an extracted sequence's ancestry: the source revision it was
    /// cut from, or the exact unsaved bytes from `current` when the document
    /// has unsaved edits.
    pub(crate) fn extraction(
        &self,
        path: &Path,
        activity: &LogEntry,
        snapshot: &crate::records::DocumentRecord,
        source: Option<(PathBuf, basecraft_protocol::provenance::FileStamp, String)>,
        current: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<()> {
        use basecraft_protocol::provenance::{ArtifactRevision, Completeness, InputReference};
        let _scope = crate::records::input_evidence::lock_in(self.project)?;
        if !self.tracked || self.policy == ProvenancePolicy::None {
            return Ok(());
        }
        let run = if self.policy == ProvenancePolicy::Portable {
            Some(crate::records::ArtifactRun::create(path, &activity.id)?)
        } else {
            None
        };
        let bundle = run.as_ref().map_or_else(
            || {
                self.local_root.join(
                    basecraft_io::hash_bytes(path.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                )
            },
            |r| r.directory().to_path_buf(),
        );
        std::fs::create_dir_all(&bundle)?;
        let (reference, hash, size, logical_name) = if let Some((source, stamp, hash)) = source {
            let size = stamp.size;
            let reference = InputReference {
                path: crate::records::relative(&bundle.canonicalize()?, &source)?,
                stamp,
                hash: hash.clone(),
                extra: BTreeMap::default(),
            };
            (Some(reference), hash, size, name(&source.to_string_lossy()))
        } else {
            // Unsaved edits need their exact current generated input. This is
            // not an automatic copy of an unchanged original.
            let bytes = current()?;
            let submitted = crate::records::analysis_store::submitted_name(Path::new(self.key));
            basecraft_io::write_atomic(&bundle.join(submitted), |w| {
                std::io::Write::write_all(w, &bytes)
            })?;
            (
                None,
                basecraft_io::hash_bytes(&bytes),
                bytes.len() as u64,
                self.key.into(),
            )
        };
        let mut upstream = Vec::new();
        for dependency in snapshot.origin_bundles() {
            let bytes = crate::records::read_record(&dependency.join("manifest.json"))?;
            let parent: Publication = serde_json::from_slice(&bytes)?;
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: crate::records::relative(
                    &bundle.canonicalize()?,
                    &dependency.canonicalize()?,
                )?
                .into(),
                id: parent.id.clone(),
                hash: basecraft_io::hash_bytes(&bytes),
            });
        }
        let extraction = basecraft_protocol::records::ExtractionRecord {
            schema_version: 1,
            source_revision: snapshot.revision.clone(),
            selection: activity.op.clone(),
            coordinates: "zero-based-half-open; segments in biological order; reverse-complement after splicing".into(),
            extra: BTreeMap::new(),
        };
        let input = crate::records::SubmittedInput {
            schema_version: 1,
            source: reference.clone(),
            submitted_hash: hash.clone(),
            submitted_size: size,
            transformation: "identity-v1".into(),
            state: if reference.is_some() {
                "referenced"
            } else {
                "generated"
            }
            .into(),
            analysis_run: None,
            proposal_target: None,
            upstream,
            origin_run: None,
            extra: BTreeMap::from([("extraction".into(), serde_json::to_value(extraction)?)]),
        };
        crate::records::write_json(&bundle.join("input.json"), &input)?;
        if let Some(run) = &run {
            run.write_run(
                basecraft_protocol::records::AnalysisRunStatus::Saved,
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned),
            )?;
            crate::records::register(run)?;
        }
        let artifact = ArtifactRevision {
            id: format!("{}:source", activity.id),
            logical_name,
            hash,
            size,
            retained: reference.is_none().then(|| {
                crate::records::analysis_store::submitted_name(Path::new(self.key)).into()
            }),
            reference,
        };
        let (events, effective_edits) = portable_analysis_activity(snapshot, activity);
        let publication = Publication {
            schema_version: 1,
            id: activity.id.clone(),
            session: self.session.into(),
            parents: Vec::new(),
            parent_hashes: BTreeMap::default(),
            created: activity.t.clone(),
            origin: "Sequence extraction from the recorded source revision".into(),
            inputs: vec![artifact],
            outputs: vec![ArtifactRevision {
                id: format!("{}:result", activity.id),
                logical_name: name(&path.to_string_lossy()),
                hash: basecraft_io::hash_file(path)?,
                size: std::fs::metadata(path)?.len(),
                retained: None,
                reference: None,
            }],
            events,
            effective_edits,
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        crate::records::publish(&bundle, &publication)?;
        if let Some(run) = &run {
            run.bind(path)?;
        }
        Ok(())
    }
}

/// A path's file name, for records that must not carry local paths.
fn name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Drop the host, user and local paths from a journal event for a public record.
fn portable(event: &mut LogEntry) {
    event.host.clear();
    event.doc = name(&event.doc);
    if matches!(event.actor, Actor::Human { .. }) {
        event.actor = Actor::Human {
            user: "redacted".into(),
        };
    }
    redact_operation_paths(&mut event.op);
    for (key, _) in &mut event.result.hashes {
        *key = name(key);
    }
}

pub(crate) fn portable_analysis_activity(
    snapshot: &crate::records::document_record::DocumentRecord,
    activity: &LogEntry,
) -> (
    Vec<LogEntry>,
    Vec<basecraft_protocol::provenance::EditActivity>,
) {
    use basecraft_protocol::provenance::EditActivity;
    let mut events = snapshot.events.clone();
    if !events.iter().any(|event| event.id == activity.id) {
        events.push(activity.clone());
    }
    let mut effective = snapshot.effective.clone();
    effective.push(EditActivity {
        id: activity.id.clone(),
        operation: activity.op.clone(),
    });
    events.iter_mut().for_each(portable);
    for edit in &mut effective {
        redact_operation_paths(&mut edit.operation);
    }
    (events, effective)
}

fn redact_operation_paths(operation: &mut Op) {
    fn scrub(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) if Path::new(text).is_absolute() => {
                *text = name(text);
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    scrub(value);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    scrub(value);
                }
            }
            _ => {}
        }
    }
    if let Op::CreateSangerReview { review } | Op::SangerUpdate { review } = operation {
        review.path = Path::new("artifacts").join(review.hash.trim_start_matches("b3:"));
    }
    if let Op::ToolRun {
        inputs,
        outputs,
        params,
        ..
    } = operation
    {
        for path in inputs.iter_mut().chain(outputs.iter_mut()) {
            *path = name(path);
        }
        scrub(params);
    }
}

/// Copy a parent bundle's ancestry folder into a new bundle, keeping existing
/// files. A registered run's retained collection lives under `artifacts/` and
/// may hold folders; anything other than files and folders is refused.
fn copy_ancestry(from: &Path, to: &Path, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err(invalid("parent bundle ancestry is nested too deeply"));
    }
    std::fs::create_dir_all(to)?;
    if !from.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() {
            copy_ancestry(&entry.path(), &destination, depth + 1)?;
        } else if !kind.is_file() {
            return Err(invalid("parent bundle contains non-file ancestry"));
        } else if !destination.exists() {
            basecraft_io::write_atomic(&destination, |w| {
                std::io::copy(&mut std::fs::File::open(entry.path())?, w)?;
                Ok(())
            })?;
        }
    }
    Ok(())
}
