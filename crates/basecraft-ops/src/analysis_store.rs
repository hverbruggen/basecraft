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
                // Preserve the original compact, sorted JSON and final newline.
                let mut bytes = serde_json::to_vec(&serde_json::to_value(marker)?)?;
                bytes.push(b'\n');
                basecraft_io::write_atomic(&store.join("store.json"), |w| w.write_all(&bytes))?;
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
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker)?)?;
        if value["format"] != "basecraft.analysis-store" || value["schema_version"] != 1 {
            return Err(invalid(
                "foreign or unsupported analysis namespace; nothing was migrated",
            ));
        }
        let _: StoreMarker = serde_json::from_value(value)?;
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn artifact(&self, name: &str) -> Result<PathBuf> {
        self.destination(ArtifactDestination::Metadata, name)
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
        if size != metadata.len() || hashing.hash() != basecraft_io::hash_file(&source)? {
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
    /// Bounded, read-only discovery beside a result. Hidden bindings take
    /// precedence; legacy companions remain readable when no binding exists.
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
            let input = crate::input_evidence::read_input(&parent)?;
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
                let value: serde_json::Value =
                    serde_json::from_slice(&crate::provenance::read_record(&binding)?)?;
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
        crate::input_evidence::write_json(
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
        let input = crate::input_evidence::read_input(&run)?;
        if input.state == "preserved" && input.source.as_ref().is_some_and(|r| r.hash == hash) {
            return Ok(Some(run.join("preserved-input")));
        }
        if input.state == "generated" && input.submitted_hash == hash {
            return Ok(Some(run.join("submitted.fasta")));
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
) -> Result<()> {
    if let Some(run) = new_run {
        let input = crate::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: main_hash.to_owned(),
            submitted_size: std::fs::metadata(target)?.len(),
            transformation: "identity-v1".into(),
            state: "generated".into(),
            extra: std::collections::BTreeMap::new(),
        };
        basecraft_io::write_atomic(&run.artifact("submitted.fasta")?, |w| {
            std::io::copy(&mut std::fs::File::open(target)?, w)?;
            Ok(())
        })?;
        crate::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
        crate::input_evidence::write_json(
            &run.artifact("run.json")?,
            &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
                schema_version: 1,
                id: run.id().into(),
                result: target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_owned),
                status: basecraft_protocol::records::AnalysisRunStatus::Saved,
                outputs: None,
                extra: std::collections::BTreeMap::new(),
            })?,
        )?;
        crate::input_evidence::register(run)?;
        run.bind(target)?;
    }
    if let Some(run) = crate::analysis_store::ArtifactRun::for_result(target)? {
        let mut input = crate::input_evidence::read_input(&run)?;
        let mut upstream = input
            .extra
            .get("upstream")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for dependency in crate::delimitation::dependencies(work)? {
            let dependency = dependency.canonicalize()?;
            if dependency == run {
                continue;
            }
            let bytes = crate::provenance::read_record(&dependency.join("manifest.json"))?;
            let publication: basecraft_protocol::provenance::Publication =
                serde_json::from_slice(&bytes)?;
            let reference = serde_json::json!({"bundle":crate::input_evidence::relative(&run, &dependency)?,"id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)});
            if !upstream.contains(&reference) {
                upstream.push(reference);
            }
        }
        if !upstream.is_empty() {
            input
                .extra
                .insert("upstream".into(), serde_json::json!(upstream));
        }
        crate::input_evidence::write_json(&run.join("input.json"), &input)?;
    }
    Ok(())
}
