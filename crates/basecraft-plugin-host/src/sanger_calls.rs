//! Single-trace caller service: runs the one installed plugin that declares
//! `caller: single_trace`, through the public registry, shared jobs and
//! retained receipts. The plugin names and hashes its own program through its
//! `engine` query; the host knows only the contract and the Sanger draft.
use crate::{
    Error, Result,
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
    InputReference, InvocationInputEvidence, OperationErrorCode as Code, OperationRequest,
    RuntimeState,
};
use serde_json::json;
use std::path::{Path, PathBuf};

fn refused(code: Code, message: impl Into<String>) -> Error {
    Error::refused(code, message)
}

fn unavailable(message: impl Into<String>) -> Error {
    Error::refused(Code::PluginUnavailable, message)
}

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
) -> Result<(String, String)> {
    let mut process = crate::PluginProcess::spawn_with(&operation.package.manifest, interpreter)?;
    let answer = process.query("engine");
    let _ = process.finish();
    let answer = answer?;
    if let Some(problem) = answer["problem"].as_str() {
        return Err(unavailable(problem));
    }
    match (answer["name"].as_str(), answer["sha256"].as_str()) {
        (Some(name), Some(sha256)) => Ok((name.into(), sha256.into())),
        _ => Err(refused(
            Code::PluginProtocolViolation,
            "Caller reported no engine",
        )),
    }
}

fn checked(verified: VerifiedOperationResult, identity: &Identity) -> Result<CalledDraft> {
    let violation = |message: &str| refused(Code::PluginProtocolViolation, message);
    let outcome = &verified.outcome;
    let artifact = outcome
        .artifacts
        .iter()
        .find(|artifact| {
            Path::new(&artifact.path)
                .file_name()
                .is_some_and(|name| name == identity.primary.as_str())
        })
        .ok_or_else(|| violation("Caller did not retain draft evidence"))?;
    let path = PathBuf::from(&artifact.path);
    let mut draft: SangerDraft =
        basecraft_ops::sanger_review::read_evidence(&path, SangerDraft::validate)?;
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
        return Err(basecraft_ops::Error::Outdated(
            "Source or executed caller identity changed; instrument review preserved".into(),
        )
        .into());
    }
    identity.capture.check_current()?;
    let trace = basecraft_io::abif::parse(identity.capture.bytes())?;
    if draft.inputs[0].calls.as_ref().is_none_or(|calls| {
        calls
            .positions
            .last()
            .is_none_or(|position| *position as usize >= trace.samples())
    }) {
        return Err(violation(
            "Caller coordinates do not fit the original trace",
        ));
    }
    let run = verified.directory;
    draft.inputs[0].path.clone_from(&identity.source);
    draft.limitations.push(format!(
        "Caller package {}; contract {}; effective settings {}",
        identity.query.pins.package_digest,
        identity.query.pins.descriptor_digest,
        identity.query.effective_parameters
    ));
    draft.validate().map_err(|e| violation(&e))?;
    Ok(CalledDraft { draft, run })
}

impl SangerCaller {
    /// Returns a verified cached result, or starts a session-owned background job.
    pub fn start(
        &mut self,
        source: &Path,
        sha256: &str,
        signal_threshold: u32,
    ) -> Result<Option<CalledDraft>> {
        let capture = basecraft_ops::sanger_review::CapturedInput::open(source)?;
        if capture.sha256() != sha256 {
            return Err(basecraft_ops::Error::ChangedOnDisk {
                path: source.into(),
            }
            .into());
        }
        self.start_captured(capture, signal_threshold)
    }

    pub fn start_captured(
        &mut self,
        capture: basecraft_ops::sanger_review::CapturedInput,
        signal_threshold: u32,
    ) -> Result<Option<CalledDraft>> {
        if !(1..=90).contains(&signal_threshold) {
            return Err(refused(
                Code::InvalidArguments,
                "Signal cutoff must be between 1 and 90",
            ));
        }
        self.cancel()?;
        capture.check_current()?;
        let source = capture.path().to_path_buf();
        let sha256 = capture.sha256().to_owned();
        let registry = Registry::load(&RegistryConfig::discover()?)?;
        let mut callers = registry.operations.values().filter(|operation| {
            operation.descriptor.host.as_ref().is_some_and(|host| {
                host.caller == Some(basecraft_protocol::operation::CallerKind::SingleTrace)
            })
        });
        let operation = callers.next().ok_or_else(|| {
            unavailable("Install a single-trace caller plugin to compute alternative calls")
        })?;
        if callers.next().is_some() {
            return Err(unavailable(
                "More than one single-trace caller is installed",
            ));
        }
        let name = &operation.package.manifest.name;
        if !operation.package.enabled {
            return Err(refused(Code::PluginDisabled, format!("{name} is disabled")));
        }
        let interpreter = operation
            .package
            .interpreter
            .as_ref()
            .ok_or_else(|| unavailable(format!("{name} Python runtime is unavailable")))?;
        let (program, engine_sha256) = engine(operation, interpreter)?;
        let parameters = crate::contract::normalize_parameters(
            &operation.descriptor,
            &json!({"engine_sha256": engine_sha256, "signal_threshold": signal_threshold}),
        )?;
        let binding = operation
            .descriptor
            .inputs
            .keys()
            .next()
            .ok_or_else(|| refused(Code::IncompatibleContract, "Caller declares no input"))?
            .clone();
        let primary = operation
            .descriptor
            .outputs
            .values()
            .find_map(|output| output.primary.clone())
            .ok_or_else(|| {
                refused(
                    Code::IncompatibleContract,
                    "Caller declares no primary output",
                )
            })?;
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
                        transformation: basecraft_protocol::input_records::Transformation::Identity,
                        run_directory: String::new(),
                    }],
                )]),
            },
        };
        let root = source
            .parent()
            .ok_or_else(|| refused(Code::InvalidArguments, "AB1 has no parent folder"))?;
        let lookup = lookup_completed_result(&root.join("operation"), &identity.query);
        self.lookup_diagnostics = lookup.diagnostics;
        if let Some(result) = lookup.result {
            match checked(result, &identity) {
                Ok(calls) => return Ok(Some(calls)),
                Err(error) => self.lookup_diagnostics.push(error.to_string()),
            }
        }
        let request = OperationRequest {
            operation: operation.id.clone(),
            inputs: std::collections::BTreeMap::from([(
                "reads".into(),
                vec![InputReference::File {
                    path: source
                        .to_str()
                        .ok_or_else(|| {
                            basecraft_ops::Error::Unsupported("AB1 path is not UTF-8".into())
                        })?
                        .into(),
                }],
            )]),
            parameters,
            pins: Some(identity.query.pins.clone()),
            destinations: std::collections::BTreeMap::new(),
            request_key: None,
            target: None,
        };
        self.jobs = std::mem::take(&mut self.jobs).with_captured_inputs(vec![capture]);
        let accepted = self.jobs.submit(
            &registry,
            request,
            SessionPolicy::new(root, true)?,
            "Automatic single-trace alternative calls",
        )?;
        self.active = Some((accepted.run_id, identity));
        Ok(None)
    }

    pub fn cancel(&mut self) -> Result<()> {
        if let Some((id, _)) = self.active.take() {
            self.jobs.cancel(&id)?;
        }
        Ok(())
    }

    /// Block until the started call ends. Setting `cancelled` cancels it.
    pub fn wait(&mut self, cancelled: &std::sync::atomic::AtomicBool) -> Result<CalledDraft> {
        let (id, identity) = self
            .active
            .take()
            .ok_or_else(|| refused(Code::InvalidArguments, "No call was started"))?;
        let outcome = self.jobs.wait(&id, None, &|| {
            cancelled.load(std::sync::atomic::Ordering::Acquire)
        })?;
        if outcome.state != RuntimeState::Completed {
            return Err(outcome.errors.first().map_or_else(
                || refused(Code::Cancelled, "Alternative calling cancelled"),
                |error| refused(error.code, error.message.clone()),
            ));
        }
        let verified = verify_completed_result(&outcome, &identity.query)?;
        checked(verified, &identity)
    }
}
