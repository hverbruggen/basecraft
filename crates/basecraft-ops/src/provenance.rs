//! Inspectable portable save history. No bundle content is ever executed.
use crate::{Result, error::invalid};
use basecraft_protocol::provenance::Publication;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

pub fn companion(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".provenance");
    PathBuf::from(name)
}

/// Locate explicitly published project or standalone history. A missing portable
/// bundle is an error, never reconstructed from a filename or local journal.
pub fn bundle_for_result(result: &Path) -> PathBuf {
    if let Ok(Some(hidden)) = crate::analysis_store::ArtifactRun::for_result(result) {
        return hidden;
    }
    let standalone = companion(result);
    if standalone.join("manifest.json").is_file() {
        return standalone;
    }
    let absolute = result
        .canonicalize()
        .unwrap_or_else(|_| result.to_path_buf());
    for root in absolute.ancestors().skip(1) {
        if root.join(".basecraft/project.toml").is_file() {
            if let Ok(relative) = absolute.strip_prefix(root) {
                let key = relative.to_string_lossy();
                return root
                    .join(".basecraft/provenance")
                    .join(basecraft_io::hash_bytes(key.as_bytes()).trim_start_matches("b3:"));
            }
        }
    }
    if let Ok(paths) = basecraft_platform::paths::AppPaths::discover() {
        if let Ok(preferences) =
            basecraft_platform::settings::SettingsStore::new(paths.preferences()).load()
        {
            if preferences.files.provenance
                == basecraft_platform::settings::ProvenancePolicy::LocalOnly
            {
                let local = paths.state.join("provenance").join(
                    basecraft_io::hash_bytes(absolute.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                );
                if local.join("manifest.json").is_file() {
                    return local;
                }
            }
        }
    }
    standalone
}

fn safe_relative(root: &Path, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(invalid("provenance contains an unsafe relative path"));
    }
    let joined = root.join(p);
    // Reject symlink escape as well as lexical traversal.
    let canonical_root = root.canonicalize()?;
    let canonical = joined.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        return Err(invalid("provenance artifact escapes its bundle"));
    }
    Ok(canonical)
}

pub fn read_record(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    const LIMIT: u64 = 64 << 20;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > LIMIT {
        return Err(invalid("provenance record exceeds 64 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("provenance record exceeds 64 MiB"));
    }
    Ok(bytes)
}

fn copy_atomic(source: &Path, target: &Path) -> Result<()> {
    basecraft_io::write_atomic(target, |writer| {
        let mut input = std::fs::File::open(source)?;
        std::io::copy(&mut input, writer)?;
        Ok(())
    })?;
    Ok(())
}

pub fn inspect(result: &Path) -> Result<Publication> {
    // A damaged hidden binding is an error, never a reason to use stale legacy context.
    crate::analysis_store::ArtifactRun::for_result(result)?;
    let bundle = bundle_for_result(result);
    let publication: Publication =
        serde_json::from_slice(&read_record(&bundle.join("manifest.json"))?)?;
    if publication.schema_version != 1 {
        return Err(invalid("unsupported provenance schema version"));
    }
    Ok(publication)
}

/// Validate exact retained bytes and the current result; never launch software.
pub fn verify(result: &Path) -> Result<Vec<String>> {
    let publication = inspect(result)?;
    let bundle = bundle_for_result(result);
    let mut issues = verify_records(&bundle, &publication)?;
    for artifact in &publication.inputs {
        if let Some(relative) = &artifact.retained {
            match safe_relative(&bundle, relative).and_then(|path| {
                let size = std::fs::metadata(&path)?.len();
                let hash = basecraft_io::hash_file(&path)?;
                Ok(size == artifact.size && hash == artifact.hash)
            }) {
                Ok(true) => {}
                Ok(false) => issues.push(format!("{}: retained bytes differ", artifact.id)),
                Err(e) => issues.push(format!("{}: {e}", artifact.id)),
            }
        } else if let Some(reference) = &artifact.reference {
            if let Err(e) = crate::input_evidence::verified_source(&bundle, reference) {
                issues.push(format!("{}: {e}", artifact.id));
            }
        } else {
            issues.push(format!("{}: input bytes unavailable", artifact.id));
        }
    }
    if publication.outputs.is_empty() {
        return Err(invalid("publication has no output"));
    }
    for (index, output) in publication.outputs.iter().enumerate() {
        let path = if index == 0 {
            result.to_path_buf()
        } else {
            safe_relative(
                result.parent().unwrap_or(Path::new(".")),
                &output.logical_name,
            )?
        };
        match basecraft_io::hash_file(&path) {
            Ok(hash) if hash == output.hash => {}
            Ok(_) => issues.push(format!(
                "{} differs from its published revision",
                output.logical_name
            )),
            Err(error) => issues.push(format!("{} is unavailable: {error}", output.logical_name)),
        }
    }
    Ok(issues)
}

fn verify_records(bundle: &Path, current: &Publication) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    let mut states = std::collections::HashMap::<String, u8>::new();
    let mut hashes = std::collections::HashMap::<String, String>::new();
    // Queue only identities and expected checksums, never parsed publications.
    let mut stack = vec![(current.id.clone(), None, false)];
    let mut scheduled = 1usize;
    let mut record_bytes = 0u64;
    let mut artifact_bytes = 0u64;
    while let Some((id, expected_hash, leaving)) = stack.pop() {
        if leaving {
            states.insert(id, 2);
            continue;
        }
        if states.get(&id) == Some(&1) {
            return Err(invalid("provenance ancestry contains a cycle"));
        }
        if let Some(hash) = hashes.get(&id) {
            if expected_hash
                .as_ref()
                .is_some_and(|expected| expected != hash)
            {
                issues.push(format!("{id}: ancestor record hash differs"));
            }
            continue;
        }
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("unsafe publication identity"));
        }
        let path = safe_relative(bundle, &format!("records/{id}.json"))?;
        record_bytes = record_bytes.saturating_add(std::fs::metadata(&path)?.len());
        if record_bytes > 128 << 20 {
            return Err(invalid(
                "provenance ancestry exceeds 128 MiB record verification limit",
            ));
        }
        let bytes = read_record(&path)?;
        let hash = basecraft_io::hash_bytes(&bytes);
        if expected_hash
            .as_ref()
            .is_some_and(|expected| *expected != hash)
        {
            issues.push(format!("{id}: ancestor record hash differs"));
        }
        hashes.insert(id.clone(), hash);
        let publication: Publication = serde_json::from_slice(&bytes)?;
        if publication.id != id || publication.schema_version != 1 {
            return Err(invalid(
                "unsupported or inconsistent ancestor identity/version",
            ));
        }
        if id == current.id && serde_json::to_value(&publication)? != serde_json::to_value(current)?
        {
            issues.push(format!("{id}: immutable publication record differs"));
        }
        let events: std::collections::HashMap<_, _> =
            publication.events.iter().map(|e| (&e.id, &e.op)).collect();
        if events.len() != publication.events.len() {
            issues.push(format!("{id}: duplicate event identities"));
        }
        for activity in &publication.effective_edits {
            if events.get(&activity.id).copied() != Some(&activity.operation) {
                issues.push(format!(
                    "{}: effective activity has no matching event",
                    activity.id
                ));
            }
        }
        for artifact in publication.inputs.iter().chain(&publication.outputs) {
            if let Some(relative) = &artifact.retained {
                let retained = safe_relative(bundle, relative)?;
                let size = std::fs::metadata(&retained)?.len();
                artifact_bytes = artifact_bytes.saturating_add(size);
                if artifact_bytes > 8u64 << 30 {
                    return Err(invalid(
                        "ancestry exceeds 8 GiB artifact verification work limit",
                    ));
                }
                if size != artifact.size || basecraft_io::hash_file(&retained)? != artifact.hash {
                    issues.push(format!("{}: ancestor retained bytes differ", artifact.id));
                }
            } else if let Some(reference) = &artifact.reference {
                if let Err(e) = crate::input_evidence::verified_source(bundle, reference) {
                    issues.push(format!("{}: {e}", artifact.id));
                }
            } else if publication.inputs.iter().any(|a| a.id == artifact.id) {
                issues.push(format!("{}: ancestor input bytes unavailable", artifact.id));
            }
        }
        let mut parents = std::collections::HashSet::new();
        states.insert(id.clone(), 1);
        stack.push((id, None, true));
        for parent in &publication.parents {
            if !parents.insert(parent) {
                return Err(invalid("duplicate parent identity"));
            }
            scheduled += 1;
            if scheduled > 10_000 {
                return Err(invalid(
                    "provenance ancestry exceeds 10,000-reference verification limit",
                ));
            }
            let expected = publication
                .parent_hashes
                .get(parent)
                .cloned()
                .ok_or_else(|| invalid("ancestor record hash missing"))?;
            stack.push((parent.clone(), Some(expected), false));
        }
    }
    Ok(issues)
}

fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() > 64 << 20 {
        return Err(invalid(
            "provenance record exceeds 64 MiB publication limit",
        ));
    }
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    Ok(())
}

/// Artifacts must already be durable before this final manifest publication.
pub(crate) fn publish(bundle: &Path, publication: &Publication) -> Result<()> {
    std::fs::create_dir_all(bundle.join("records"))?;
    atomic_json(
        &bundle
            .join("records")
            .join(format!("{}.json", publication.id)),
        publication,
    )?;
    write_reports(bundle, publication)?;
    basecraft_io::write_atomic(&bundle.join("README.md"), |w| {
        w.write_all(
        b"# Basecraft provenance v1\n\nmanifest.json selects the latest publication. records/ holds immutable publication records; artifacts/ holds BLAKE3-addressed original and saved bytes. Reports are generated descriptions, not inferred laboratory protocols. Imported history before Basecraft is unknown. Checksums detect changes; they are not signatures. No file in this bundle should be executed to inspect or verify it.\n"
    )
    })?;
    atomic_json(&bundle.join("manifest.json"), publication)
}

/// Deterministic evidence-based reports. User-authored methods.md is untouched.
pub fn write_reports(output: &Path, publication: &Publication) -> Result<()> {
    std::fs::create_dir_all(output.join("reports"))?;
    let report = crate::document_record::DocumentRecord::from_publication(publication).report()?;
    let history = report.history_text;
    let methods = report.methods;
    let mut params = String::from("activity\toperation\tparameters_json\n");
    for activity in &publication.effective_edits {
        let _ = writeln!(
            params,
            "{}\t{}\t{}",
            activity.id,
            activity.operation.name(),
            serde_json::to_string(&activity.operation)?
        );
    }
    let mappings: Vec<_> = report.paragraphs.iter().enumerate().map(|(index, paragraph)|
        serde_json::json!({"paragraph": index + 1, "activities": paragraph.activity_ids})
    ).collect();
    for (name, bytes) in [
        ("history.md", history.as_bytes()),
        ("methods.generated.md", methods.as_bytes()),
        ("parameters.tsv", params.as_bytes()),
    ] {
        basecraft_io::write_atomic(&output.join("reports").join(name), |w| w.write_all(bytes))?;
    }
    atomic_json(&output.join("reports/source-map.json"), &mappings)?;
    atomic_json(
        &output.join("reports/manifest.json"),
        &serde_json::json!({
            "schema_version": 1, "publication": publication.id,
            "template_version": 2,
            "output_hashes": publication.outputs.iter().map(|a| &a.hash).collect::<Vec<_>>(),
            "redactions": ["machine-local paths", "host", "user"], "citations": report.citations
        }),
    )
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    if std::fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err(invalid("package bundle contains a symlink"));
    }
    std::fs::create_dir(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(invalid("package bundle contains a symlink"));
        }
        let target = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            copy_atomic(&entry.path(), &target)?;
        } else {
            return Err(invalid("package contains a special file"));
        }
    }
    Ok(())
}

/// Result and complete retained bundle copied into a new directory. This is a
/// data package, not a promise that external software can reproduce the result.
pub fn package(result: &Path, destination: &Path) -> Result<()> {
    if crate::analysis_store::ArtifactRun::for_result(result)?.is_some() {
        return crate::input_evidence::package(result, destination);
    }
    let issues = verify(result)?;
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
    let name = result
        .file_name()
        .ok_or_else(|| invalid("result has no filename"))?;
    let copied = staging.path().join(name);
    copy_atomic(result, &copied)?;
    let publication = inspect(result)?;
    for artifact in publication.outputs.iter().skip(1) {
        let name = Path::new(&artifact.logical_name);
        if name
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid("unsafe output logical name"));
        }
        copy_atomic(
            &result.parent().unwrap_or(Path::new(".")).join(name),
            &staging.path().join(name),
        )?;
    }
    copy_tree(&bundle_for_result(result), &companion(&copied))?;
    let copied_issues = verify(&copied)?;
    if !copied_issues.is_empty() {
        return Err(invalid(copied_issues.join("; ")));
    }
    // The destination is deliberately new: never merge unrelated package bytes.
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    std::fs::rename(staging.path(), destination)?;
    Ok(())
}
