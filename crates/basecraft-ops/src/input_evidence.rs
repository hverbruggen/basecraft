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

fn stores() -> Result<Vec<PathBuf>> {
    match std::fs::read(registry()?) {
        Ok(bytes) => {
            if bytes.len() > 1024 * 1024 {
                return Err(invalid("known-store registry exceeds 1 MiB"));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if value["schema_version"] != 1 {
                return Err(invalid("unsupported known-store registry"));
            }
            Ok(serde_json::from_value(value["stores"].clone())?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
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
        write_json(
            &registry()?,
            &serde_json::json!({"schema_version":1,"stores":all}),
        )?;
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
            if input
                .extra
                .get("upstream")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|parents| {
                    parents.iter().any(|parent| {
                        parent["bundle"].as_str().is_some_and(|path| {
                            let location = lexical(&run.join(path));
                            analyses.iter().any(|a| lexical(&a.run) == location)
                        })
                    })
                })
            {
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
                let source = affected.run.join(&reference.path);
                let destination = affected.run.join("preserved-input");
                let mut hash = String::new();
                let mut copied = 0;
                basecraft_io::write_atomic(&destination, |writer| {
                    let mut hashing = crate::files::HashingWriter::new(writer);
                    copied = std::io::copy(&mut std::fs::File::open(&source)?, &mut hashing)?;
                    hash = hashing.hash();
                    Ok(())
                })?;
                if hash != reference.hash
                    || std::fs::metadata(&destination)?.len() != reference.stamp.size
                {
                    return Err(invalid(
                        "source changed: copied bytes are not the analysed original; overwrite cancelled",
                    ));
                }
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

/// Resolve evidence only on explicit verification/export/relink. No UI-event hashing.
pub fn verified_source(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let mut location = bundle.to_path_buf();
    let mut input = read_input(&location)?;
    let mut visited = std::collections::HashSet::new();
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
    let mut input = read_input(bundle)?;
    let reference = input
        .source
        .as_mut()
        .ok_or_else(|| invalid("generated input has no disk reference"))?;
    let original = original.canonicalize()?;
    if basecraft_io::hash_file(&original)? != reference.hash {
        return Err(invalid("selected file is not the analysed original"));
    }
    reference.path = relative(bundle, &original)?;
    reference.stamp = stamp(&original)?;
    input.state = "referenced".into();
    write_json(&bundle.join("input.json"), &input)
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
    let copied = staging.path().join(
        result
            .file_name()
            .ok_or_else(|| invalid("result has no filename"))?,
    );
    let target_bundle = crate::provenance::companion(&copied);
    std::fs::create_dir_all(target_bundle.join("artifacts"))?;
    let mut evidence_inputs: Vec<_> = publication
        .inputs
        .iter()
        .cloned()
        .map(|a| (bundle.clone(), a))
        .collect();
    for (directory, ancestor) in ancestors(&bundle)? {
        for artifact in ancestor.inputs {
            if !evidence_inputs.iter().any(|(_, a)| a.id == artifact.id) {
                evidence_inputs.push((directory.clone(), artifact));
            }
        }
    }
    publication.inputs.clear();
    for (directory, mut artifact) in evidence_inputs {
        let source = if let Some(reference) = &artifact.reference {
            verified_source(&directory, reference)?
        } else {
            directory.join(
                artifact
                    .retained
                    .as_ref()
                    .ok_or_else(|| invalid("ancestor input bytes unavailable"))?,
            )
        };
        let name = format!("artifacts/{}", artifact.hash.trim_start_matches("b3:"));
        copy(&source, &target_bundle.join(&name))?;
        artifact.retained = Some(name);
        artifact.reference = None;
        publication.inputs.push(artifact);
    }
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
    crate::provenance::publish(&target_bundle, &publication)?;
    let authored = bundle.join("reports/methods.md");
    if authored.is_file() {
        copy(&authored, &target_bundle.join("reports/methods.md"))?;
    }
    if !crate::provenance::verify(&copied)?.is_empty() {
        return Err(invalid(
            "export verification failed; source and staging preserved",
        ));
    }
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    std::fs::rename(staging.path(), destination)?;
    Ok(())
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
    let mut stack = vec![bundle.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    let mut records = Vec::new();
    let mut bytes_read = 0usize;
    while let Some(location) = stack.pop() {
        if !location.join("input.json").is_file() {
            continue;
        }
        let input = read_input(&location)?;
        if let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        {
            let origin = location.join(origin).canonicalize()?;
            if !visited.insert((origin.clone(), String::new())) {
                return Err(invalid("evidence ancestry contains a cycle"));
            }
            stack.push(origin);
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
                if !visited.insert((directory.clone(), id.into())) {
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
                records.push((directory.clone(), publication));
                stack.push(directory);
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
