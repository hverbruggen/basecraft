//! Single-read Tracy adapter using the public registry, shared jobs and retained receipts.
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
use basecraft_protocol::operation::{InvocationInputEvidence, OperationRequest, RuntimeState};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct SangerCaller {
    jobs: JobManager,
    active: Option<(String, Identity)>,
    pub lookup_diagnostics: Vec<String>,
}

struct Identity {
    capture: basecraft_ops::sanger_review::CapturedInput,
    source: PathBuf,
    sha256: String,
    query: CompletedResultQuery,
}

pub struct CalledDraft {
    pub draft: SangerDraft,
    pub run: PathBuf,
}

// Match the public SDK resolver: explicit override, interpreter prefix, then PATH.
fn engine(interpreter: &Path) -> Result<PathBuf, String> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("BASECRAFT_TRACY") {
        paths.push(PathBuf::from(path));
    } else {
        if let Some(parent) = interpreter.parent() {
            paths.push(parent.join("tracy"));
        }
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path).map(|folder| folder.join("tracy")));
        }
    }
    paths
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Tracy is unavailable. Install the Tracy dependency; instrument calls remain usable."
                .into()
        })
}

fn checked(verified: VerifiedOperationResult, identity: &Identity) -> Result<CalledDraft, String> {
    let outcome = &verified.outcome;
    let artifact = outcome
        .artifacts
        .iter()
        .find(|artifact| {
            Path::new(&artifact.path)
                .file_name()
                .is_some_and(|name| name == "evidence.json")
        })
        .ok_or("Caller did not retain draft evidence")?;
    let path = PathBuf::from(&artifact.path);
    let mut draft = SangerDraft::read(&path)?;
    if draft.inputs.len() != 1
        || draft.engine != "Tracy"
        || draft.inputs[0].sha256 != identity.sha256
        || draft.engine_sha256
            != identity.query.effective_parameters["engine_sha256"]
                .as_str()
                .unwrap_or_default()
        || draft.signal_threshold.map(u64::from)
            != identity.query.effective_parameters["signal_threshold"].as_u64()
        || draft.minimum_overlap != 50
        || draft.minimum_identity.to_bits() != 0.8_f64.to_bits()
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
            operation
                .descriptor
                .host
                .as_ref()
                .and_then(|host| host.caller.as_deref())
                == Some("single_trace")
        });
        let operation = callers
            .next()
            .ok_or("Install the Tracy Sanger plugin to compute alternative calls")?;
        if callers.next().is_some() {
            return Err("More than one single-trace caller is installed".into());
        }
        if !operation.package.enabled {
            return Err("Tracy Sanger plugin is disabled".into());
        }
        let interpreter = operation
            .package
            .interpreter
            .as_ref()
            .ok_or("Tracy plugin Python runtime is unavailable")?;
        let caller = engine(interpreter)?;
        let parameters = json!({"engine_sha256":basecraft_io::sha256_file(&caller).map_err(|e| e.to_string())?,
            "minimum_overlap":50,"minimum_identity":0.8,"signal_threshold":signal_threshold});
        let hash = capture.hash().to_owned();
        let identity = Identity {
            capture: capture.clone(),
            source: source.clone(),
            sha256,
            query: CompletedResultQuery {
                operation: operation.id.clone(),
                pins: operation.pins(&registry.generation),
                effective_parameters: parameters.clone(),
                inputs: std::collections::BTreeMap::from([(
                    "reads".into(),
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
                vec![basecraft_protocol::operation::InputReference::File {
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

    pub fn poll(&mut self) -> Result<Option<CalledDraft>, String> {
        let Some((id, _)) = &self.active else {
            return Ok(None);
        };
        let outcome = self.jobs.get(id).map_err(|e| e.to_string())?;
        if !outcome.state.terminal() {
            return Ok(None);
        }
        let (_, identity) = self.active.take().ok_or("Caller identity is missing")?;
        if outcome.state != RuntimeState::Completed {
            return Err(outcome.errors.first().map_or_else(
                || "Alternative calling cancelled".into(),
                |error| error.message.clone(),
            ));
        }
        let verified =
            verify_completed_result(&outcome, &identity.query).map_err(|e| e.to_string())?;
        checked(verified, &identity).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn biological_validation_rejects_stale_source_and_wrong_coordinates() {
        use basecraft_protocol::operation::{OperationOutcome, OperationPins, PublishedArtifact};
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &source,
        )
        .unwrap();
        let parameters = json!({"engine_sha256":"a".repeat(64),"minimum_overlap":50,
            "minimum_identity":0.8,"signal_threshold":20});
        let pins = OperationPins {
            generation: "registry".into(),
            package_digest: "package".into(),
            descriptor_digest: "contract".into(),
        };
        let identity = Identity {
            capture: basecraft_ops::sanger_review::CapturedInput::open(&source).unwrap(),
            source: source.clone(),
            sha256: basecraft_io::sha256_file(&source).unwrap(),
            query: CompletedResultQuery {
                operation: "example.caller/consensus@1".into(),
                pins: pins.clone(),
                effective_parameters: parameters.clone(),
                inputs: std::collections::BTreeMap::default(),
            },
        };
        let mut draft = basecraft_ops::sanger_review::instrument_review(&source)
            .unwrap()
            .draft;
        draft.engine = "Tracy".into();
        draft.mode = "basecall".into();
        draft.engine_sha256 = "a".repeat(64);
        draft.signal_threshold = Some(20);
        draft.minimum_overlap = 50;
        draft.minimum_identity = 0.8;
        let path = tmp.path().join("evidence.json");
        std::fs::write(&path, serde_json::to_vec(&draft).unwrap()).unwrap();
        let outcome = OperationOutcome {
            schema: 1,
            operation: "example.caller/consensus@1".into(),
            run_id: "run".into(),
            state: RuntimeState::Completed,
            pins,
            requested_parameters: parameters.clone(),
            effective_parameters: parameters,
            data: None,
            artifacts: vec![PublishedArtifact {
                role: "artifacts".into(),
                path: path.display().to_string(),
                digest: "verified by host".into(),
            }],
            proposals: None,
            warnings: vec![],
            errors: vec![],
            evidence: None,
        };
        let verified = || VerifiedOperationResult {
            outcome: outcome.clone(),
            directory: tmp.path().into(),
        };
        assert!(checked(verified(), &identity).is_ok());
        if let Some(position) = draft.inputs[0].calls.as_mut().unwrap().positions.last_mut() {
            *position = u32::MAX;
        }
        std::fs::write(&path, serde_json::to_vec(&draft).unwrap()).unwrap();
        assert!(checked(verified(), &identity).is_err());
        std::fs::write(&source, "changed source at same pathname").unwrap();
        assert!(checked(verified(), &identity).is_err());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    #[ignore = "requires BASECRAFT_SANGER_FIXTURE, public plugin Python and Tracy"]
    fn actual_open_call_review_reopen_cache_cancel_and_stale_source() {
        use basecraft_ops::sanger_review::{ReviewChoice, ReviewOp, SangerReview};
        let fixture = std::env::var_os("BASECRAFT_SANGER_FIXTURE").expect("public AB1 fixture");
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.ab1");
        std::fs::copy(fixture, &source).unwrap();
        let original = std::fs::read(&source).unwrap();
        let mut review = basecraft_ops::sanger_review::instrument_review(&source).unwrap();
        let instrument_id = review.draft_sha256.clone();
        let sha256 = review.draft.inputs[0].sha256.clone();
        let mut caller = SangerCaller::default();
        assert!(caller.start(&source, &sha256, 20).unwrap().is_none());
        // Review remains editable through Session while the shared job runs.
        let mut session = basecraft_ops::Session::recoverable(tmp.path()).unwrap();
        let id = session.create_sanger_review(&review).unwrap();
        session
            .apply(
                id,
                basecraft_protocol::Op::SangerReview {
                    call_set: instrument_id.clone(),
                    edit: Box::new(ReviewOp::Set {
                        columns: vec![0],
                        choice: ReviewChoice::Edit('G'),
                    }),
                },
            )
            .unwrap();
        let started = std::time::Instant::now();
        let called = loop {
            if let Some(called) = caller.poll().unwrap() {
                break called;
            }
            assert!(started.elapsed().as_secs() < 120, "caller timeout");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        // Shared host verification, not Sanger, owns common receipt/input/artifact rules.
        let receipt: basecraft_protocol::operation::OperationOutcome = serde_json::from_slice(
            &basecraft_ops::provenance::read_record(&called.run.join("runtime.json")).unwrap(),
        )
        .unwrap();
        let execution: serde_json::Value = serde_json::from_slice(
            &basecraft_ops::provenance::read_record(&called.run.join("execution.json")).unwrap(),
        )
        .unwrap();
        let context: basecraft_protocol::operation::InvocationContext =
            serde_json::from_value(execution["context"].clone()).unwrap();
        let mut query = CompletedResultQuery {
            operation: receipt.operation.clone(),
            pins: receipt.pins.clone(),
            effective_parameters: receipt.effective_parameters.clone(),
            inputs: context.input_evidence,
        };
        assert!(verify_completed_result(&receipt, &query).is_ok());
        query.inputs.get_mut("reads").unwrap()[0].submitted_hash = "b3:changed".into();
        assert!(verify_completed_result(&receipt, &query).is_err());
        query.inputs.get_mut("reads").unwrap()[0].submitted_hash =
            basecraft_io::hash_file(&source).unwrap();
        query.pins.package_digest = "changed producer".into();
        assert!(verify_completed_result(&receipt, &query).is_err());
        query.pins = receipt.pins.clone();
        let mut future = receipt.clone();
        future.schema = 99;
        assert!(verify_completed_result(&future, &query).is_err());
        let fake =
            basecraft_ops::analysis_store::ArtifactRun::create(&source, "0000-future").unwrap();
        basecraft_ops::input_evidence::write_json(&fake.artifact("runtime.json").unwrap(), &future)
            .unwrap();
        let corrupt =
            basecraft_ops::analysis_store::ArtifactRun::create(&source, "0001-corrupt").unwrap();
        std::fs::write(
            corrupt.artifact("runtime.json").unwrap(),
            b"corrupt receipt",
        )
        .unwrap();
        let found = lookup_completed_result(&source, &query);
        assert!(found.result.is_some());
        assert_eq!(found.diagnostics.len(), 2);
        let artifact_path = Path::new(&receipt.artifacts[0].path);
        let retained_bytes = std::fs::read(artifact_path).unwrap();
        std::fs::write(artifact_path, b"damaged retained output").unwrap();
        assert!(verify_completed_result(&receipt, &query).is_err());
        std::fs::write(artifact_path, retained_bytes).unwrap();
        let mut computed = SangerReview::from_draft(called.draft, &source).unwrap();
        computed.attach_operation_run(&called.run).unwrap();
        let computed_id = computed.draft_sha256.clone();
        review = session.sanger_review(id).unwrap().clone();
        review.retain_call_set(computed).unwrap();
        session.update_sanger_review(id, &review).unwrap();
        session
            .apply(
                id,
                basecraft_protocol::Op::SangerReview {
                    call_set: computed_id.clone(),
                    edit: Box::new(ReviewOp::Set {
                        columns: vec![0],
                        choice: ReviewChoice::Edit('C'),
                    }),
                },
            )
            .unwrap();
        assert_eq!(
            review.draft_sha256, instrument_id,
            "completion must not switch calls"
        );
        assert_eq!(review.decision(0), Some(&ReviewChoice::Edit('G')));
        assert!(
            caller.start(&source, &sha256, 20).unwrap().is_some(),
            "reuse completed receipt"
        );
        session
            .apply(
                id,
                basecraft_protocol::Op::SangerSelect {
                    call_set: computed_id,
                },
            )
            .unwrap();
        let saved = tmp.path().join("saved.json");
        session.save(id, Some(&saved)).unwrap();
        let mut reopened = basecraft_ops::Session::recoverable(tmp.path()).unwrap();
        let id = reopened.open(&saved).unwrap();
        assert_eq!(
            reopened.sanger_review(id).unwrap().decision(0),
            Some(&ReviewChoice::Edit('C'))
        );
        reopened
            .apply(
                id,
                basecraft_protocol::Op::SangerSelect {
                    call_set: instrument_id,
                },
            )
            .unwrap();
        assert_eq!(
            reopened.sanger_review(id).unwrap().decision(0),
            Some(&ReviewChoice::Edit('G'))
        );
        assert!(caller.start(&source, &sha256, 19).unwrap().is_none());
        caller.cancel().unwrap();
        assert!(
            caller.poll().unwrap().is_none(),
            "late completion must be detached after cancel"
        );
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert!(caller.start(&source, &sha256, 18).unwrap().is_none());
        std::fs::write(&source, "source replaced while caller was running").unwrap();
        let started = std::time::Instant::now();
        loop {
            match caller.poll() {
                Err(_) => break,
                Ok(Some(_)) => panic!("stale completion must never be reusable"),
                Ok(None) => {}
            }
            assert!(started.elapsed().as_secs() < 120, "stale caller timeout");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::write(&source, "different bytes at the same path").unwrap();
        assert!(caller.start(&source, &sha256, 20).is_err());
        assert_eq!(
            reopened.sanger_review(id).unwrap().decision(0),
            Some(&ReviewChoice::Edit('G'))
        );
    }
}
