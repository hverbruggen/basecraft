//! Folder-local analysis artifacts. Opening a document never creates this store.
//!
//! An unmarked `.basecraft` may contain unrelated application data. We claim only
//! `analyses-v1`, and refuse an existing namespace without our exact marker.
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use basecraft_protocol::records::{StoreFormat, StoreMarker};

use crate::{Result, error::invalid};

#[cfg(test)]
mod publication_tests;

#[derive(Clone, Debug)]
pub struct ArtifactRun {
    directory: PathBuf,
    working_directory: PathBuf,
    id: String,
}

/// The default is hidden permanent metadata; working-directory output is opt-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArtifactDestination {
    #[default]
    Metadata,
    WorkingDirectory,
}

fn component(name: &str) -> Result<()> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(invalid("artifact name must be one ordinary path component"));
    }
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(invalid(format!(
            "unsafe artifact directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

impl ArtifactRun {
    /// Enumerate owned runs without creating storage. Exceeding the bound is an
    /// explicit incomplete query, never a silently truncated inventory.
    pub fn runs_for_anchor(anchor: &Path, limit: usize) -> Result<Vec<PathBuf>> {
        let parent = anchor
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let hidden = parent.join(".basecraft");
        match std::fs::symlink_metadata(&hidden) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(invalid("unsafe analysis metadata directory"));
            }
            Ok(_) => {}
        }
        let store = hidden.join("analyses-v1");
        if !store.try_exists()? {
            return Ok(Vec::new());
        }
        Self::validate_store(&store)?;
        let runs = store.join("runs");
        let metadata = std::fs::symlink_metadata(&runs)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("unsafe analysis runs directory"));
        }
        let mut found = Vec::new();
        for (index, entry) in std::fs::read_dir(runs)?.enumerate() {
            if index >= limit {
                return Err(invalid(
                    "analysis run lookup limit exceeded; inventory incomplete",
                ));
            }
            let entry = entry?;
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                return Err(invalid("symlink in owned analysis runs"));
            }
            if metadata.is_dir() {
                found.push(entry.path());
            }
        }
        found.sort();
        Ok(found)
    }

    /// Reserve a run before execution. Existing IDs and foreign namespaces fail.
    /// No project marker is created. Failed/partial runs remain available here.
    pub fn create(output: &Path, id: &str) -> Result<Self> {
        component(id)?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("invalid analysis run identity"));
        }
        let working_directory = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let hidden = working_directory.join(".basecraft");
        directory(&hidden)?;
        let store = hidden.join("analyses-v1");
        match std::fs::create_dir(&store) {
            Ok(()) => {
                let marker = StoreMarker {
                    format: StoreFormat::AnalysisStore,
                    schema_version: 1,
                    extra: BTreeMap::new(),
                };
                let bytes = basecraft_protocol::records::encode(&marker)?;
                basecraft_io::write_atomic(&store.join("store.json"), |w| w.write_all(&bytes))?;
                basecraft_io::write_atomic(&store.join("identity"), |w| {
                    w.write_all(ulid::Ulid::generate().to_string().as_bytes())
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::validate_store(&store)?;
            }
            Err(error) => return Err(error.into()),
        }
        directory(&store.join("runs"))?;
        let path = store.join("runs").join(id);
        std::fs::create_dir(&path)?;
        std::fs::create_dir(path.join("pending"))?;
        Ok(Self {
            directory: path,
            working_directory,
            id: id.into(),
        })
    }

    pub fn validate_store(store: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(store)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("analysis namespace is not an ordinary directory"));
        }
        let marker = store.join("store.json");
        let metadata = std::fs::symlink_metadata(&marker)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
            return Err(invalid("analysis namespace has an unsafe marker"));
        }
        let (value, writer) = basecraft_protocol::records::decode_written::<serde_json::Value>(
            &std::fs::read(marker)?,
        )?;
        if value["format"] != "basecraft.analysis-store" || value["schema_version"] != 1 {
            return Err(invalid(format!(
                "foreign or unsupported analysis namespace; nothing was migrated ({})",
                basecraft_protocol::records::other_format(&writer, &value["schema_version"], 1)
            )));
        }
        let _: StoreMarker = basecraft_protocol::records::convert(value, &writer)?;
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    /// The folder whose `.basecraft` holds this run.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn artifact(&self, name: &str) -> Result<PathBuf> {
        self.destination(ArtifactDestination::Metadata, name)
    }

    /// Write this run's `run.json` with its status and the result it is named by.
    pub fn write_run(
        &self,
        status: basecraft_protocol::records::AnalysisRunStatus,
        result: Option<String>,
    ) -> Result<()> {
        crate::records::input_evidence::write_json(
            &self.artifact("run.json")?,
            // Sorted keys, as every run.json has been written.
            &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
                schema_version: 1,
                id: self.id().into(),
                result,
                status,
                outputs: None,
                extra: std::collections::BTreeMap::new(),
            })?,
        )
    }

    pub fn destination(&self, location: ArtifactDestination, name: &str) -> Result<PathBuf> {
        component(name)?;
        Ok(match location {
            ArtifactDestination::Metadata => self.directory.join(name),
            ArtifactDestination::WorkingDirectory => self.working_directory.join(name),
        })
    }

    /// Stream here, close/sync successfully, then publish. Pending data is never
    /// advertised as a completed artifact and is retained on failure/cancel.
    pub fn staging(&self, name: &str) -> Result<PathBuf> {
        component(name)?;
        Ok(self.directory.join("pending").join(name))
    }

    /// Same-filesystem no-clobber publication. Unlike `rename`/`write_atomic` this
    /// cannot replace another run or user file. Cross-filesystem export fails.
    pub fn publish(&self, name: &str, location: ArtifactDestination) -> Result<PathBuf> {
        let source = self.staging(name)?;
        let metadata = std::fs::symlink_metadata(&source)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("staged artifact must be an ordinary file"));
        }
        basecraft_platform::files::sync_file(&source)?;
        let destination = self.destination(location, name)?;
        std::fs::hard_link(&source, &destination)?;
        basecraft_platform::files::sync_directory(
            destination
                .parent()
                .ok_or_else(|| invalid("artifact has no parent"))?,
        )?;
        // Keep the pending hardlink if cleanup fails; publication already succeeded.
        let _ = std::fs::remove_file(source);
        Ok(destination)
    }

    /// Explicit secondary export of an already retained artifact. Stage on the
    /// chosen filesystem and never replace a destination created by another writer.
    /// The caller owns destination preflight and records the declared export.
    pub fn publish_secondary(&self, name: &str, destination: &Path) -> Result<()> {
        let source = self.artifact(name)?;
        let metadata = std::fs::symlink_metadata(&source)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("secondary artifact must be an ordinary file"));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| invalid("secondary output has no parent"))?;
        let mut candidate = tempfile::NamedTempFile::new_in(parent)?;

        let mut hashing = crate::files::HashingWriter::new(candidate.as_file_mut());
        let size = std::io::copy(&mut std::fs::File::open(&source)?, &mut hashing)?;
        if size != metadata.len() {
            return Err(invalid("secondary artifact changed during export"));
        }
        if hashing.hash() != basecraft_io::hash_file(&source)? {
            return Err(invalid("secondary artifact changed during export"));
        }
        candidate.as_file().sync_all()?;
        candidate
            .persist_noclobber(destination)
            .map_err(|e| e.error)?;
        basecraft_platform::files::sync_directory(parent)?;

        Ok(())
    }
}

impl ArtifactRun {
    /// Bounded, read-only discovery of current hidden bindings beside a result.
    pub fn for_result(result: &Path) -> Result<Option<PathBuf>> {
        Self::resolve_result(result, false, None)
    }

    /// Read association only when the binding describes these exact result bytes.
    /// Save-time location discovery deliberately uses `for_result` instead.
    pub fn for_result_checked(result: &Path) -> Result<Option<PathBuf>> {
        Self::resolve_result(result, true, None)
    }

    /// Verify the same binding against bytes already owned by the caller.
    /// Namespace, binding and run validation is shared with disk verification.
    pub(crate) fn for_result_with_hash(result: &Path, hash: &str) -> Result<Option<PathBuf>> {
        Self::resolve_result(result, true, Some(hash))
    }

    fn resolve_result(
        result: &Path,
        checked: bool,
        snapshot_hash: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        let parent = result
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        // Species stage documents live inside an allocated run. Their explicit
        // declaration reuses the same discovery contract as visible bindings.
        if parent
            .parent()
            .is_some_and(|p| p.file_name().is_some_and(|n| n == "runs"))
            && let Some(store) = parent.parent().and_then(Path::parent)
            && store.file_name().is_some_and(|n| n == "analyses-v1")
        {
            Self::validate_store(store)?;
            let input = crate::records::input_evidence::read_input(&parent)?;
            if input
                .extra
                .get("species_result")
                .and_then(serde_json::Value::as_str)
                == result.file_name().and_then(|n| n.to_str())
            {
                return Ok(Some(parent));
            }
        }
        let absolute = parent.join(
            result
                .file_name()
                .ok_or_else(|| invalid("result filename missing"))?,
        );
        let mut base = parent.as_path();
        for depth in 0..64 {
            let relative = absolute
                .strip_prefix(base)
                .map_err(|_| invalid("result lies outside binding root"))?;
            let key = basecraft_io::hash_bytes(relative.as_os_str().as_encoded_bytes());
            let hidden = base.join(".basecraft");
            let store = hidden.join("analyses-v1");
            let binding = store.join("bindings").join(key.trim_start_matches("b3:"));
            let exists = std::fs::symlink_metadata(&binding).is_ok();
            if depth == 0 && store.exists() || exists {
                if std::fs::symlink_metadata(&hidden)?.file_type().is_symlink() {
                    return Err(invalid("hidden analysis directory is a symlink"));
                }
                Self::validate_store(&store)?;
            }
            if exists {
                for directory in [store.join("bindings"), store.join("runs")] {
                    let metadata = directory.symlink_metadata()?;
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(invalid("unsafe analysis binding namespace"));
                    }
                }
                let metadata = binding.symlink_metadata()?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(invalid("unsafe result binding"));
                }
                let value: serde_json::Value = basecraft_protocol::records::decode(
                    &crate::records::provenance::read_record(&binding)?,
                )?;
                if value["schema_version"] != 1 || value["result"].as_str() != relative.to_str() {
                    return Err(invalid("unsupported or mismatched result binding"));
                }
                if checked {
                    let hash = snapshot_hash.map_or_else(
                        || basecraft_io::hash_file(result),
                        |hash| Ok(hash.to_owned()),
                    )?;
                    if value["result_hash"].as_str() != Some(hash.as_str()) {
                        return Err(invalid(
                            "result contents differ from the hidden binding; metadata was not applied",
                        ));
                    }
                }
                let id = value["run"]
                    .as_str()
                    .ok_or_else(|| invalid("result binding has no run"))?;
                component(id)?;
                let run = store.join("runs").join(id);
                if !run.exists() {
                    return if checked {
                        Err(invalid(
                            "bound analysis run is missing; metadata was not applied",
                        ))
                    } else {
                        Ok(None)
                    };
                }
                let metadata = run.symlink_metadata()?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(invalid("unsafe analysis run"));
                }
                return Ok(Some(run));
            }
            let Some(ancestor) = base.parent() else {
                return Ok(None);
            };
            base = ancestor;
        }
        Err(invalid("result binding ancestor depth exceeds 64"))
    }

    /// Refresh a previously discovered binding after a successful document save.
    /// This is a writer operation; it never certifies bytes on passive opening.
    pub(crate) fn refresh_binding(result: &Path) -> Result<()> {
        if let Some(directory) = Self::for_result(result)? {
            let store = directory
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| invalid("run has no store"))?;
            let working_directory = store
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| invalid("store has no working directory"))?
                .to_path_buf();
            let id = directory
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| invalid("run has no identity"))?
                .to_owned();
            Self {
                directory,
                working_directory,
                id,
            }
            .bind(result)?;
        }
        Ok(())
    }

    /// Bind only after primary and supporting outputs plus manifest are durable.
    pub fn bind(&self, result: &Path) -> Result<()> {
        let store = self
            .directory
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("run has no store"))?;
        directory(&store.join("bindings"))?;
        let parent = result
            .parent()
            .ok_or_else(|| invalid("result parent missing"))?
            .canonicalize()?;
        let absolute = parent.join(
            result
                .file_name()
                .ok_or_else(|| invalid("result filename missing"))?,
        );
        let name = absolute
            .strip_prefix(&self.working_directory)
            .map_err(|_| invalid("binding result lies outside store working directory"))?;
        let key = basecraft_io::hash_bytes(name.as_os_str().as_encoded_bytes());
        crate::records::input_evidence::write_json(
            &store.join("bindings").join(key.trim_start_matches("b3:")),
            &serde_json::to_value(basecraft_protocol::records::ResultBinding {
                schema_version: 1,
                run: self.id.clone(),
                result: name
                    .to_str()
                    .ok_or_else(|| invalid("result filename is not UTF-8"))?
                    .into(),
                result_hash: basecraft_io::hash_file(result)?,
                extra: BTreeMap::new(),
            })?,
        )
    }
}

pub(crate) fn bind_existing(run: &Path, id: &str, result: &Path) -> Result<()> {
    let artifact_run = ArtifactRun {
        directory: run.into(),
        working_directory: result.parent().unwrap_or(Path::new(".")).canonicalize()?,
        id: id.into(),
    };
    artifact_run.bind(result)
}

impl ArtifactRun {
    /// Promote a staged primary/component output without replacing existing data.
    /// Its basename is preserved; allocation remains anchored to the final folder.
    pub fn publish_primary(&self, name: &str) -> Result<PathBuf> {
        self.publish(name, ArtifactDestination::WorkingDirectory)
    }
}

/// The name a run or bundle keeps a generated input under, in the document's
/// own format: SPART stays SPART, everything else is FASTA.
pub(crate) fn submitted_name(document: &Path) -> &'static str {
    if crate::files::Format::from_path(document).ok() != Some(crate::files::Format::Delimitation) {
        "submitted.fasta"
    } else if document
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("spart"))
    {
        "submitted.spart"
    } else {
        "submitted.spart.xml"
    }
}

/// The generated input a run or bundle keeps, whatever its format.
pub(crate) fn submitted_input(directory: &Path) -> Option<PathBuf> {
    ["submitted.fasta", "submitted.spart", "submitted.spart.xml"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}

pub(crate) fn retained_source(root: &Path, hash: &str) -> Result<Option<PathBuf>> {
    let store = root.join(".basecraft/analyses-v1");
    if !store.exists() {
        return Ok(None);
    }
    ArtifactRun::validate_store(&store)?;
    for (index, entry) in std::fs::read_dir(store.join("runs"))?.enumerate() {
        if index >= 4096 {
            return Err(invalid("replay analysis-store run limit exceeded"));
        }
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let run = entry.path();
        if !run.join("input.json").is_file() {
            continue;
        }
        let input = crate::records::input_evidence::read_input(&run)?;
        // A delegating ledger is also Preserved but holds no bytes; keep looking.
        let retained = if input.state == basecraft_protocol::input_records::EvidenceState::Preserved
            && input.source.as_ref().is_some_and(|r| r.hash == hash)
        {
            run.join("preserved-input")
        } else if input.state == basecraft_protocol::input_records::EvidenceState::Generated
            && input.submitted_hash == hash
        {
            match submitted_input(&run) {
                Some(path) => path,
                None => continue,
            }
        } else {
            continue;
        };
        if retained.is_file() {
            return Ok(Some(retained));
        }
    }
    Ok(None)
}

impl ArtifactRun {
    /// Validate members first, then atomically promote the owned collection.
    pub fn publish_collection(&self, name: &str) -> Result<PathBuf> {
        let source = self.staging(name)?;
        let metadata = source.symlink_metadata()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("staged collection is not an ordinary directory"));
        }
        let destination = self.artifact(name)?;
        basecraft_platform::atomic::rename_noreplace(&source, &destination)?;
        basecraft_platform::files::sync_directory(self.directory())?;
        Ok(destination)
    }
}

/// Publish the existing Delimitation analysis records after main and companion
/// writes. Session remains responsible for save authorization, recovery and
/// document provenance publication. Preserve ordering: retain input, register,
/// bind, then add upstream dependencies (including for an existing run).
pub(crate) fn publish_delimitation(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    target: &Path,
    main_hash: &str,
    new_run: Option<&ArtifactRun>,
    placement: &crate::documents::WritePublication,
) -> Result<()> {
    if let Some(run) = new_run {
        let input = crate::records::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: main_hash.to_owned(),
            submitted_size: std::fs::metadata(target)?.len(),
            transformation: "identity-v1".into(),
            state: "generated".into(),
            analysis_run: None,
            proposal_target: None,
            upstream: Vec::new(),
            origin_run: None,
            extra: std::collections::BTreeMap::new(),
        };
        basecraft_io::write_atomic(&run.artifact(submitted_name(target))?, |w| {
            std::io::copy(&mut std::fs::File::open(target)?, w)?;
            Ok(())
        })?;
        crate::records::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
        run.write_run(
            basecraft_protocol::records::AnalysisRunStatus::Saved,
            target
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_owned),
        )?;
        placement.register(run)?;
        run.bind(target)?;
    }
    if let Some(run) = crate::records::analysis_store::ArtifactRun::for_result(target)? {
        let mut input = crate::records::input_evidence::read_input(&run)?;
        let mut upstream = input.upstream.clone();
        for dependency in crate::delimitation::dependencies(work)? {
            let dependency = dependency.canonicalize()?;
            if dependency == run {
                continue;
            }
            let (publication, hash) = crate::records::provenance::latest_record(&dependency)?;
            let reference = basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::records::input_evidence::relative(
                    &placement.logical(&run)?,
                    &dependency,
                )?)
                .into(),
                id: publication.id,
                hash,
            };
            if !upstream.contains(&reference) {
                upstream.push(reference);
            }
        }
        if !upstream.is_empty() {
            input.upstream = upstream;
        }
        crate::records::input_evidence::write_json(&run.join("input.json"), &input)?;
    }
    Ok(())
}

impl ArtifactRun {
    pub(crate) fn relocated(&self, stage: &Path, destination: &Path) -> Result<Self> {
        let relocate = |path: &Path| -> Result<PathBuf> {
            Ok(destination.join(
                path.strip_prefix(stage)
                    .map_err(|_| invalid("recovery store outside stage"))?,
            ))
        };
        Ok(Self {
            directory: relocate(&self.directory)?,
            working_directory: relocate(&self.working_directory)?,
            id: self.id.clone(),
        })
    }
}
