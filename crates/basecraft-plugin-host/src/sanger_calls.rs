//! Single-read Tracy adapter using the public registry, shared jobs and retained receipts.
use crate::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_docs::sanger::SangerDraft;
use basecraft_protocol::operation::{
    OperationOutcome, OperationPins, OperationRequest, RuntimeState,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const OPERATION: &str = "basecraft.tracy-sanger/consensus@1";

#[derive(Default)]
pub struct SangerCaller {
    jobs: JobManager,
    active: Option<(String, Identity)>,
}

struct Identity {
    source: PathBuf,
    sha256: String,
    parameters: Value,
    pins: OperationPins,
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

fn checked(outcome: &OperationOutcome, identity: &Identity) -> Result<CalledDraft, String> {
    if outcome.state != RuntimeState::Completed
        || outcome.operation != OPERATION
        || outcome.pins.package_digest != identity.pins.package_digest
        || outcome.pins.descriptor_digest != identity.pins.descriptor_digest
        || outcome.effective_parameters != identity.parameters
    {
        return Err("Caller receipt does not match the requested implementation/settings".into());
    }
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
    if basecraft_io::hash_file(&path).map_err(|e| e.to_string())? != artifact.digest {
        return Err("Cached caller evidence changed; recompute calls".into());
    }
    let mut draft = SangerDraft::read(&path)?;
    if draft.inputs.len() != 1
        || draft.engine != "Tracy"
        || draft.inputs[0].sha256 != identity.sha256
        || draft.engine_sha256
            != identity.parameters["engine_sha256"]
                .as_str()
                .unwrap_or_default()
        || draft.signal_threshold.map(u64::from) != identity.parameters["signal_threshold"].as_u64()
        || draft.minimum_overlap != 50
        || draft.minimum_identity != 0.8
        || basecraft_io::sha256_file(&identity.source).map_err(|e| e.to_string())?
            != identity.sha256
    {
        return Err(
            "Source or executed caller identity changed; instrument review preserved".into(),
        );
    }
    // Runtime outputs live under the owned run's artifacts collection.
    let run = path
        .ancestors()
        .find(|ancestor| ancestor.join("runtime.json").is_file())
        .ok_or("Caller receipt directory is missing")?
        .to_path_buf();
    draft.inputs[0].path.clone_from(&identity.source);
    draft.limitations.push(format!(
        "Caller package {}; contract {}; effective settings {}",
        identity.pins.package_digest, identity.pins.descriptor_digest, identity.parameters
    ));
    draft.validate()?;
    Ok(CalledDraft { draft, run })
}

impl SangerCaller {
    /// Returns a verified cached result, or starts a session-owned background job.
    pub fn start(
        &mut self,
        source: &Path,
        sha256: &str,
        signal_threshold: u32,
    ) -> Result<Option<CalledDraft>, String> {
        if !(1..=90).contains(&signal_threshold) {
            return Err("Signal cutoff must be between 1 and 90".into());
        }
        self.cancel()?;
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        if basecraft_io::sha256_file(&source).map_err(|e| e.to_string())? != sha256 {
            return Err("AB1 changed since opening; reopen it before calling".into());
        }
        let registry = Registry::load(&RegistryConfig::discover().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let operation = registry
            .operations
            .get(OPERATION)
            .ok_or("Install the Tracy Sanger plugin to compute alternative calls")?;
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
        let identity = Identity {
            source: source.clone(),
            sha256: sha256.into(),
            parameters: parameters.clone(),
            pins: operation.pins(&registry.generation),
        };
        let root = source.parent().ok_or("AB1 has no parent folder")?;
        let store = root.join(".basecraft/analyses-v1");
        if basecraft_ops::analysis_store::ArtifactRun::validate_store(&store).is_ok() {
            // Reuse the shared run receipts; no second cache/index format.
            if let Ok(runs) = std::fs::read_dir(store.join("runs")) {
                for entry in runs.flatten().take(4096) {
                    let receipt = entry.path().join("runtime.json");
                    let Ok(bytes) = basecraft_ops::provenance::read_record(&receipt) else {
                        continue;
                    };
                    let Ok(outcome) = serde_json::from_slice::<OperationOutcome>(&bytes) else {
                        continue;
                    };
                    if let Ok(calls) = checked(&outcome, &identity) {
                        return Ok(Some(calls));
                    }
                }
            }
        }
        let request: OperationRequest = serde_json::from_value(json!({"operation":OPERATION,
            "inputs":{"reads":[{"kind":"file","path":source}]},"parameters":parameters,
            "pins":identity.pins}))
        .map_err(|e| e.to_string())?;
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
        checked(&outcome, &identity).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_requires_complete_matching_receipt_and_unchanged_input_and_artifact() {
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
            source: source.clone(),
            sha256: basecraft_io::sha256_file(&source).unwrap(),
            parameters: parameters.clone(),
            pins: pins.clone(),
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
        let run = tmp.path().join("run");
        std::fs::create_dir(&run).unwrap();
        std::fs::write(run.join("runtime.json"), "{}").unwrap();
        let path = run.join("evidence.json");
        std::fs::write(&path, serde_json::to_vec(&draft).unwrap()).unwrap();
        let mut outcome = OperationOutcome {
            schema: 1,
            operation: OPERATION.into(),
            run_id: "run".into(),
            state: RuntimeState::Completed,
            pins,
            requested_parameters: parameters.clone(),
            effective_parameters: parameters,
            data: None,
            artifacts: vec![basecraft_protocol::operation::PublishedArtifact {
                role: "artifacts".into(),
                path: path.display().to_string(),
                digest: basecraft_io::hash_file(&path).unwrap(),
            }],
            proposals: None,
            warnings: vec![],
            errors: vec![],
            evidence: None,
        };
        assert!(checked(&outcome, &identity).is_ok());
        outcome.state = RuntimeState::Cancelled;
        assert!(checked(&outcome, &identity).is_err());
        outcome.state = RuntimeState::Completed;
        outcome.effective_parameters["signal_threshold"] = 21.into();
        assert!(checked(&outcome, &identity).is_err());
        outcome.effective_parameters = identity.parameters.clone();
        outcome.pins.package_digest = "replaced".into();
        assert!(checked(&outcome, &identity).is_err());
        outcome.pins = identity.pins.clone();
        let original = std::fs::read(&source).unwrap();
        std::fs::write(&source, "changed source at same pathname").unwrap();
        assert!(checked(&outcome, &identity).is_err());
        std::fs::write(&source, original).unwrap();
        std::fs::write(&path, "damaged evidence").unwrap();
        assert!(checked(&outcome, &identity).is_err());
    }
}
