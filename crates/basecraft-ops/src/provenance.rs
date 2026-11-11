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

/// Select the existing binding, companion, project-derived or local history path.
/// The final fallback is the original companion spelling, even if it does not
/// exist. Selection does not validate content or establish complete provenance.
pub fn bundle_for_result(result: &Path) -> PathBuf {
    resolve_bundle(result, &mut None).0
}

// Intentionally unwired: observation is not an access/consent capability.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct BundleObservation {
    pub selected: PathBuf,
    pub selected_by: BundleSelection,
    /// Ordered attempted stages. An absent stage is `NotAttempted`, not absent on
    /// disk: an earlier selection or a failed/skipped prerequisite prevented it.
    pub steps: Vec<ResolverStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleSelection {
    Binding,
    Companion,
    Project,
    Local,
    CompanionFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolverStage {
    Binding,
    HiddenManifest,
    CompanionManifest,
    Canonicalize,
    ProjectMarker,
    ProjectRelative,
    Discover,
    Settings,
    LocalManifest,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum ResolverStep {
    /// None is opaque owner output, not proof that no binding exists.
    Binding(Result<Option<PathBuf>>),
    /// Following metadata only: `NotFound` can include a dangling link or missing
    /// ancestor; a regular file need not be readable or remain the same object.
    Metadata(ResolverStage, PathBuf, std::io::Result<std::fs::Metadata>),
    /// Failure retains original spelling for all subsequent calculations.
    Canonicalize(std::io::Result<PathBuf>),
    ProjectRelative(
        PathBuf,
        std::result::Result<PathBuf, std::path::StripPrefixError>,
    ),
    Discover(std::result::Result<(), basecraft_platform::paths::PathError>),
    /// Successful policy has unknown origin: load can return defaults on missing
    /// preferences. Non-LocalOnly policy skips the local manifest stage.
    Settings(
        std::result::Result<
            basecraft_platform::settings::ProvenancePolicy,
            basecraft_platform::settings::Error,
        >,
    ),
}

#[allow(dead_code)]
pub(crate) fn observe_bundle_for_result(result: &Path) -> BundleObservation {
    let mut steps = Some(Vec::new());
    let (selected, selected_by) = resolve_bundle(result, &mut steps);
    BundleObservation {
        selected,
        selected_by,
        steps: steps.unwrap_or_default(),
    }
}

// The closure is never evaluated on the legacy route: no diagnostic path
// copies, vector allocation, error clones or additional probes on that route.
fn resolver_record(steps: &mut Option<Vec<ResolverStep>>, step: impl FnOnce() -> ResolverStep) {
    if let Some(steps) = steps {
        steps.push(step());
    }
}

fn resolver_call(stage: ResolverStage) {
    #[cfg(test)]
    resolver_observation_tests::called(stage);
    #[cfg(not(test))]
    let _ = stage;
}

fn resolver_file(path: &Path, stage: ResolverStage, steps: &mut Option<Vec<ResolverStep>>) -> bool {
    resolver_call(stage);
    let metadata = std::fs::metadata(path);
    let is_file = metadata.as_ref().is_ok_and(std::fs::Metadata::is_file);
    resolver_record(steps, || {
        ResolverStep::Metadata(stage, path.to_path_buf(), metadata)
    });
    is_file
}

fn resolve_bundle(
    result: &Path,
    steps: &mut Option<Vec<ResolverStep>>,
) -> (PathBuf, BundleSelection) {
    resolver_call(ResolverStage::Binding);
    let binding = crate::analysis_store::ArtifactRun::for_result(result);
    // The selected candidate must be owned by both return and observation only
    // when collecting. Errors are moved exactly once, never reconstructed.
    let hidden = match binding {
        Ok(hidden) => {
            resolver_record(steps, || ResolverStep::Binding(Ok(hidden.clone())));
            hidden
        }
        Err(error) => {
            resolver_record(steps, || ResolverStep::Binding(Err(error)));
            None
        }
    };
    if let Some(hidden) = hidden
        && resolver_file(
            &hidden.join("manifest.json"),
            ResolverStage::HiddenManifest,
            steps,
        )
    {
        return (hidden, BundleSelection::Binding);
    }
    let standalone = companion(result);
    if resolver_file(
        &standalone.join("manifest.json"),
        ResolverStage::CompanionManifest,
        steps,
    ) {
        return (standalone, BundleSelection::Companion);
    }
    resolver_call(ResolverStage::Canonicalize);
    let absolute = match result.canonicalize() {
        Ok(path) => {
            resolver_record(steps, || ResolverStep::Canonicalize(Ok(path.clone())));
            path
        }
        Err(error) => {
            resolver_record(steps, || ResolverStep::Canonicalize(Err(error)));
            result.to_path_buf()
        }
    };
    for root in absolute.ancestors().skip(1) {
        if resolver_file(
            &root.join(".basecraft/project.toml"),
            ResolverStage::ProjectMarker,
            steps,
        ) {
            resolver_call(ResolverStage::ProjectRelative);
            let relative = match absolute.strip_prefix(root) {
                Ok(relative) => {
                    resolver_record(steps, || {
                        ResolverStep::ProjectRelative(
                            root.to_path_buf(),
                            Ok(relative.to_path_buf()),
                        )
                    });
                    Some(relative)
                }
                Err(error) => {
                    resolver_record(steps, || {
                        ResolverStep::ProjectRelative(root.to_path_buf(), Err(error))
                    });
                    None
                }
            };
            if let Some(relative) = relative {
                let key = relative.to_string_lossy();
                return (
                    root.join(".basecraft/provenance")
                        .join(basecraft_io::hash_bytes(key.as_bytes()).trim_start_matches("b3:")),
                    BundleSelection::Project,
                );
            }
        }
    }
    resolver_call(ResolverStage::Discover);
    match basecraft_platform::paths::AppPaths::discover() {
        Ok(paths) => {
            resolver_record(steps, || ResolverStep::Discover(Ok(())));
            resolver_call(ResolverStage::Settings);
            match basecraft_platform::settings::SettingsStore::new(paths.preferences()).load() {
                Ok(preferences) => {
                    let policy = preferences.files.provenance;
                    resolver_record(steps, || ResolverStep::Settings(Ok(policy)));
                    if policy == basecraft_platform::settings::ProvenancePolicy::LocalOnly {
                        let local = paths.state.join("provenance").join(
                            basecraft_io::hash_bytes(absolute.as_os_str().as_encoded_bytes())
                                .trim_start_matches("b3:"),
                        );
                        if resolver_file(
                            &local.join("manifest.json"),
                            ResolverStage::LocalManifest,
                            steps,
                        ) {
                            return (local, BundleSelection::Local);
                        }
                    }
                }
                Err(error) => resolver_record(steps, || ResolverStep::Settings(Err(error))),
            }
        }
        Err(error) => resolver_record(steps, || ResolverStep::Discover(Err(error))),
    }
    (standalone, BundleSelection::CompanionFallback)
}

#[cfg(test)]
mod resolver_observation_tests;

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
    read_record_with_access(path, remaining, &crate::access::TrustedLegacy)
}

/// Authorize before this reader's metadata/open and use the returned spelling.
/// This raw-byte entry is unwired: namespace safety belongs to the supplied
/// access owner, not this reader. It does not decode records or call services.
pub(crate) fn read_record_with_access(
    path: &Path,
    remaining: &mut u64,
    access: &dyn crate::access::Access,
) -> Result<Vec<u8>> {
    read_record_with_access_open(path, remaining, access, |path| {
        let file = basecraft_platform::files::open_regular_read(path)?;
        let length = file.metadata()?.len();
        Ok((file, length))
    })
}

// One bounded body for both entry points; the opener seam permits observing
// denial ordering and partial read errors without a second test algorithm.
fn read_record_with_access_open<R: std::io::Read>(
    path: &Path,
    remaining: &mut u64,
    access: &dyn crate::access::Access,
    open: impl FnOnce(&Path) -> std::io::Result<(R, u64)>,
) -> Result<Vec<u8>> {
    let checked = access.resource(path, crate::access::ResourceAction::ReadFile)?;
    let (file, length) = open(&checked)?;
    let bytes = read_record_from_open(file, length, remaining, 64 << 20, RecordLimit::Provenance)?;
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::record_read(&checked, bytes.len() as u64);
    Ok(bytes)
}

#[derive(Clone, Copy)]
pub(crate) enum RecordLimit {
    Provenance,
    Registry,
}
impl RecordLimit {
    fn message(self) -> &'static str {
        match self {
            Self::Provenance => "provenance record exceeds reading limit (at most 64 MiB)",
            Self::Registry => "known-store registry exceeds 1 MiB",
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
        return Err(invalid(label.message()));
    }
    let mut bytes = Vec::new();
    let result = file.take(limit + 1).read_to_end(&mut bytes);
    *remaining = remaining.saturating_sub(bytes.len() as u64);
    result?;
    if bytes.len() as u64 > limit {
        return Err(invalid(label.message()));
    }
    Ok(bytes)
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
    if bundle.join("input.json").is_file() {
        if crate::input_evidence::read_input(&bundle)?.state == "broken" {
            issues.push("Input evidence explicitly broken".into());
        }
        match crate::input_evidence::verify_ancestors(&bundle) {
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
    let hidden = crate::analysis_store::ArtifactRun::for_result(result)?.is_some();
    for (index, output) in publication.outputs.iter().enumerate() {
        let path = if index == 0 && !hidden {
            result.to_path_buf()
        } else {
            let base = if output.retained.as_deref() == Some(output.logical_name.as_str()) {
                bundle.as_path()
            } else if hidden {
                bundle
                    .ancestors()
                    .nth(4)
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
                if artifact.hash != reference.hash || artifact.size != reference.stamp.size {
                    issues.push(format!(
                        "{}: ancestor input revision/reference identity differs",
                        artifact.id
                    ));
                }
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

// Carry explicitly selected input runs into ordinary saved ancestry: a Sanger
// review chosen for an alignment (`sanger_input`) or any other `ToolRun` that
// names a registered reference ledger (`input_run`), such as imported support
// values. The event remains a ToolRun; edits, undo and replay are unchanged.
// Processed events stay listed under the original `sanger_input_events` key.
fn link_selected_inputs(bundle: &Path, publication: &Publication) -> Result<()> {
    let mut effective = publication.effective_edits.clone();
    crate::contributions::project(&publication.events, &mut effective);
    let retained: std::collections::HashSet<_> = effective.iter().map(|e| e.id.as_str()).collect();
    let dependencies: Vec<_> = publication
        .events
        .iter()
        .filter_map(|event| {
            if let basecraft_protocol::Op::ToolRun {
                plugin,
                operation,
                params,
                ..
            } = &event.op
            {
                if plugin == "basecraft.sanger-review" && operation == "reviewed_consensus_input" {
                    return Some((&event.id, &params["sanger_input"]));
                }
                if params.get("contributions").is_some() && !retained.contains(event.id.as_str()) {
                    return None;
                }
                if let Some(run) = params.get("input_run") {
                    return Some((&event.id, run));
                }
            }
            None
        })
        .collect();
    let path = bundle.join("input.json");
    if dependencies.is_empty() && !path.is_file() {
        return Ok(());
    }
    let mut input = if path.is_file() {
        crate::input_evidence::read_input(bundle)?
    } else {
        let output = publication
            .outputs
            .first()
            .ok_or_else(|| invalid("input publication has no result"))?;
        crate::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: output.hash.clone(),
            submitted_size: output.size,
            transformation: "identity-v1".into(),
            state: "generated".into(),
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
        return crate::input_evidence::write_json(&path, &input);
    }
    let mut upstream = input
        .extra
        .get("upstream")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Keep ownership of selected edges separate from unrelated scientific inputs.
    // Historical publications and their records are left intact.
    let mut links = input
        .extra
        .get("selected_input_links")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    for reference in links.values_mut() {
        if let Some(current) = upstream.iter().find(|current| {
            current["id"] == reference["id"] && current["hash"] == reference["hash"]
        }) {
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
    let inherited = crate::input_evidence::ancestors(bundle)?;
    for (event, dependency) in dependencies {
        if processed.iter().any(|id| id.as_str() == Some(event)) {
            continue;
        }
        if dependency["schema_version"] != 1 {
            return Err(invalid("Unsupported selected input dependency"));
        }
        let id = dependency["id"]
            .as_str()
            .ok_or_else(|| invalid("Selected input has no identity"))?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("Unsafe selected input identity"));
        }
        let expected = dependency["hash"]
            .as_str()
            .ok_or_else(|| invalid("Selected input has no checksum"))?;
        // An existing relative ancestor remains valid after moving the folder;
        // never replace it with an obsolete absolute path from historical events.
        if let Some((directory, _)) = inherited.iter().find(|(_, p)| p.id == id) {
            if basecraft_io::hash_file(&directory.join("records").join(format!("{id}.json")))?
                != expected
            {
                return Err(invalid("Inherited selected input identity differs"));
            }
        } else {
            let directory = crate::input_evidence::registered_run(id, expected)?;
            let reference = serde_json::json!({"bundle":crate::input_evidence::relative(&bundle.canonicalize()?,&directory)?,"id":id,"hash":expected});
            if !upstream.contains(&reference) {
                upstream.push(reference.clone());
                links.insert(event.clone(), reference);
            } else if links.values().any(|owned| owned == &reference) {
                links.insert(event.clone(), reference);
            }
        }
        if let Some(reference) = upstream.iter().find(|r| r["id"].as_str() == Some(id))
            && links.values().any(|owned| owned == reference)
        {
            links.insert(event.clone(), reference.clone());
        }
        processed.push(serde_json::json!(event));
    }
    input
        .extra
        .insert("selected_input_links".into(), serde_json::json!(links));
    input
        .extra
        .insert("upstream".into(), serde_json::json!(upstream));
    input
        .extra
        .insert("sanger_input_events".into(), serde_json::json!(processed));
    crate::input_evidence::write_json(&path, &input)
}

/// Publish a host-validated operation using the ordinary provenance writer.
/// Callers retain pending artifacts and report partial writes if this fails.
pub fn publish_operation(bundle: &Path, publication: &Publication) -> Result<()> {
    publish(bundle, publication)
}

/// Artifacts must already be durable before this final manifest publication.
pub(crate) fn publish(bundle: &Path, publication: &Publication) -> Result<()> {
    std::fs::create_dir_all(bundle.join("records"))?;
    link_selected_inputs(bundle, publication)?;
    let mut prepared = crate::documents::PreparedWrite::default();
    let mut bytes = serde_json::to_vec_pretty(publication)?;
    bytes.push(b'\n');
    if bytes.len() > 64 << 20 {
        return Err(crate::Error::Limit {
            what: "provenance record bytes".into(),
            limit: 64 << 20,
        });
    }
    prepared.push(
        &bundle
            .join("records")
            .join(format!("{}.json", publication.id)),
        |w| w.write_all(&bytes),
    )?;
    prepare_reports(bundle, publication, &mut prepared)?;
    prepared.push(&bundle.join("README.md"), |w| {
        w.write_all(
        b"# Basecraft provenance v1\n\nmanifest.json selects the latest publication. records/ holds immutable publication records; artifacts/ holds BLAKE3-addressed analysis inputs. Reports are generated descriptions, not inferred laboratory protocols. Imported history before Basecraft is unknown. Checksums detect changes; they are not signatures. No file in this bundle should be executed to inspect or verify it.\n"
    )
    })?;
    prepared.push(&bundle.join("manifest.json"), |w| w.write_all(&bytes))?;
    prepared.publish()?;
    Ok(())
}

/// Deterministic evidence-based reports. User-authored methods.md is untouched.
pub fn write_reports(output: &Path, publication: &Publication) -> Result<()> {
    let mut prepared = crate::documents::PreparedWrite::default();
    prepare_reports(output, publication, &mut prepared)?;
    prepared.publish()?;
    Ok(())
}

fn prepare_reports(
    output: &Path,
    publication: &Publication,
    prepared: &mut crate::documents::PreparedWrite,
) -> Result<()> {
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
    prepared.push(&output.join("reports/source-map.json"), |w| {
        serde_json::to_writer_pretty(&mut *w, &mappings)?;
        w.write_all(b"\n")
    })?;
    prepared.push(&output.join("reports/manifest.json"), |w| {
        serde_json::to_writer_pretty(
            &mut *w,
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
        )?;
        w.write_all(b"\n")
    })?;
    Ok(())
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
    if crate::analysis_store::ArtifactRun::for_result(result)?.is_some()
        || bundle_for_result(result).join("input.json").is_file()
    {
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
    basecraft_platform::atomic::rename_noreplace(staging.path(), destination)?;
    Ok(())
}
