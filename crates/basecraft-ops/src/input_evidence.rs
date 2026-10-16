//! Referenced-input lifecycle. Only known stores are inspected, never filesystem trees.
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::provenance::{FileStamp, InputReference};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

use crate::{Result, analysis_store::ArtifactRun, error::invalid};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmittedInput {
    pub schema_version: u32,
    pub source: Option<InputReference>,
    pub submitted_hash: String,
    pub submitted_size: u64,
    /// canonical-fasta-v1 or ungapped-fasta-v1; generated inputs retain their bytes.
    pub transformation: String,
    pub state: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavePolicy {
    #[default]
    Cancel,
    PreserveCopy,
    RemoveAnalysis,
    BreakEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffectedAnalysis {
    pub run: PathBuf,
    pub result: String,
    pub source_hash: String,
    pub can_preserve: bool,
    pub direct: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceConflict {
    pub target: PathBuf,
    pub analyses: Vec<AffectedAnalysis>,
    pub options: Vec<SavePolicy>,
}

impl std::fmt::Display for EvidenceConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // CLI/SDK callers receive a structured conflict without an interactive prompt.
        write!(
            f,
            "analysis_input_conflict: {}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}

pub fn stamp(path: &Path) -> Result<FileStamp> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid("analysis input must be an ordinary file"));
    }
    let time = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid("input timestamp is before Unix epoch"))?;
    #[cfg(unix)]
    let (device, inode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.dev()), Some(metadata.ino()))
    };
    #[cfg(not(unix))]
    let (device, inode) = (None, None);
    Ok(FileStamp {
        size: metadata.len(),
        modified_seconds: time.as_secs(),
        modified_nanos: time.subsec_nanos(),
        device,
        inode,
    })
}

/// Lexical relative path between canonical absolute locations, including parents.
/// Referenced paths are read-only; managed artifact names use stricter validation.
pub fn relative(base: &Path, target: &Path) -> Result<String> {
    let a: Vec<_> = base.components().collect();
    let b: Vec<_> = target.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return target
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid("input path is not UTF-8"));
    }
    let mut path = PathBuf::new();
    for _ in common..a.len() {
        path.push("..");
    }
    for part in &b[common..] {
        path.push(part.as_os_str());
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("input path is not UTF-8"))
}

pub fn read_input(run: &Path) -> Result<SubmittedInput> {
    let metadata = std::fs::symlink_metadata(run.join("input.json"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("unsafe input evidence record"));
    }
    let record: SubmittedInput =
        serde_json::from_slice(&crate::provenance::read_record(&run.join("input.json"))?)?;
    if record.schema_version != 1
        || !matches!(
            record.state.as_str(),
            "referenced" | "generated" | "preserved" | "broken"
        )
    {
        return Err(invalid("unsupported input evidence record"));
    }
    if !matches!(
        record.transformation.as_str(),
        "canonical-fasta-v1" | "ungapped-fasta-v1" | "canonical-newick-v1" | "identity-v1"
    ) || matches!(record.state.as_str(), "referenced" | "preserved") && record.source.is_none()
        || record.state == "generated" && record.source.is_some()
    {
        return Err(invalid(
            "unsupported or inconsistent submitted input evidence",
        ));
    }
    Ok(record)
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    Ok(())
}

fn registry() -> Result<PathBuf> {
    let paths =
        basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
    Ok(paths.state.join("analysis-stores.json"))
}

/// Shared lock covers registration and the entire guarded replacement, including
/// copy/decision publication. It does not claim to lock external editors or sync.
pub fn lock() -> Result<std::fs::File> {
    let registry = registry()?;
    let parent = registry
        .parent()
        .ok_or_else(|| invalid("registry has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("analysis-evidence.lock"))?;
    lock.lock_exclusive()
        .map_err(|_| invalid("analysis evidence is busy; retry shortly"))?;
    Ok(lock)
}

fn store_registry() -> Result<serde_json::Value> {
    use std::io::Read as _;
    match std::fs::File::open(registry()?) {
        Ok(file) => {
            if file.metadata()?.len() > 1024 * 1024 {
                return Err(invalid("known-store registry exceeds 1 MiB"));
            }
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err(invalid("known-store registry exceeds 1 MiB"));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if value["schema_version"] != 1 || !value["stores"].is_array() {
                return Err(invalid("unsupported known-store registry"));
            }
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(serde_json::json!({"schema_version":1,"stores":[]}))
        }
        Err(e) => Err(e.into()),
    }
}

fn stores() -> Result<Vec<PathBuf>> {
    let paths: Vec<PathBuf> = serde_json::from_value(store_registry()?["stores"].clone())?;
    if paths.len() > 4096 || paths.iter().any(|p| !p.is_absolute()) {
        return Err(invalid("known-store paths must be absolute and bounded"));
    }
    Ok(paths)
}

/// Caller holds `lock`. Only explicit analysis execution registers a store.
pub fn register(run: &ArtifactRun) -> Result<()> {
    let store = run
        .directory()
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("run has no store"))?;
    let mut all = stores()?;
    if !all.iter().any(|p| p == store) {
        if all.len() >= 4096 {
            return Err(invalid("known-store registry limit reached"));
        }
        all.push(store.into());
        let mut value = store_registry()?;
        value["stores"] = serde_json::json!(all);
        write_json(&registry()?, &value)?;
    }
    Ok(())
}

fn same_source(run: &Path, reference: &InputReference, target: &Path) -> bool {
    let source = run.join(&reference.path);
    if source.canonicalize().ok().as_deref() == Some(target) {
        return true;
    }
    if let (Ok(a), Ok(b)) = (stamp(&source), stamp(target)) {
        return a.device.is_some() && a.device == b.device && a.inode == b.inode;
    }
    // Missing inputs still match their lexical old location.
    lexical(&source) == lexical(target)
}

fn lexical(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(part),
        }
    }
    result
}

/// Metadata-only UI query. Matching metadata is a signal, never a hash verdict.
pub fn conflicts(target: &Path) -> Result<EvidenceConflict> {
    let mut all = stores()?;
    let target = std::path::absolute(target)?;
    let canonical = target.canonicalize().unwrap_or_else(|_| target.clone());
    for parent in canonical.ancestors().skip(1) {
        let nearby = parent.join(".basecraft/analyses-v1");
        if nearby.exists() && !all.contains(&nearby) {
            all.push(nearby);
        }
    }
    conflicts_in(&canonical, &all)
}

pub fn conflicts_in(target: &Path, stores: &[PathBuf]) -> Result<EvidenceConflict> {
    let mut analyses = Vec::new();
    let mut known_runs = Vec::new();
    let mut metadata_bytes = 0u64;
    for store in stores {
        if !store.exists() {
            continue;
        } // Moved/unmounted store is not silently rewritten.
        ArtifactRun::validate_store(store)?;
        let runs = store.join("runs");
        let mut count = 0;
        for entry in std::fs::read_dir(&runs)? {
            count += 1;
            if count > 4096 {
                return Err(invalid(
                    "store exceeds 4096 runs; explicit inspection required",
                ));
            }
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                return Err(invalid("unsafe entry in analysis store"));
            }
            let run = entry.path();
            if !run.join("input.json").exists() {
                continue;
            } // Reserved, no input registered yet.
            metadata_bytes =
                metadata_bytes.saturating_add(std::fs::metadata(run.join("input.json"))?.len());
            if metadata_bytes > 64 << 20 {
                return Err(invalid(
                    "known analysis metadata exceeds 64 MiB query limit",
                ));
            }
            let input = read_input(&run)?;
            known_runs.push((run.clone(), input.clone()));
            if input.state != "referenced" {
                continue;
            }
            if let Some(reference) = input.source {
                if same_source(&run, &reference, target) {
                    let can_preserve = stamp(target).is_ok_and(|s| {
                        s.size == reference.stamp.size
                            && s.modified_seconds == reference.stamp.modified_seconds
                            && s.modified_nanos == reference.stamp.modified_nanos
                    });
                    analyses.push(AffectedAnalysis {
                        result: result_name(&run)?,
                        run,
                        source_hash: reference.hash,
                        can_preserve,
                        direct: true,
                    });
                }
            }
        }
    }
    // Metadata-only transitive closure: preserving the direct original repairs
    // descendants through their owning ledger, while removal lists every record
    // that would otherwise be left with a missing ancestor.
    loop {
        let before = analyses.len();
        for (run, input) in &known_runs {
            if analyses.iter().any(|a| &a.run == run) {
                continue;
            }
            let mut dependent = false;
            if let Some(parents) = input
                .extra
                .get("upstream")
                .and_then(serde_json::Value::as_array)
            {
                for parent in parents {
                    let path = parent["bundle"]
                        .as_str()
                        .ok_or_else(|| invalid("ancestor location is missing"))?;
                    dependent |= depends_on(&run.join(path), &analyses)?;
                }
            }
            if dependent {
                analyses.push(AffectedAnalysis {
                    run: run.clone(),
                    result: result_name(run)?,
                    source_hash: String::new(),
                    can_preserve: true,
                    direct: false,
                });
            }
        }
        if before == analyses.len() {
            break;
        }
    }
    let mut options = vec![
        SavePolicy::Cancel,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ];
    if !analyses.is_empty() && analyses.iter().all(|a| a.can_preserve) {
        options.insert(1, SavePolicy::PreserveCopy);
    }
    Ok(EvidenceConflict {
        target: target.into(),
        analyses,
        options,
    })
}

// Save As companions are not store runs. Include each delegation location
// before reading it, so a removed owner can still be matched by its old path.
fn depends_on(bundle: &Path, analyses: &[AffectedAnalysis]) -> Result<bool> {
    let mut location = lexical(bundle);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..32 {
        if analyses.iter().any(|a| lexical(&a.run) == location) {
            return Ok(true);
        }
        if !seen.insert(location.clone()) {
            return Err(invalid("input evidence contains a cycle"));
        }
        if !location.join("input.json").try_exists()? {
            return Ok(false);
        }
        let input = read_input(&location)?;
        let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(false);
        };
        location = lexical(&location.join(origin));
    }
    Err(invalid("input evidence ancestry exceeds 32 references"))
}

fn result_name(run: &Path) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_slice(&crate::provenance::read_record(&run.join("run.json"))?)?;
    Ok(value["result"]
        .as_str()
        .unwrap_or("Unpublished analysis")
        .into())
}

/// Apply explicit decisions before overwrite. A partial decision failure aborts
/// the save; earlier preserved copies/removed metadata remain recoverable.
pub fn resolve(conflict: &EvidenceConflict, policy: SavePolicy) -> Result<()> {
    if conflict.analyses.is_empty() {
        return Ok(());
    }
    if policy == SavePolicy::Cancel {
        return Err(crate::Error::EvidenceConflict(Box::new(conflict.clone())));
    }
    if !conflict.options.contains(&policy) {
        return Err(invalid(
            "old analysed bytes are unavailable; preserve-copy cannot repair this input",
        ));
    }
    for affected in &conflict.analyses {
        let mut input = read_input(&affected.run)?;
        match policy {
            SavePolicy::PreserveCopy => {
                if !affected.direct {
                    continue;
                }
                let reference = input
                    .source
                    .as_ref()
                    .ok_or_else(|| invalid("reference is missing"))?;
                if reference.hash != affected.source_hash {
                    return Err(invalid("stale input preservation decision"));
                }
                let destination = affected.run.join("preserved-input");
                // A public retry must never touch the live source or replace
                // retained bytes. The owning ledger may have advanced already.
                if input.state == "preserved" {
                    verify_preserved(&destination, reference)?;
                    continue;
                }
                if input.state != "referenced" {
                    return Err(invalid("input evidence changed; refresh the save decision"));
                }
                let source = affected.run.join(&reference.path);
                if !same_source(&affected.run, reference, &conflict.target) {
                    return Err(invalid("input reference moved; refresh the save decision"));
                }
                let copied = if destination.try_exists()? {
                    // A previous ledger write may have failed after publication.
                    verify_preserved(&destination, reference)?;
                    0
                } else {
                    let mut candidate = tempfile::NamedTempFile::new_in(&affected.run)?;
                    let mut hashing = crate::files::HashingWriter::new(candidate.as_file_mut());
                    let copied = std::io::copy(&mut std::fs::File::open(&source)?, &mut hashing)?;
                    if hashing.hash() != reference.hash || copied != reference.stamp.size {
                        return Err(invalid(
                            "source changed: copied bytes are not the analysed original; overwrite cancelled",
                        ));
                    }
                    candidate.as_file().sync_all()?;
                    candidate
                        .persist_noclobber(&destination)
                        .map_err(|e| e.error)?;
                    std::fs::File::open(&affected.run)?.sync_all()?;
                    copied
                };
                input.state = "preserved".into();
                input
                    .extra
                    .insert("preserved_original_bytes".into(), serde_json::json!(copied));
                write_json(&affected.run.join("input.json"), &input)?;
            }
            SavePolicy::BreakEvidence => {
                input.state = "broken".into();
                write_json(&affected.run.join("input.json"), &input)?;
            }
            SavePolicy::RemoveAnalysis => {
                let store = affected
                    .run
                    .parent()
                    .and_then(Path::parent)
                    .ok_or_else(|| invalid("run has no store"))?;
                let trash = store.join("trash");
                if !trash.exists() {
                    std::fs::create_dir(&trash)?;
                }
                if std::fs::symlink_metadata(&trash)?.file_type().is_symlink() {
                    return Err(invalid("unsafe analysis trash"));
                }
                let destination = trash.join(format!(
                    "{}-{}",
                    affected
                        .run
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    ulid::Ulid::generate()
                ));
                std::fs::rename(&affected.run, destination)?;
            }
            SavePolicy::Cancel => return Err(invalid("save cancelled")),
        }
    }
    Ok(())
}

fn verify_preserved(path: &Path, reference: &InputReference) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != reference.stamp.size
        || basecraft_io::hash_file(path)? != reference.hash
    {
        return Err(invalid(
            "retained original is damaged; existing bytes preserved",
        ));
    }
    Ok(())
}

/// Follow only the bounded Save As delegation, never a source file or cache.
fn owning_ledger(bundle: &Path) -> Result<(PathBuf, SubmittedInput)> {
    let mut location = bundle.canonicalize()?;
    let mut input = read_input(&location)?;
    let mut visited = std::collections::HashSet::from([location.clone()]);
    for _ in 0..32 {
        let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        else {
            break;
        };
        location = location.join(origin).canonicalize()?;
        if !visited.insert(location.clone()) {
            return Err(invalid("input evidence contains a cycle"));
        }
        input = read_input(&location)?;
    }
    if input.extra.contains_key("origin_run") {
        return Err(invalid("input evidence ancestry exceeds 32 references"));
    }
    Ok((location, input))
}

/// Resolve evidence only on explicit verification/export/relink. No UI-event hashing.
pub fn verified_source(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let (location, input) = owning_ledger(bundle)?;
    if input.state == "broken" {
        return Err(invalid("evidence chain explicitly broken"));
    }
    let effective = input
        .source
        .as_ref()
        .ok_or_else(|| invalid("reference evidence is missing"))?;
    if effective.hash != reference.hash {
        return Err(invalid("input reference identity changed"));
    }
    let path = if input.state == "preserved" {
        location.join("preserved-input")
    } else {
        location.join(&effective.path)
    };
    if basecraft_io::hash_file(&path)? != reference.hash
        || std::fs::metadata(&path)?.len() != reference.stamp.size
    {
        return Err(invalid(
            "referenced input changed; original evidence unavailable (timestamp alone is not identity)",
        ));
    }
    Ok(path)
}

/// Honest repair: only a byte-verified original may replace a missing/changed reference.
pub fn relink(bundle: &Path, original: &Path) -> Result<()> {
    let _lock = lock()?;
    let local = read_input(bundle)?;
    let (owner, mut input) = owning_ledger(bundle)?;
    if local.source.as_ref().map(|r| &r.hash) != input.source.as_ref().map(|r| &r.hash) {
        return Err(invalid("delegated input reference identity changed"));
    }
    let reference = input
        .source
        .as_mut()
        .ok_or_else(|| invalid("generated input has no disk reference"))?;
    let original = original.canonicalize()?;
    if basecraft_io::hash_file(&original)? != reference.hash
        || std::fs::metadata(&original)?.len() != reference.stamp.size
    {
        return Err(invalid("selected file is not the analysed original"));
    }
    reference.path = relative(&owner, &original)?;
    reference.stamp = stamp(&original)?;
    input.state = "referenced".into();
    write_json(&owner.join("input.json"), &input)
}

/// Explicit portable export materializes referenced dependencies; ordinary analysis does not.
pub fn package(result: &Path, destination: &Path) -> Result<()> {
    let issues = crate::provenance::verify(result)?;
    if !issues.is_empty() {
        return Err(invalid(issues.join("; ")));
    }
    if destination.exists() {
        return Err(invalid("package destination already exists"));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-package-")
        .tempdir_in(parent)?;
    let bundle = crate::provenance::bundle_for_result(result);
    let mut publication = crate::provenance::inspect(result)?;
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    let copied = staging.path().join(
        result
            .file_name()
            .ok_or_else(|| invalid("result has no filename"))?,
    );
    let target_bundle = crate::provenance::companion(&copied);
    std::fs::create_dir_all(target_bundle.join("artifacts"))?;
    // Rebase the whole immutable record graph, not only today's inputs.
    // Original record bytes are retained as checked artifacts; relocated records
    // get new identities and recomputed parent checksums in dependency order.
    let original_id = publication.id.clone();
    let upstream = ancestors(&bundle)?;
    let mut roots = vec![(bundle.clone(), publication.clone())];
    roots.extend(upstream);
    let species =
        crate::files::Format::from_path(result).ok() == Some(crate::files::Format::Delimitation);
    let mut species_paths = BTreeMap::new();
    if species {
        for (directory, record) in &roots {
            for artifact in record.inputs.iter().chain(&record.outputs) {
                let original = if let Some(reference) = &artifact.reference {
                    let (owner, input) = owning_ledger(directory)?;
                    let original = input.source.as_ref().map_or_else(
                        || directory.join(&reference.path),
                        |source| owner.join(&source.path),
                    );
                    Some(original)
                } else {
                    artifact.retained.as_ref().map(|name| directory.join(name))
                };
                if let Some(original) = original {
                    species_paths.insert(normalized_path(&original), artifact.hash.clone());
                }
            }
        }
    }
    let (mut exported, archived) = export_history(&roots, &target_bundle)?;
    publication = exported.remove(0);
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    for ancestor in exported {
        if !publication.parents.contains(&ancestor.id) {
            publication.parent_hashes.insert(
                ancestor.id.clone(),
                basecraft_io::hash_file(
                    &target_bundle
                        .join("records")
                        .join(format!("{}.json", ancestor.id)),
                )?,
            );
            publication.parents.push(ancestor.id);
        }
    }
    publication.inputs.extend(archived);
    for (index, artifact) in publication.outputs.iter_mut().enumerate() {
        let source = if index == 0 {
            result.to_path_buf()
        } else {
            result
                .parent()
                .unwrap_or(Path::new("."))
                .join(&artifact.logical_name)
        };
        let name = if index == 0 {
            copied
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else if source == crate::files::tree_sidecar(result) {
            crate::files::tree_sidecar(&copied)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            format!(
                "support-{}-{}",
                index,
                source.file_name().unwrap_or_default().to_string_lossy()
            )
        };
        copy(&source, &staging.path().join(&name))?;
        artifact.logical_name = name;
    }
    if species {
        let mut work = crate::files::load_delimitation(result)?;
        let final_folder = parent.canonicalize()?.join(
            destination
                .file_name()
                .ok_or_else(|| invalid("package destination has no name"))?,
        );
        let final_bundle = crate::provenance::companion(
            &final_folder.join(copied.file_name().unwrap_or_default()),
        );
        // Resolve declared paths to the checked, materialized dependency closure.
        // Immutable scientific reports stay byte-for-byte; only the derived
        // workspace metadata is relocated for use without its originals.
        crate::files::map_delimitation_paths(&mut work, |path| {
            species_paths
                .get(&normalized_path(Path::new(path)))
                .map_or_else(
                    || path.to_owned(),
                    |hash| {
                        final_bundle
                            .join("artifacts")
                            .join(hash.trim_start_matches("b3:"))
                            .display()
                            .to_string()
                    },
                )
        });
        work.exchange_fields
            .remove("basecraft_display_evidence_run");
        for hypothesis in &mut work.hypotheses {
            if let Some(source) = hypothesis
                .provenance
                .parameters
                .get_mut("combined_source")
                .and_then(serde_json::Value::as_object_mut)
            {
                source.remove("evidence_run");
            }
        }
        let sidecar = crate::files::tree_sidecar(&copied);
        basecraft_io::write_atomic(&sidecar, |w| {
            crate::files::write_delimitation_sidecar_in(
                &work,
                &publication.outputs[0].hash,
                &final_folder,
                w,
            )
        })?;
        let side_hash = basecraft_io::hash_file(&sidecar)?;
        if let Some(artifact) = publication
            .outputs
            .iter_mut()
            .find(|a| a.logical_name == sidecar.file_name().unwrap_or_default().to_string_lossy())
        {
            artifact.hash = side_hash;
            artifact.size = std::fs::metadata(&sidecar)?.len();
            artifact.retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:species-context", publication.id),
                logical_name: sidecar
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash: side_hash,
                size: std::fs::metadata(&sidecar)?.len(),
                retained: None,
                reference: None,
            });
        }
    }
    // Complete tree exports deliberately materialize the legacy exchange
    // association, so extraction still works with the original store absent.
    if let Ok(tree) = crate::files::load_tree(result) {
        if crate::links::metadata_present(&crate::links::sidecar(result))? {
            let link = crate::links::AlignmentLink::read(result)?;
            let input_path = link.checked_snapshot(result, &tree.doc)?;
            let indexed = std::sync::Arc::new(basecraft_io::cache::open_fasta(&input_path)?);
            let alignment = basecraft_docs::AlignmentDoc::from_fasta("Exported input", &indexed);
            let mut canonical = tempfile::Builder::new()
                .suffix(".afa")
                .tempfile_in(staging.path())?;
            crate::files::write_alignment(&alignment, canonical.as_file_mut())?;
            canonical.as_file().sync_all()?;
            let source_artifact = publication
                .inputs
                .first()
                .ok_or_else(|| invalid("tree export has no source"))?;
            let source_path = target_bundle.join(
                source_artifact
                    .retained
                    .as_ref()
                    .ok_or_else(|| invalid("exported source is not retained"))?,
            );
            let paths = crate::links::AlignmentLink::create(
                &copied,
                &source_path,
                canonical.path(),
                link.alignment_version,
                link.inferred,
                None,
            )?;
            for path in paths {
                publication
                    .outputs
                    .push(basecraft_protocol::provenance::ArtifactRevision {
                        id: format!(
                            "{}:package-link:{}",
                            publication.id,
                            publication.outputs.len()
                        ),
                        logical_name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        hash: basecraft_io::hash_file(&path)?,
                        size: std::fs::metadata(&path)?.len(),
                        retained: None,
                        reference: None,
                    });
            }
        }
    }
    publication.origin = format!(
        "Portable export of revision {}. {}",
        original_id, publication.origin
    );
    crate::provenance::publish(&target_bundle, &publication)?;
    let authored = bundle.join("reports/methods.md");
    if authored.is_file() {
        copy(&authored, &target_bundle.join("reports/methods.md"))?;
    }
    if !crate::provenance::verify(&copied)?.is_empty() {
        return Err(invalid(
            "export verification failed; original source and records preserved",
        ));
    }
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    // Reserve the destination atomically; rename replaces only our empty slot.
    std::fs::create_dir(destination)?;
    std::fs::rename(staging.path(), destination)?;
    Ok(())
}

fn normalized_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Copy checked bytes into the package's content-addressed artifact directory.
fn materialize(
    directory: &Path,
    target: &Path,
    artifact: &mut basecraft_protocol::provenance::ArtifactRevision,
) -> Result<()> {
    let source = if let Some(reference) = &artifact.reference {
        verified_source(directory, reference)?
    } else {
        directory.join(
            artifact
                .retained
                .as_ref()
                .ok_or_else(|| invalid("input bytes unavailable"))?,
        )
    };
    let name = format!("artifacts/{}", artifact.hash.trim_start_matches("b3:"));
    let destination = target.join(&name);
    if !destination.try_exists()? {
        copy(&source, &destination)?;
    }
    if std::fs::metadata(&destination)?.len() != artifact.size
        || basecraft_io::hash_file(&destination)? != artifact.hash
    {
        return Err(invalid("evidence changed during export"));
    }
    artifact.retained = Some(name);
    artifact.reference = None;
    Ok(())
}

type RecordKey = (PathBuf, String);
type Publication = basecraft_protocol::provenance::Publication;
type ArtifactRevision = basecraft_protocol::provenance::ArtifactRevision;

/// Bounded, iterative postorder traversal of each bundle's parent graph. Export
/// records are explicit relocations, while original immutable JSON stays intact
/// as a hash-checked input artifact. No source records are rewritten.
fn export_history(
    roots: &[(PathBuf, Publication)],
    target: &Path,
) -> Result<(Vec<Publication>, Vec<ArtifactRevision>)> {
    let mut stack = Vec::new();
    let mut root_keys = Vec::new();
    for (directory, publication) in roots {
        let key = (directory.canonicalize()?, publication.id.clone());
        stack.push((key.clone(), false));
        root_keys.push(key);
    }
    let mut states = std::collections::HashMap::new();
    let mut records = std::collections::HashMap::new();
    let mut order = Vec::new();
    let mut record_bytes = 0usize;
    let mut scheduled = stack.len();
    while let Some((key, leaving)) = stack.pop() {
        if leaving {
            states.insert(key.clone(), 2);
            order.push(key);
            continue;
        }
        match states.get(&key) {
            Some(1) => return Err(invalid("export history contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        if key.1.is_empty()
            || !key
                .1
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid("unsafe publication identity"));
        }
        let bytes =
            crate::provenance::read_record(&key.0.join("records").join(format!("{}.json", key.1)))?;
        record_bytes = record_bytes.saturating_add(bytes.len());
        if record_bytes > 128 << 20 || scheduled > 10_000 {
            return Err(invalid("export history exceeds bounded record limit"));
        }
        let record: Publication = serde_json::from_slice(&bytes)?;
        if record.id != key.1 || record.schema_version != 1 {
            return Err(invalid("inconsistent export history record"));
        }
        states.insert(key.clone(), 1);
        stack.push((key.clone(), true));
        for parent in &record.parents {
            scheduled += 1;
            stack.push(((key.0.clone(), parent.clone()), false));
        }
        records.insert(key, (record, bytes));
    }
    let mut rebased = std::collections::HashMap::<RecordKey, Publication>::new();
    let mut hashes = std::collections::HashMap::<RecordKey, String>::new();
    let mut archives = Vec::new();
    let mut mapping = Vec::new();
    std::fs::create_dir_all(target.join("records"))?;
    for key in order {
        let (mut publication, bytes) = records
            .remove(&key)
            .ok_or_else(|| invalid("export history is missing"))?;
        let hash = basecraft_io::hash_bytes(&bytes);
        let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
        basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
        let old_id = publication.id.clone();
        publication.id = ulid::Ulid::generate().to_string();
        mapping.push(serde_json::json!({"original_id":old_id,"original_hash":hash,"export_id":publication.id}));
        archives.push(ArtifactRevision {
            id: format!("{}:original-record", publication.id),
            logical_name: format!("original-record-{old_id}.json"),
            hash,
            size: bytes.len() as u64,
            retained: Some(name),
            reference: None,
        });
        let mut parents = Vec::new();
        let mut parent_hashes = BTreeMap::new();
        for parent in &publication.parents {
            let parent_key = (key.0.clone(), parent.clone());
            let rewritten = rebased
                .get(&parent_key)
                .ok_or_else(|| invalid("export parent missing"))?;
            parents.push(rewritten.id.clone());
            parent_hashes.insert(rewritten.id.clone(), hashes[&parent_key].clone());
        }
        publication.parents = parents;
        publication.parent_hashes = parent_hashes;
        for artifact in publication
            .inputs
            .iter_mut()
            .chain(&mut publication.outputs)
        {
            if artifact.retained.is_some() || artifact.reference.is_some() {
                materialize(&key.0, target, artifact)?;
            }
        }
        publication.origin = format!(
            "Relocated export record of {old_id}. {}",
            publication.origin
        );
        let path = target
            .join("records")
            .join(format!("{}.json", publication.id));
        write_json(&path, &publication)?;
        hashes.insert(key.clone(), basecraft_io::hash_file(&path)?);
        rebased.insert(key, publication);
    }
    let bytes =
        serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"records":mapping}))?;
    let hash = basecraft_io::hash_bytes(&bytes);
    let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
    basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
    archives.push(ArtifactRevision {
        id: format!("{}:export-map", ulid::Ulid::generate()),
        logical_name: "export-map.json".into(),
        hash,
        size: bytes.len() as u64,
        retained: Some(name),
        reference: None,
    });
    let exported = root_keys
        .iter()
        .map(|key| {
            rebased
                .get(key)
                .cloned()
                .ok_or_else(|| invalid("export root missing"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((exported, archives))
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    basecraft_io::write_atomic(destination, |w| {
        std::io::copy(&mut std::fs::File::open(source)?, w)?;
        Ok(())
    })?;
    Ok(())
}

/// Walk only declared immutable ancestor records. No ancestor input is copied.
/// The mutable evidence ledger stays in its owning run so later preservation or
/// explicit evidence-break decisions remain visible to descendants.
pub(crate) fn ancestors(
    bundle: &Path,
) -> Result<Vec<(PathBuf, basecraft_protocol::provenance::Publication)>> {
    let mut stack = vec![(bundle.to_path_buf(), false)];
    let mut states = std::collections::HashMap::new();
    let mut seen_records = std::collections::HashSet::new();
    let mut records = Vec::new();
    let mut bytes_read = 0usize;
    while let Some((location, leaving)) = stack.pop() {
        let location = location.canonicalize()?;
        if leaving {
            states.insert(location, 2);
            continue;
        }
        match states.get(&location) {
            Some(1) => return Err(invalid("evidence ancestry contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        states.insert(location.clone(), 1);
        stack.push((location.clone(), true));
        if !location.join("input.json").is_file() {
            continue;
        }
        let input = read_input(&location)?;
        if let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        {
            stack.push((location.join(origin), false));
        }
        if let Some(upstream) = input
            .extra
            .get("upstream")
            .and_then(serde_json::Value::as_array)
        {
            for reference in upstream {
                if records.len() + stack.len() >= 10_000 {
                    return Err(invalid("evidence ancestry exceeds 10,000 references"));
                }
                let directory = reference["bundle"]
                    .as_str()
                    .ok_or_else(|| invalid("ancestor location is missing"))?;
                let directory = location.join(directory).canonicalize()?;
                let id = reference["id"]
                    .as_str()
                    .ok_or_else(|| invalid("ancestor identity is missing"))?;
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err(invalid("unsafe ancestor identity"));
                }
                stack.push((directory.clone(), false));
                if !seen_records.insert((directory.clone(), id.to_owned())) {
                    continue;
                }
                let bytes = crate::provenance::read_record(
                    &directory.join("records").join(format!("{id}.json")),
                )?;
                bytes_read = bytes_read.saturating_add(bytes.len());
                if bytes_read > 128 << 20 {
                    return Err(invalid("evidence ancestry exceeds 128 MiB"));
                }
                if reference["hash"].as_str() != Some(basecraft_io::hash_bytes(&bytes).as_str()) {
                    return Err(invalid("ancestor record hash differs"));
                }
                let publication: basecraft_protocol::provenance::Publication =
                    serde_json::from_slice(&bytes)?;
                if publication.id != id || publication.schema_version != 1 {
                    return Err(invalid("unsupported or inconsistent ancestor record"));
                }
                records.push((directory, publication));
            }
        }
    }
    Ok(records)
}

pub(crate) fn verify_ancestors(bundle: &Path) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    for (directory, publication) in ancestors(bundle)? {
        issues.extend(crate::provenance::verify_records(&directory, &publication)?);
    }
    Ok(issues)
}
