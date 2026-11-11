//! Document and analysis provenance publication.
use super::Session;
use super::{
    Actor, DocId, Error, LogEntry, Op, Path, Result, copy_ancestry, files, invalid,
    portable_analysis_activity,
};
use super::{PathBuf, objects_dir, redact_operation_paths};

impl Session {
    /// Publish exact submitted analysis inputs and its output ancestry after a
    /// validated successful `ToolRun`. Failed attempts remain local job evidence.
    pub fn publish_analysis(
        &self,
        id: DocId,
        target: &Path,
        inputs: &[(String, String)],
    ) -> Result<()> {
        if !self.tracked
            || self.provenance_policy == basecraft_platform::settings::ProvenancePolicy::None
        {
            return Ok(());
        }
        let mut activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::ToolRun { .. }))
            .cloned()
            .ok_or_else(|| invalid("analysis has no recorded activity"))?;
        activity.result.hashes.retain(|pair| !inputs.contains(pair));
        self.publish_save(id, target, &activity, inputs, None)
    }

    /// Publish against the evidence frozen when the job was submitted.
    pub fn publish_analysis_snapshot(
        &self,
        id: DocId,
        target: &Path,
        inputs: &[(String, String)],
        snapshot: &crate::records::document_record::DocumentRecord,
    ) -> Result<()> {
        if !self.tracked
            || self.provenance_policy == basecraft_platform::settings::ProvenancePolicy::None
        {
            return Ok(());
        }
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let mut activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == slot.key && matches!(e.op, Op::ToolRun { .. }))
            .cloned()
            .ok_or_else(|| invalid("analysis has no recorded activity"))?;
        activity.result.hashes.retain(|pair| !inputs.contains(pair));
        self.publish_save(id, target, &activity, inputs, Some(snapshot))
    }

    pub(super) fn publish_save(
        &self,
        id: DocId,
        target: &Path,
        save: &LogEntry,
        analysis_inputs: &[(String, String)],
        snapshot: Option<&crate::records::document_record::DocumentRecord>,
    ) -> Result<()> {
        use basecraft_protocol::provenance::{
            ArtifactRevision, Completeness, EditActivity, Publication,
        };
        let bundle = match self.provenance_policy {
            basecraft_platform::settings::ProvenancePolicy::Portable => {
                if let Some(hidden) = crate::analysis_store::ArtifactRun::for_result(target)? {
                    hidden
                } else if self.root.join(".basecraft/project.toml").is_file() {
                    self.root.join(".basecraft/provenance").join(
                        basecraft_io::hash_bytes(self.relative(target).as_bytes())
                            .trim_start_matches("b3:"),
                    )
                } else {
                    crate::records::provenance::companion(target)
                }
            }
            basecraft_platform::settings::ProvenancePolicy::LocalOnly => {
                self.local_provenance_root.join(
                    basecraft_io::hash_bytes(target.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                )
            }
            basecraft_platform::settings::ProvenancePolicy::None => return Ok(()),
        };
        std::fs::create_dir_all(bundle.join("artifacts"))?;
        let source_bundle = snapshot.and_then(|r| r.origins.first()).map_or_else(
            || self.docs[id.0].origin_bundle.clone(),
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
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::Open { .. }))
            .and_then(|e| e.result.hashes.first())
            .map(|(_, hash)| hash);
        let previous = previous.filter(|p| {
            snapshot.is_some()
                || p.session == self.id
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
            if self.is_delimitation(id) {
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
            let mut input = crate::input_evidence::read_input(previous_bundle)?;
            let mut upstream = input.upstream.clone();
            for reference in &mut upstream {
                reference.bundle = crate::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &previous_bundle.join(&reference.bundle).canonicalize()?,
                )?
                .into();
            }
            if bundle.join("input.json").is_file() {
                let current = crate::input_evidence::read_input(&bundle)?;
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
                input.origin_run = Some(crate::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &previous_bundle.canonicalize()?,
                )?);
            } else if previous_bundle.join("submitted.fasta").is_file() {
                basecraft_io::write_atomic(&bundle.join("submitted.fasta"), |w| {
                    std::io::copy(
                        &mut std::fs::File::open(previous_bundle.join("submitted.fasta"))?,
                        w,
                    )?;
                    Ok(())
                })?;
            }
            crate::input_evidence::write_json(&bundle.join("input.json"), &input)?;
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
        for event in self.events.iter().filter(|e| {
            snapshot.is_none_or(|r| r.document == self.docs[id.0].key)
                && e.doc == self.docs[id.0].key
                && matches!(e.op, Op::Open { .. })
        }) {
            for (key, hash) in &event.result.hashes {
                inputs.push(ArtifactRevision {
                    id: format!("{}:input:{}", event.id, inputs.len()),
                    logical_name: Path::new(key)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
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
            .filter(|event| event.doc == self.docs[id.0].key)
        {
            if let Op::CreateSangerReview { review } | Op::SangerUpdate { review } = &event.op {
                let source =
                    objects_dir(&self.storage_root).join(review.hash.trim_start_matches("b3:"));
                inputs.push(retain(
                    &source,
                    &review.hash,
                    format!("{}:review", event.id),
                    "Sanger analysis record".into(),
                )?);
            }
        }
        for (key, hash) in analysis_inputs {
            let mut source = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
            if !source.is_file() {
                source = self.resolve(key);
            }
            inputs.push(retain(
                &source,
                hash,
                format!("{}:submitted:{}", save.id, inputs.len()),
                Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )?);
        }
        let mut outputs = Vec::new();
        for (key, hash) in &save.result.hashes {
            let logical_name = if crate::analysis_store::ArtifactRun::for_result(target)?.is_some()
            {
                crate::input_evidence::relative(
                    &target.parent().unwrap_or(Path::new(".")).canonicalize()?,
                    &self.resolve(key).canonicalize()?,
                )?
            } else {
                Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
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
            .filter(|e| e.doc == self.docs[id.0].key)
            .cloned()
            .collect();
        for event in &mut events {
            event.doc = target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            event.host.clear();
            if matches!(event.actor, Actor::Human { .. }) {
                event.actor = Actor::Human {
                    user: "redacted".into(),
                };
            }
            if let Some(source) = &mut event.source {
                source.root = PathBuf::from(".");
                source.links.clear();
                for file in &mut source.files {
                    file.path = Path::new(&file.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                }
            }
            match &mut event.op {
                Op::Open { path } | Op::Save { path, .. } => path.clone_from(&event.doc),
                _ => {}
            }
            redact_operation_paths(&mut event.op);
            for (key, _) in &mut event.result.hashes {
                *key = Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
        let mut effective_edits: Vec<_> = self.docs[id.0]
            .undo
            .iter()
            .map(|a| EditActivity {
                id: a.entry_id.clone(),
                operation: a.op.clone(),
            })
            .collect();
        if matches!(save.op, Op::ToolRun { .. }) {
            effective_edits.push(EditActivity {
                id: save.id.clone(),
                operation: save.op.clone(),
            });
        }
        for activity in &mut effective_edits {
            redact_operation_paths(&mut activity.operation);
        }
        let inherited = if snapshot.is_some() {
            previous
        } else {
            self.docs[id.0].origin.clone().or(previous)
        };
        if let Some(previous) = inherited
            && previous.session != self.id
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
        for event in &mut events {
            event.host.clear();
            event.doc = Path::new(&event.doc)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if matches!(event.actor, Actor::Human { .. }) {
                event.actor = Actor::Human {
                    user: "redacted".into(),
                };
            }
            redact_operation_paths(&mut event.op);
            for (key, _) in &mut event.result.hashes {
                *key = Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
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
            session: self.id.clone(),
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
        crate::records::provenance::publish(&bundle, &publication)
    }

    /// Add a captured input revision and its recorded producing activity to an
    /// existing operation inventory. The caller keeps all inputs and outputs.
    pub fn complete_operation_publication(
        &self,
        id: DocId,
        snapshot: &crate::records::document_record::DocumentRecord,
        run: &crate::analysis_store::ArtifactRun,
    ) -> Result<()> {
        let activity = self
            .events
            .iter()
            .rev()
            .find(|event| {
                self.doc_key(id).is_ok_and(|key| event.doc == key)
                    && matches!(&event.op,
                Op::ToolRun { params, .. } if params["run_id"].as_str() == Some(run.id()))
            })
            .ok_or_else(|| invalid("analysis has no recorded activity"))?
            .clone();
        snapshot.publish_operation_activity(run, &activity)
    }

    /// Reference-first publication: actual submitted input identity and the
    /// transformation are recorded in input.json; clean source bytes are not copied.
    pub fn publish_referenced_analysis(
        &self,
        id: DocId,
        target: &Path,
        outputs: &[PathBuf],
        snapshot: &crate::records::document_record::DocumentRecord,
        run: &crate::analysis_store::ArtifactRun,
    ) -> Result<()> {
        use basecraft_protocol::provenance::{ArtifactRevision, Completeness, Publication};
        let mut input = crate::input_evidence::read_input(run.directory())?;
        let mut upstream = input.upstream.clone();
        for (bundle, origin) in &snapshot.origins {
            let mut bytes = serde_json::to_vec_pretty(origin)?;
            bytes.push(b'\n');
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::input_evidence::relative(
                    run.directory(),
                    &bundle.canonicalize()?,
                )?)
                .into(),
                id: origin.id.clone(),
                hash: basecraft_io::hash_bytes(&bytes),
            });
        }
        if !upstream.is_empty() {
            input.upstream = upstream;
            crate::input_evidence::write_json(&run.directory().join("input.json"), &input)?;
        }
        let activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::ToolRun { .. }))
            .ok_or_else(|| invalid("analysis has no recorded activity"))?
            .clone();
        std::fs::create_dir_all(run.directory().join("artifacts"))?;
        let source = if let Some(reference) = input.source.clone() {
            ArtifactRevision {
                id: format!("{}:source", run.id()),
                logical_name: Path::new(&reference.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash: reference.hash.clone(),
                size: reference.stamp.size,
                retained: None,
                reference: Some(reference),
            }
        } else {
            ArtifactRevision {
                id: format!("{}:submitted", run.id()),
                logical_name: "submitted.fasta".into(),
                hash: input.submitted_hash.clone(),
                size: input.submitted_size,
                retained: Some("submitted.fasta".into()),
                reference: None,
            }
        };
        let mut inputs = vec![source];
        if let Some(retained) = input.extra.get("retained_inputs") {
            let additional: Vec<ArtifactRevision> = serde_json::from_value(retained.clone())?;
            if additional.len() > 64 {
                return Err(invalid("too many retained analysis inputs"));
            }
            for artifact in additional {
                let name = artifact
                    .retained
                    .as_deref()
                    .ok_or_else(|| invalid("auxiliary input is not retained"))?;
                if name.is_empty()
                    || Path::new(name)
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    || artifact.reference.is_some()
                {
                    return Err(invalid("unsafe retained analysis input"));
                }
                let path = run.directory().join(name);
                let metadata = std::fs::symlink_metadata(&path)?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || path.canonicalize()? != path
                    || metadata.len() != artifact.size
                    || basecraft_io::hash_file(&path)? != artifact.hash
                {
                    return Err(invalid("retained analysis input changed"));
                }
                inputs.push(artifact);
            }
        }
        let mut artifacts = Vec::new();
        for path in outputs {
            let retained = if !input.extra.contains_key("species_result") {
                None
            } else if *path == files::tree_sidecar(target) {
                let hash = basecraft_io::hash_file(path)?;
                let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
                basecraft_io::write_atomic(&run.directory().join(&name), |w| {
                    std::io::copy(&mut std::fs::File::open(path)?, w)?;
                    Ok(())
                })?;
                Some(name)
            } else {
                path.strip_prefix(run.directory())
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(str::to_owned)
            };
            artifacts.push(ArtifactRevision {
                id: format!("{}:output:{}", run.id(), artifacts.len()),
                logical_name: crate::input_evidence::relative(
                    target
                        .parent()
                        .unwrap_or(Path::new("."))
                        .canonicalize()?
                        .as_path(),
                    path,
                )?,
                hash: basecraft_io::hash_file(path)?,
                size: std::fs::metadata(path)?.len(),
                retained,
                reference: None,
            });
        }
        let (events, effective) = portable_analysis_activity(snapshot, &activity);
        let publication = Publication {
            schema_version: 1,
            id: activity.id, session: self.id.clone(), parents: Vec::new(), parent_hashes: std::collections::BTreeMap::new(),
            created: activity.t, origin: "Referenced source; submitted transformation recorded in input.json. Declared immutable upstream records are referenced without copying input bytes. Earlier history before import remains unknown.".into(),
            inputs, outputs: artifacts, events, effective_edits: effective,
            completeness: Completeness { ancestry_known: false, input_bytes_available: input.state != basecraft_protocol::input_records::EvidenceState::Broken, execution_specification_available: false, replay_verified: false }, report_template_version: 2,
        };
        match self.provenance_policy {
            basecraft_platform::settings::ProvenancePolicy::Portable => {
                crate::records::provenance::publish(run.directory(), &publication)?;
            }
            basecraft_platform::settings::ProvenancePolicy::LocalOnly => {
                let bundle = self.local_provenance_root.join(
                    basecraft_io::hash_bytes(target.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                );
                std::fs::create_dir_all(bundle.join("artifacts"))?;
                let mut local_input = input.clone();
                local_input.origin_run =
                    Some(crate::input_evidence::relative(&bundle, run.directory())?);
                crate::input_evidence::write_json(&bundle.join("input.json"), &local_input)?;
                if input.source.is_none() {
                    basecraft_io::write_atomic(&bundle.join("submitted.fasta"), |w| {
                        std::io::copy(
                            &mut std::fs::File::open(run.directory().join("submitted.fasta"))?,
                            w,
                        )?;
                        Ok(())
                    })?;
                }
                for artifact in &publication.inputs {
                    if let Some(name) = artifact
                        .retained
                        .as_deref()
                        .filter(|n| *n != "submitted.fasta")
                    {
                        basecraft_io::write_atomic(&bundle.join(name), |w| {
                            std::io::copy(
                                &mut std::fs::File::open(run.directory().join(name))?,
                                w,
                            )?;
                            Ok(())
                        })?;
                    }
                }
                crate::records::provenance::publish(&bundle, &publication)?;
            }
            basecraft_platform::settings::ProvenancePolicy::None => {}
        }
        if input.extra.contains_key("species_result")
            && !run.directory().join("manifest.json").is_file()
        {
            std::fs::create_dir_all(run.directory().join("records"))?;
            crate::input_evidence::write_json(
                &run.directory()
                    .join("records")
                    .join(format!("{}.json", publication.id)),
                &publication,
            )?;
            crate::input_evidence::write_json(
                &run.directory().join("manifest.json"),
                &publication,
            )?;
        }
        crate::input_evidence::write_json(
            &run.directory().join("run.json"),
            &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
                schema_version: 1,
                id: run.id().into(),
                result: Some(
                    target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| invalid("result filename is not UTF-8"))?
                        .into(),
                ),
                status: basecraft_protocol::records::AnalysisRunStatus::Completed,
                outputs: Some(
                    publication
                        .outputs
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                ),
                extra: std::collections::BTreeMap::new(),
            })?,
        )?;
        run.bind(target)
    }
}
