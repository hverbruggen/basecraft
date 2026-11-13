//! Inspectable portable save history. No bundle content is ever executed.
use crate::{Result, error::invalid};
use basecraft_protocol::{
    Actor, DocState, LogEntry, OpResult,
    provenance::{ArtifactRevision, Completeness, Publication},
};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

pub fn companion(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".provenance");
    PathBuf::from(name)
}

/// Select the existing binding or companion path.
/// The final fallback is the original companion spelling, even if it does not
/// exist. Selection does not validate content or establish complete provenance.
pub fn bundle_for_result(result: &Path, project: Option<&crate::project::Project>) -> PathBuf {
    if let Ok(Some(hidden)) = crate::records::analysis_store::ArtifactRun::for_result(result)
        && hidden.join("manifest.json").is_file()
    {
        return hidden;
    }
    let standalone = companion(result);
    if standalone.join("manifest.json").is_file() {
        return standalone;
    }
    let absolute = result
        .canonicalize()
        .unwrap_or_else(|_| result.to_path_buf());
    // The project's bundle counts only when it holds history.
    if let Some(bundle) = project.and_then(|project| project.provenance_bundle(&absolute))
        && bundle.join("manifest.json").is_file()
    {
        return bundle;
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
    read_record_with_budget(path, &mut (64 << 20))
}

/// Share the per-record cap with callers enforcing an aggregate read budget.
pub(crate) fn read_record_with_budget(path: &Path, remaining: &mut u64) -> Result<Vec<u8>> {
    let file = basecraft_platform::files::open_regular_read(path)?;
    let length = file.metadata()?.len();
    read_record_from_open(file, length, remaining, 64 << 20, RecordLimit::Provenance)
}

#[derive(Clone, Copy)]
pub(crate) enum RecordLimit {
    Provenance,
}
impl RecordLimit {
    fn label(self) -> &'static str {
        match self {
            Self::Provenance => "provenance record reading",
        }
    }
}

pub(crate) fn read_record_from_open(
    file: impl std::io::Read,
    length: u64,
    remaining: &mut u64,
    cap: u64,
    label: RecordLimit,
) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let limit = (*remaining).min(64 << 20).min(cap);
    if length > limit {
        return Err(crate::Error::Limit {
            what: label.label().into(),
            limit,
        });
    }
    let mut bytes = Vec::new();
    let result = file.take(limit + 1).read_to_end(&mut bytes);
    *remaining = remaining.saturating_sub(bytes.len() as u64);
    result?;
    if bytes.len() as u64 > limit {
        return Err(crate::Error::Limit {
            what: label.label().into(),
            limit,
        });
    }
    Ok(bytes)
}

/// A file a run keeps, as its publication lists it: `<run id>:<label>`, the
/// name shown, its bytes and where in the run folder they are.
pub fn retained_file(
    run: &crate::records::ArtifactRun,
    label: &str,
    logical_name: String,
    path: &Path,
    hash: String,
) -> Result<ArtifactRevision> {
    Ok(ArtifactRevision {
        id: format!("{}:{label}", run.id()),
        logical_name,
        hash,
        size: std::fs::metadata(path)?.len(),
        retained: Some(crate::records::relative(run.directory(), path)?),
        reference: None,
    })
}

/// Copy a run's file to a new file in the project, never replacing one. The
/// receipt names the new file's inode before the rename, so a later repair
/// adopts only a file this run created.
pub fn export_file(source: &Path, destination: &Path, hash: &str, receipt: &Path) -> Result<()> {
    use basecraft_platform::files::identity;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("missing destination parent"))?;
    let pending = tempfile::Builder::new()
        .prefix(".basecraft-export-")
        .tempfile_in(parent)?;
    std::fs::copy(source, pending.path())?;
    if basecraft_io::hash_file(pending.path())? != hash {
        return Err(invalid("member changed during export"));
    }
    pending.as_file().sync_all()?;
    let (device, inode) = identity(pending.path())?;
    crate::records::write_json(
        receipt,
        &serde_json::to_value(
            basecraft_protocol::publication_records::MemberExportReceipt {
                schema: 1,
                destination: destination.to_path_buf(),
                hash: hash.into(),
                device,
                inode,
                size: pending.as_file().metadata()?.len(),
            },
        )?,
    )?;
    basecraft_platform::atomic::rename_noreplace(pending.path(), destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            crate::access::AccessError::DestinationExists.into()
        } else {
            crate::Error::from(error)
        }
    })?;
    basecraft_platform::files::sync_directory(parent)?;
    Ok(())
}

/// Whether `destination` is still the file [`export_file`] created, with the
/// same bytes. Matching bytes alone never authorize adopting another file.
pub fn verify_export(destination: &Path, hash: &str, receipt: &Path) -> Result<()> {
    use basecraft_platform::files::{identity, link_count};
    let receipt: basecraft_protocol::publication_records::MemberExportReceipt =
        basecraft_protocol::records::decode(&read_record(receipt)?)?;
    let metadata = std::fs::symlink_metadata(destination)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || link_count(destination)? != 1
        || receipt.schema != 1
        || receipt.destination != destination
        || receipt.hash != hash
        || (receipt.device, receipt.inode) != identity(destination)?
        || receipt.size != metadata.len()
        || basecraft_io::hash_file(destination)? != hash
    {
        return Err(crate::Error::Damaged {
            path: destination.to_path_buf(),
            detail: "output_conflict: member ownership or bytes changed; repair refused".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "provenance/record_access_tests.rs"]
mod record_access_tests;

fn copy_atomic(source: &Path, target: &Path) -> Result<()> {
    basecraft_io::write_atomic(target, |writer| {
        let mut input = std::fs::File::open(source)?;
        std::io::copy(&mut input, writer)?;
        Ok(())
    })?;
    Ok(())
}

pub fn inspect(result: &Path, project: Option<&crate::project::Project>) -> Result<Publication> {
    // A damaged hidden binding is an error, never a reason to use stale legacy context.
    crate::records::analysis_store::ArtifactRun::for_result(result)?;
    let bundle = bundle_for_result(result, project);
    let manifest = bundle.join("manifest.json");
    if project.is_none() && !manifest.is_file() {
        // Never guess a project; say where else the history could be.
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No history beside this file. If it belongs to a project, choose that project \
             (CLI and MCP: --project).",
        )
        .into());
    }
    let publication: Publication = basecraft_protocol::records::decode(&read_record(&manifest)?)?;
    if publication.schema_version != 1 {
        return Err(crate::Error::Unsupported(
            "unsupported provenance schema version".into(),
        ));
    }
    Ok(publication)
}

/// Validate exact retained bytes and the current result; never launch software.
pub fn verify(result: &Path, project: Option<&crate::project::Project>) -> Result<Vec<String>> {
    let publication = inspect(result, project)?;
    let bundle = bundle_for_result(result, project);
    let mut issues = verify_records(&bundle, &publication)?;
    if bundle.join("input.json").is_file() {
        if crate::records::input_evidence::read_input(&bundle)?.state
            == basecraft_protocol::input_records::EvidenceState::Broken
        {
            issues.push("Input evidence explicitly broken".into());
        }
        match crate::records::input_evidence::verify_ancestors(&bundle) {
            Ok(upstream) => issues.extend(upstream),
            Err(e) => issues.push(format!("Upstream evidence unavailable: {e}")),
        }
    }
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
            if artifact.hash != reference.hash || artifact.size != reference.stamp.size {
                issues.push(format!(
                    "{}: input revision/reference identity differs",
                    artifact.id
                ));
            }
            if let Err(e) = crate::records::input_evidence::verified_source(&bundle, reference) {
                issues.push(format!("{}: {e}", artifact.id));
            }
        }
    }
    if publication.outputs.is_empty() {
        return Err(invalid("publication has no output"));
    }
    let hidden = crate::records::analysis_store::ArtifactRun::for_result(result)?.is_some();
    for (index, output) in publication.outputs.iter().enumerate() {
        let path = if index == 0 && !hidden {
            result.to_path_buf()
        } else {
            let base = if output.retained.as_deref() == Some(output.logical_name.as_str()) {
                bundle.as_path()
            } else if hidden {
                bundle
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .ok_or_else(|| invalid("invalid hidden publication path"))?
            } else {
                result.parent().unwrap_or(Path::new("."))
            };
            safe_relative(base, &output.logical_name)?
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

pub(crate) fn verify_records(bundle: &Path, current: &Publication) -> Result<Vec<String>> {
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
            return Err(crate::Error::Limit {
                what: "provenance ancestry record verification".into(),
                limit: 128 << 20,
            });
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
        let publication: Publication = basecraft_protocol::records::decode(&bytes)?;
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
                    return Err(crate::Error::Limit {
                        what: "ancestry artifact verification".into(),
                        limit: 8 << 30,
                    });
                }
                if size != artifact.size || basecraft_io::hash_file(&retained)? != artifact.hash {
                    issues.push(format!("{}: ancestor retained bytes differ", artifact.id));
                }
            } else if let Some(reference) = &artifact.reference {
                if artifact.hash != reference.hash || artifact.size != reference.stamp.size {
                    issues.push(format!(
                        "{}: ancestor input revision/reference identity differs",
                        artifact.id
                    ));
                }
                if let Err(e) = crate::records::input_evidence::verified_source(bundle, reference) {
                    issues.push(format!("{}: {e}", artifact.id));
                }
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
                return Err(crate::Error::Limit {
                    what: "provenance ancestry references".into(),
                    limit: 10_000,
                });
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

fn stage_json(
    prepared: &mut crate::documents::PreparedWrite,
    path: &Path,
    value: &impl serde::Serialize,
) -> Result<()> {
    let bytes = basecraft_protocol::records::encode(value)?;
    if bytes.len() > 64 << 20 {
        return Err(crate::Error::Limit {
            what: "provenance record publication".into(),
            limit: 64 << 20,
        });
    }
    prepared.push(path, |w| w.write_all(&bytes))?;
    Ok(())
}

// Carry explicitly selected input runs into ordinary saved ancestry: a Sanger
// review chosen for an alignment (`sanger_input`) or any other `ToolRun` that
// names a registered reference ledger (`input_run`), such as imported support
// values. The event remains a ToolRun; edits, undo and replay are unchanged.
// Processed events stay listed under the original `sanger_input_events` key.
fn link_selected_inputs(bundle: &Path, publication: &Publication) -> Result<()> {
    let mut effective = publication.effective_edits.clone();
    crate::records::contributions::project(&publication.events, &mut effective);
    let retained: std::collections::HashSet<_> = effective.iter().map(|e| e.id.as_str()).collect();
    // Each selected input names the run that retains its exact bytes.
    let mut dependencies: Vec<(&String, basecraft_protocol::records::ActivityInputRun)> =
        Vec::new();
    for event in &publication.events {
        let basecraft_protocol::Op::ToolRun {
            input_run,
            contributions,
            ..
        } = &event.op
        else {
            continue;
        };
        if let Some(run) = input_run
            && (contributions.is_none() || retained.contains(event.id.as_str()))
        {
            dependencies.push((&event.id, run.clone()));
        }
    }
    let path = bundle.join("input.json");
    if dependencies.is_empty() && !path.is_file() {
        return Ok(());
    }
    let mut input = if path.is_file() {
        crate::records::input_evidence::read_input(bundle)?
    } else {
        let output = publication
            .outputs
            .first()
            .ok_or_else(|| invalid("input publication has no result"))?;
        crate::records::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: output.hash.clone(),
            submitted_size: output.size,
            transformation: "identity-v1".into(),
            state: "generated".into(),
            analysis_run: None,
            proposal_target: None,
            upstream: Vec::new(),
            origin_run: None,
            extra: std::collections::BTreeMap::new(),
        }
    };
    let mut processed = input
        .extra
        .get("sanger_input_events")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if publication
        .origin
        .starts_with("Portable export of revision ")
    {
        // The package already materialized and verified the entire ancestor
        // graph. Remember its historical selections without reconnecting an
        // export (or its later Save As) to the original live ledgers.
        for (event, _) in dependencies {
            if !processed.iter().any(|id| id.as_str() == Some(event)) {
                processed.push(serde_json::json!(event));
            }
        }
        input
            .extra
            .insert("sanger_input_events".into(), serde_json::json!(processed));
        return crate::records::input_evidence::write_json(&path, &input);
    }
    let mut upstream = input.upstream.clone();
    // Keep ownership of selected edges separate from unrelated scientific inputs.
    // Historical publications and their records are left intact.
    let mut links: std::collections::BTreeMap<
        String,
        basecraft_protocol::input_records::UpstreamLink,
    > = input
        .extra
        .get("selected_input_links")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    for reference in links.values_mut() {
        if let Some(current) = upstream
            .iter()
            .find(|current| current.id == reference.id && current.hash == reference.hash)
        {
            *reference = current.clone();
        }
    }
    let selected: std::collections::HashSet<_> = dependencies
        .iter()
        .map(|(event, _)| event.as_str())
        .collect();
    let expired: Vec<_> = links
        .iter()
        .filter(|(event, _)| !selected.contains(event.as_str()))
        .map(|(event, reference)| (event.clone(), reference.clone()))
        .collect();
    for (event, reference) in expired {
        links.remove(&event);
        if !links.values().any(|other| other == &reference) {
            upstream.retain(|other| other != &reference);
        }
        processed.retain(|id| id.as_str() != Some(event.as_str()));
    }
    let inherited = crate::records::input_evidence::ancestors(bundle)?;
    for (event, dependency) in dependencies {
        if processed.iter().any(|id| id.as_str() == Some(event)) {
            continue;
        }
        if dependency.schema_version != 1 {
            return Err(crate::Error::Unsupported(
                "Unsupported selected input dependency".into(),
            ));
        }
        let id = dependency.id.as_str();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("Unsafe selected input identity"));
        }
        let expected = dependency.hash.as_str();
        // An existing relative ancestor remains valid after moving the folder;
        // never replace it with an obsolete absolute path from historical events.
        if let Some((directory, _)) = inherited.iter().find(|(_, p)| p.id == id) {
            if basecraft_io::hash_file(&directory.join("records").join(format!("{id}.json")))?
                != expected
            {
                return Err(invalid("Inherited selected input identity differs"));
            }
        } else {
            let directory = crate::records::input_evidence::registered_run(id, expected, bundle)?;
            let reference = basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::records::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &directory,
                )?)
                .into(),
                id: (id).to_owned(),
                hash: (expected).to_owned(),
            };
            if !upstream.contains(&reference) {
                upstream.push(reference.clone());
                links.insert(event.clone(), reference);
            } else if links.values().any(|owned| owned == &reference) {
                links.insert(event.clone(), reference);
            }
        }
        if let Some(reference) = upstream.iter().find(|r| r.id == id)
            && links.values().any(|owned| owned == reference)
        {
            links.insert(event.clone(), reference.clone());
        }
        processed.push(serde_json::json!(event));
    }
    input
        .extra
        .insert("selected_input_links".into(), serde_json::json!(links));
    input.upstream = upstream;
    input
        .extra
        .insert("sanger_input_events".into(), serde_json::json!(processed));
    crate::records::input_evidence::write_json(&path, &input)
}

/// Publish a finished analysis run into its own bundle, keeping the events and
/// effective edits of the publication already there. Callers retain pending
/// artifacts and report partial writes if this fails.
pub fn publish_run(
    bundle: &Path,
    id: &str,
    origin: String,
    inputs: Vec<ArtifactRevision>,
    outputs: Vec<ArtifactRevision>,
) -> Result<Publication> {
    let (events, effective_edits) = std::fs::read(bundle.join("manifest.json"))
        .ok()
        .and_then(|bytes| basecraft_protocol::records::decode::<Publication>(&bytes).ok())
        .map(|previous| (previous.events, previous.effective_edits))
        .unwrap_or_default();
    let publication = Publication {
        schema_version: 1,
        id: id.into(),
        session: basecraft_platform::process_instance().into(),
        parents: vec![],
        parent_hashes: BTreeMap::new(),
        created: basecraft_platform::now_rfc3339(),
        origin,
        inputs,
        outputs,
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
    publish(bundle, &publication)?;
    Ok(publication)
}

/// The journal entry for a tool run that changed no open document, kept in
/// the run's own publication; `doc` names the operation. The record's inputs
/// are the publication's; `base` is what the run link is relative to.
pub fn file_run_entry(
    record: crate::session::ToolRunRecord<'_>,
    base: &Path,
    publication: &Publication,
    started: &str,
    actor: Actor,
    doc: &str,
) -> Result<LogEntry> {
    let names =
        |artifacts: &[ArtifactRevision]| artifacts.iter().map(|a| a.logical_name.clone()).collect();
    let op = record.op(base, names(&publication.outputs), None, None)?;
    Ok(LogEntry {
        v: basecraft_protocol::ops::LOG_VERSION,
        id: publication.id.clone(),
        t: started.into(),
        host: String::new(),
        session: publication.session.clone(),
        actor,
        doc: doc.into(),
        source: None,
        op,
        result: OpResult {
            notes: Vec::new(),
            hashes: publication
                .inputs
                .iter()
                .chain(&publication.outputs)
                .map(|artifact| (artifact.logical_name.clone(), artifact.hash.clone()))
                .collect(),
            feature: None,
        },
        state: DocState {
            counts: None,
            len: 0,
            features: 0,
            rows: None,
        },
    })
}

/// Artifacts must already be durable before this final manifest publication.
pub fn publish(bundle: &Path, publication: &Publication) -> Result<()> {
    stage(bundle, publication)?.publish().map(|_| ())
}

/// Stage the record, reports, README and manifest. They sync together on
/// publication and are renamed in order, the manifest last.
pub(crate) fn stage(
    bundle: &Path,
    publication: &Publication,
) -> Result<crate::documents::PreparedWrite> {
    std::fs::create_dir_all(bundle.join("records"))?;
    link_selected_inputs(bundle, publication)?;
    let mut prepared = crate::documents::PreparedWrite::default();
    stage_json(
        &mut prepared,
        &bundle
            .join("records")
            .join(format!("{}.json", publication.id)),
        publication,
    )?;
    stage_reports(&mut prepared, bundle, publication)?;
    prepared.push(&bundle.join("README.md"), |w| {
        w.write_all(
        b"# Basecraft provenance v1\n\nmanifest.json selects the latest publication. records/ holds immutable publication records; artifacts/ holds BLAKE3-addressed original and saved bytes. Reports are generated descriptions, not inferred laboratory protocols. Imported history before Basecraft is unknown. Checksums detect changes; they are not signatures. No file in this bundle should be executed to inspect or verify it.\n"
    )
    })?;
    stage_json(&mut prepared, &bundle.join("manifest.json"), publication)?;
    Ok(prepared)
}

/// Deterministic evidence-based reports.
pub fn write_reports(output: &Path, publication: &Publication) -> Result<()> {
    let mut prepared = crate::documents::PreparedWrite::default();
    stage_reports(&mut prepared, output, publication)?;
    prepared.publish().map(|_| ())
}

fn stage_reports(
    prepared: &mut crate::documents::PreparedWrite,
    output: &Path,
    publication: &Publication,
) -> Result<()> {
    std::fs::create_dir_all(output.join("reports"))?;
    let report =
        crate::records::document_record::DocumentRecord::from_publication(publication).report()?;
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
    let mappings = report
        .paragraphs
        .iter()
        .enumerate()
        .map(|(index, paragraph)| {
            serde_json::to_value(basecraft_protocol::report_records::ReportSourceMapEntry {
                paragraph: index + 1,
                activities: paragraph.activity_ids.clone(),
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?;
    for (name, bytes) in [
        ("history.md", history.as_bytes()),
        ("methods.generated.md", methods.as_bytes()),
        ("parameters.tsv", params.as_bytes()),
    ] {
        prepared.push(&output.join("reports").join(name), |w| w.write_all(bytes))?;
    }
    // A list carries no version; the report manifest beside it does.
    let mut map = serde_json::to_vec_pretty(&mappings)?;
    map.push(b'\n');
    prepared.push(&output.join("reports/source-map.json"), |w| {
        w.write_all(&map)
    })?;
    stage_json(
        prepared,
        &output.join("reports/manifest.json"),
        &serde_json::to_value(
            basecraft_protocol::report_records::ProvenanceReportManifest {
                schema_version: 1,
                publication: publication.id.clone(),
                template_version: 2,
                output_hashes: publication.outputs.iter().map(|a| a.hash.clone()).collect(),
                redactions: vec!["machine-local paths".into(), "host".into(), "user".into()],
                citations: report.citations,
            },
        )?,
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
pub fn package(
    result: &Path,
    destination: &Path,
    project: Option<&crate::project::Project>,
) -> Result<()> {
    if crate::records::analysis_store::ArtifactRun::for_result(result)?.is_some()
        || bundle_for_result(result, project)
            .join("input.json")
            .is_file()
    {
        return crate::records::input_evidence::package(result, destination, project);
    }
    let issues = verify(result, project)?;
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
    let publication = inspect(result, project)?;
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
    copy_tree(&bundle_for_result(result, project), &companion(&copied))?;
    // The copy carries its history beside it, outside any project.
    let copied_issues = verify(&copied, None)?;
    if !copied_issues.is_empty() {
        return Err(invalid(copied_issues.join("; ")));
    }
    // The destination is deliberately new: never merge unrelated package bytes.
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    basecraft_platform::atomic::rename_noreplace(staging.path(), destination)?;
    Ok(())
}
