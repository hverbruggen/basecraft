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

    /// Reserve a run for `output` before execution, in the `.basecraft` of
    /// `project` when the project holds the output, else beside it (rule
    /// 2.5). Existing IDs and foreign namespaces fail. No project marker is
    /// created. Failed/partial runs remain available here.
    pub fn create(
        project: Option<&crate::project::Project>,
        output: &Path,
        id: &str,
    ) -> Result<Self> {
        component(id)?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("invalid analysis run identity"));
        }
        let working_directory =
            crate::project::Project::base_of(project, &std::path::absolute(output)?)?
                .canonicalize()?;
        let store = Self::store_at(&working_directory)?;
        let path = store.join("runs").join(id);
        std::fs::create_dir(&path)?;
        std::fs::create_dir(path.join("pending"))?;
        Ok(Self {
            directory: path,
            working_directory,
            id: id.into(),
        })
    }

    /// The analysis store in the `.basecraft` of `base`, made if missing.
    fn store_at(base: &Path) -> Result<PathBuf> {
        let hidden = base.join(".basecraft");
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
        Ok(store)
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
}

impl ArtifactRun {
    /// The run `result` is bound to, if any. Its binding is looked for in
    /// one place only (rule 2.5): the `.basecraft` of `project` when the
    /// project holds the file, else the one beside the loose file.
    pub fn for_result(
        project: Option<&crate::project::Project>,
        result: &Path,
    ) -> Result<Option<PathBuf>> {
        Self::resolve_result(project, result, None)
    }

    /// The binding only when it describes result bytes with this hash; an
    /// error when it describes other bytes or its run is missing.
    pub fn for_result_with_hash(
        project: Option<&crate::project::Project>,
        result: &Path,
        hash: &str,
    ) -> Result<Option<PathBuf>> {
        Self::resolve_result(project, result, Some(hash))
    }

    fn resolve_result(
        project: Option<&crate::project::Project>,
        result: &Path,
        expected_hash: Option<&str>,
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
        let base = records_base(project, &absolute)?;
        let relative = absolute
            .strip_prefix(&base)
            .map_err(|_| invalid("result lies outside binding root"))?;
        let key = basecraft_io::hash_bytes(relative.as_os_str().as_encoded_bytes());
        let hidden = base.join(".basecraft");
        let store = hidden.join("analyses-v1");
        let binding = store.join("bindings").join(key.trim_start_matches("b3:"));
        match std::fs::symlink_metadata(&binding) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if store.exists() {
                    Self::validate_store(&store)?;
                }
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(invalid("unsafe result binding"));
            }
            Ok(_) => {}
        }
        if std::fs::symlink_metadata(&hidden)?.file_type().is_symlink() {
            return Err(invalid("hidden analysis directory is a symlink"));
        }
        Self::validate_store(&store)?;
        for directory in [store.join("bindings"), store.join("runs")] {
            let metadata = directory.symlink_metadata()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(invalid("unsafe analysis binding namespace"));
            }
        }
        let bound: basecraft_protocol::records::ResultBinding =
            basecraft_protocol::records::decode(&crate::records::provenance::read_record(
                &binding,
            )?)?;
        if bound.schema_version != 1 || Some(bound.result.as_str()) != relative.to_str() {
            return Err(invalid("unsupported or mismatched result binding"));
        }
        if expected_hash.is_some_and(|hash| bound.result_hash != hash) {
            return Err(invalid(
                "result contents differ from the hidden binding; metadata was not applied",
            ));
        }
        component(&bound.run)?;
        let store = match &bound.home {
            None => store,
            Some(home) => {
                if Path::new(home)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_) | Component::ParentDir))
                {
                    return Err(invalid("unsafe result binding home"));
                }
                let store = base
                    .join(home)
                    .canonicalize()?
                    .join(".basecraft/analyses-v1");
                Self::validate_store(&store)?;
                store
            }
        };
        let run = store.join("runs").join(&bound.run);
        if !run.exists() {
            return if expected_hash.is_some() {
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
        Ok(Some(run))
    }

    /// Refresh a previously discovered binding after a successful document save.
    /// This is a writer operation; it never certifies bytes on passive opening.
    pub(crate) fn refresh_binding(
        project: Option<&crate::project::Project>,
        result: &Path,
        hash: &str,
    ) -> Result<()> {
        if let Some(directory) = Self::for_result(project, result)? {
            Self::existing(directory)?.bind(project, result, hash)?;
        }
        Ok(())
    }

    /// Move the binding `result` got as a loose file to where `project`
    /// keeps its records, once a recovered folder holding it has become
    /// part of the project.
    pub(crate) fn settle_binding(project: &crate::project::Project, result: &Path) -> Result<()> {
        let Some(run) = Self::for_result(None, result)? else {
            return Ok(());
        };
        if !project.contains(result) {
            return Ok(());
        }
        let hash = basecraft_io::hash_file(result)?;
        Self::existing(run)?.bind(Some(project), result, &hash)?;
        let name = result
            .file_name()
            .ok_or_else(|| invalid("result filename missing"))?;
        let key = basecraft_io::hash_bytes(name.as_encoded_bytes());
        let folder = result.parent().unwrap_or(Path::new(".")).canonicalize()?;
        std::fs::remove_file(
            folder
                .join(".basecraft/analyses-v1/bindings")
                .join(key.trim_start_matches("b3:")),
        )?;
        Ok(())
    }

    /// The run in the folder `directory` of a store.
    pub(crate) fn existing(directory: PathBuf) -> Result<Self> {
        let working_directory = directory
            .ancestors()
            .nth(4)
            .ok_or_else(|| invalid("run has no working directory"))?
            .to_path_buf();
        let id = directory
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| invalid("run has no identity"))?
            .to_owned();
        Ok(Self {
            directory,
            working_directory,
            id,
        })
    }

    /// Bind `result`, whose hash the caller has, to this run, once its
    /// outputs and records are durable. The binding goes where the result's
    /// records are kept (rule 2.5): the `.basecraft` of `project` when the
    /// project holds it, else the one beside the loose result.
    pub fn bind(
        &self,
        project: Option<&crate::project::Project>,
        result: &Path,
        hash: &str,
    ) -> Result<()> {
        let parent = result
            .parent()
            .ok_or_else(|| invalid("result parent missing"))?
            .canonicalize()?;
        let absolute = parent.join(
            result
                .file_name()
                .ok_or_else(|| invalid("result filename missing"))?,
        );
        if !absolute.starts_with(&self.working_directory) {
            return Err(invalid(
                "binding result lies outside store working directory",
            ));
        }
        let base = records_base(project, &absolute)?;
        let name = absolute
            .strip_prefix(&base)
            .map_err(|_| invalid("binding result lies outside its records folder"))?;
        let (store, home) = if base == self.working_directory {
            let store = self.directory.parent().and_then(Path::parent);
            (
                store.ok_or_else(|| invalid("run has no store"))?.into(),
                None,
            )
        } else {
            let home = crate::records::input_evidence::relative(&base, &self.working_directory)?;
            (Self::store_at(&base)?, Some(home))
        };
        directory(&store.join("bindings"))?;
        let key = basecraft_io::hash_bytes(name.as_os_str().as_encoded_bytes());
        crate::records::input_evidence::write_json(
            &store.join("bindings").join(key.trim_start_matches("b3:")),
            // As a value, so its keys are written sorted.
            &serde_json::to_value(basecraft_protocol::records::ResultBinding {
                schema_version: 1,
                run: self.id.clone(),
                home,
                result: name
                    .to_str()
                    .ok_or_else(|| invalid("result filename is not UTF-8"))?
                    .into(),
                result_hash: hash.into(),
                extra: BTreeMap::new(),
            })?,
        )
    }
}

impl ArtifactRun {
    /// Promote a staged primary/component output without replacing existing data.
    /// Its basename is preserved; allocation remains anchored to the final folder.
    pub fn publish_primary(&self, name: &str) -> Result<PathBuf> {
        self.publish(name, ArtifactDestination::WorkingDirectory)
    }
}

/// The folder whose `.basecraft` keeps the binding of `file`: the project
/// root or the loose file's folder ([`crate::project::Project::base_of`]),
/// except that a file inside a `.basecraft`, such as a run's retained
/// result, belongs to the folder holding that `.basecraft`.
fn records_base(project: Option<&crate::project::Project>, file: &Path) -> Result<PathBuf> {
    match file
        .ancestors()
        .find(|folder| folder.file_name().is_some_and(|name| name == ".basecraft"))
        .and_then(Path::parent)
    {
        Some(folder) => Ok(folder.to_path_buf()),
        None => crate::project::Project::base_of(project, file),
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
    project: Option<&crate::project::Project>,
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
        crate::records::input_evidence::write_input(run.directory(), &input)?;
        run.write_run(
            basecraft_protocol::records::AnalysisRunStatus::Saved,
            target
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_owned),
        )?;
        placement.register(run)?;
        run.bind(project, target, main_hash)?;
    }
    if let Some(run) = crate::records::analysis_store::ArtifactRun::for_result(project, target)? {
        let mut input = crate::records::input_evidence::read_input(&run)?;
        let mut upstream = input.upstream.clone();
        for dependency in crate::delimitation::dependencies(project, work)? {
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
        crate::records::input_evidence::write_input(&run, &input)?;
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
