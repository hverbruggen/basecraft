//! Single-trace caller service: runs the one installed plugin that declares
//! `caller: single_trace`, through the public registry, shared jobs and
//! retained receipts. The plugin names and hashes its own program through its
//! `engine` query; the host knows only the contract and the Sanger draft.
use crate::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
    runner::{
        CompletedResultQuery, VerifiedOperationResult, lookup_completed_result,
        verify_completed_result,
    },
};
use basecraft_docs::sanger::SangerDraft;
use basecraft_docs::sanger::SangerDraftExt;
use basecraft_protocol::operation::{
    InputReference, InvocationInputEvidence, OperationRequest, RuntimeState,
};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct SangerCaller {
    jobs: JobManager,
    active: Option<(String, Identity)>,
    pub lookup_diagnostics: Vec<String>,
}

struct Identity {
    engine: String,
    /// The draft's file name inside the caller's primary output.
    primary: String,
    capture: basecraft_ops::sanger_review::CapturedInput,
    source: PathBuf,
    sha256: String,
    query: CompletedResultQuery,
}

pub struct CalledDraft {
    pub draft: SangerDraft,
    pub run: PathBuf,
}

/// The caller's program, as its plugin reports it: name and SHA-256.
fn engine(
    operation: &crate::registry::RegisteredOperation,
    interpreter: &Path,
) -> Result<(String, String), String> {
    let mut process = crate::PluginProcess::spawn_with(&operation.package.manifest, interpreter)
        .map_err(|e| e.to_string())?;
    let answer = process.query("engine").map_err(|e| e.to_string());
    let _ = process.finish();
    let answer = answer?;
    if let Some(problem) = answer["problem"].as_str() {
        return Err(problem.into());
    }
    match (answer["name"].as_str(), answer["sha256"].as_str()) {
        (Some(name), Some(sha256)) => Ok((name.into(), sha256.into())),
        _ => Err("Caller reported no engine".into()),
    }
}

fn checked(verified: VerifiedOperationResult, identity: &Identity) -> Result<CalledDraft, String> {
    let outcome = &verified.outcome;
    let artifact = outcome
        .artifacts
        .iter()
        .find(|artifact| {
            Path::new(&artifact.path)
                .file_name()
                .is_some_and(|name| name == identity.primary.as_str())
        })
        .ok_or("Caller did not retain draft evidence")?;
    let path = PathBuf::from(&artifact.path);
    let mut draft = SangerDraft::read(&path)?;
    let effective = &identity.query.effective_parameters;
    if draft.inputs.len() != 1
        || !draft.engine.eq_ignore_ascii_case(&identity.engine)
        || draft.inputs[0].sha256 != identity.sha256
        || draft.engine_sha256 != effective["engine_sha256"].as_str().unwrap_or_default()
        || draft.signal_threshold.map(u64::from) != effective["signal_threshold"].as_u64()
        || u64::try_from(draft.minimum_overlap).ok() != effective["minimum_overlap"].as_u64()
        || Some(draft.minimum_identity.to_bits())
            != effective["minimum_identity"].as_f64().map(f64::to_bits)
    {
        return Err(
            "Source or executed caller identity changed; instrument review preserved".into(),
        );
    }
    identity
        .capture
        .check_current()
        .map_err(|e| e.to_string())?;
    let trace = basecraft_io::abif::parse(identity.capture.bytes()).map_err(|e| e.to_string())?;
    if draft.inputs[0].calls.as_ref().is_none_or(|calls| {
        calls
            .positions
            .last()
            .is_none_or(|position| *position as usize >= trace.samples())
    }) {
        return Err("Caller coordinates do not fit the original trace".into());
    }
    let run = verified.directory;
    draft.inputs[0].path.clone_from(&identity.source);
    draft.limitations.push(format!(
        "Caller package {}; contract {}; effective settings {}",
        identity.query.pins.package_digest,
        identity.query.pins.descriptor_digest,
        identity.query.effective_parameters
    ));
    draft.validate()?;
    Ok(CalledDraft { draft, run })
}

impl SangerCaller {
    /// Let the GUI Tasks dialog cancel runs through this caller's manager.
    #[must_use]
    pub fn with_task_cancellation(mut self) -> Self {
        self.jobs = self.jobs.with_task_cancellation();
        self
    }

    /// Returns a verified cached result, or starts a session-owned background job.
    pub fn start(
        &mut self,
        source: &Path,
        sha256: &str,
        signal_threshold: u32,
    ) -> Result<Option<CalledDraft>, String> {
        let capture =
            basecraft_ops::sanger_review::CapturedInput::open(source).map_err(|e| e.to_string())?;
        if capture.sha256() != sha256 {
            return Err("AB1 changed since opening; reopen it before calling".into());
        }
        self.start_captured(capture, signal_threshold)
    }

    pub fn start_captured(
        &mut self,
        capture: basecraft_ops::sanger_review::CapturedInput,
        signal_threshold: u32,
    ) -> Result<Option<CalledDraft>, String> {
        if !(1..=90).contains(&signal_threshold) {
            return Err("Signal cutoff must be between 1 and 90".into());
        }
        self.cancel()?;
        capture.check_current().map_err(|e| e.to_string())?;
        let source = capture.path().to_path_buf();
        let sha256 = capture.sha256().to_owned();
        let registry = Registry::load(&RegistryConfig::discover().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let mut callers = registry.operations.values().filter(|operation| {
            operation.descriptor.host.as_ref().is_some_and(|host| {
                host.caller == Some(basecraft_protocol::operation::CallerKind::SingleTrace)
            })
        });
        let operation = callers
            .next()
            .ok_or("Install a single-trace caller plugin to compute alternative calls")?;
        if callers.next().is_some() {
            return Err("More than one single-trace caller is installed".into());
        }
        let name = &operation.package.manifest.name;
        if !operation.package.enabled {
            return Err(format!("{name} is disabled"));
        }
        let interpreter = operation
            .package
            .interpreter
            .as_ref()
            .ok_or_else(|| format!("{name} Python runtime is unavailable"))?;
        let (program, engine_sha256) = engine(operation, interpreter)?;
        let parameters = crate::contract::normalize_parameters(
            &operation.descriptor,
            &json!({"engine_sha256": engine_sha256, "signal_threshold": signal_threshold}),
        )
        .map_err(|e| e.to_string())?;
        let binding = operation
            .descriptor
            .inputs
            .keys()
            .next()
            .ok_or("Caller declares no input")?
            .clone();
        let primary = operation
            .descriptor
            .outputs
            .values()
            .find_map(|output| output.primary.clone())
            .ok_or("Caller declares no primary output")?;
        let hash = capture.hash().to_owned();
        let identity = Identity {
            engine: program,
            primary,
            capture: capture.clone(),
            source: source.clone(),
            sha256,
            query: CompletedResultQuery {
                operation: operation.id.clone(),
                pins: operation.pins(&registry.generation),
                effective_parameters: parameters.clone(),
                inputs: std::collections::BTreeMap::from([(
                    binding.clone(),
                    vec![InvocationInputEvidence {
                        original_path: None,
                        source_hash: Some(hash.clone()),
                        submitted_hash: hash,
                        transformation: "identity-v1".into(),
                        run_directory: String::new(),
                    }],
                )]),
            },
        };
        let root = source.parent().ok_or("AB1 has no parent folder")?;
        let lookup = lookup_completed_result(&root.join("operation"), &identity.query);
        self.lookup_diagnostics = lookup.diagnostics;
        if let Some(result) = lookup.result {
            match checked(result, &identity) {
                Ok(calls) => return Ok(Some(calls)),
                Err(error) => self.lookup_diagnostics.push(error),
            }
        }
        let request = OperationRequest {
            operation: operation.id.clone(),
            inputs: std::collections::BTreeMap::from([(
                "reads".into(),
                vec![InputReference::File {
                    path: source.to_str().ok_or("AB1 path is not UTF-8")?.into(),
                }],
            )]),
            parameters,
            pins: Some(identity.query.pins.clone()),
            destinations: std::collections::BTreeMap::new(),
            request_key: None,
            target: None,
        };
        self.jobs = std::mem::take(&mut self.jobs).with_captured_inputs(vec![capture]);
        let accepted = self
            .jobs
            .submit(
                &registry,
                request,
                SessionPolicy::new(root, true).map_err(|e| e.to_string())?,
                "Automatic single-trace alternative calls",
            )
            .map_err(|e| e.to_string())?;
        self.active = Some((accepted.run_id, identity));
        Ok(None)
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        if let Some((id, _)) = self.active.take() {
            self.jobs.cancel(&id).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Block until the started call ends. Setting `cancelled` cancels it.
    pub fn wait(
        &mut self,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<CalledDraft, String> {
        let (id, identity) = self.active.take().ok_or("No call was started")?;
        let outcome = self
            .jobs
            .wait(&id, None, &|| {
                cancelled.load(std::sync::atomic::Ordering::Acquire)
            })
            .map_err(|e| e.to_string())?;
        if outcome.state != RuntimeState::Completed {
            return Err(outcome.errors.first().map_or_else(
                || "Alternative calling cancelled".into(),
                |error| error.message.clone(),
            ));
        }
        let verified =
            verify_completed_result(&outcome, &identity.query).map_err(|e| e.to_string())?;
        checked(verified, &identity)
    }
}
