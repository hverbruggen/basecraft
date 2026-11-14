//! The Tasks view's list of runs (D28): each run's own `runtime.json` in
//! the analysis store of each home, a project's root or the folder of a
//! loose file. A run's `execution.json` says which process ran it and on
//! what. Reading creates no folders.
use basecraft_protocol::operation::OperationOutcome;
use basecraft_protocol::provenance::Publication;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One row of the Tasks view.
#[derive(Debug, Serialize)]
struct Task {
    id: String,
    operation: String,
    status: String,
    source: TaskSource,
    outputs: BTreeMap<String, String>,
    primary_output: Option<String>,
    diagnostics: String,
    effective_parameters: serde_json::Value,
    record_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct TaskSource {
    path: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize)]
struct Tasks {
    jobs: Vec<Task>,
    warnings: Vec<String>,
}

/// The runs kept in the `.basecraft` of each of `homes`, whatever tabs are
/// open, newest first. Without `previous`, only this process's runs.
pub fn jobs(previous: bool, homes: &[PathBuf]) -> crate::Result<serde_json::Value> {
    let mut folders: Vec<_> = homes
        .iter()
        .filter_map(|home| home.canonicalize().ok())
        .map(|home| home.join(".basecraft/analyses-v1/runs"))
        .collect();
    folders.sort();
    folders.dedup();
    let folders: Vec<_> = folders.iter().map(PathBuf::as_path).collect();
    Ok(serde_json::to_value(job_records(
        &folders,
        previous,
        basecraft_platform::process_instance(),
    ))?)
}

fn job_records(folders: &[&Path], previous: bool, owner: &str) -> Tasks {
    let mut tasks = Tasks::default();
    let mut budget = TaskReadBudget(64 << 20);
    let mut seen = 0;
    'folders: for &folder in folders {
        let Ok(listing) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in listing.flatten() {
            let run = entry.path();
            let runtime = run.join("runtime.json");
            // Input ledgers and other records hold no runtime record.
            if !runtime.is_file() {
                continue;
            }
            seen += 1;
            if seen > 4096 {
                tasks
                    .warnings
                    .push("Showing at most 4096 retained runs".into());
                break 'folders;
            }
            match task(&run, &runtime, previous, owner, &mut budget) {
                Ok(Some(task)) => tasks.jobs.push(task),
                Ok(None) => {}
                Err(error) => tasks.warnings.push(format!(
                    "Could not read the run record {}: {error}",
                    runtime.display()
                )),
            }
            if budget.0 == 0 {
                tasks.warnings.push("Retained task display reached its 64 MiB reading limit. Evidence remains available at its recorded location.".into());
                break 'folders;
            }
        }
    }
    // Run identities are ULIDs, so this is newest first.
    tasks.jobs.sort_by(|a, b| b.id.cmp(&a.id));
    tasks
}

/// The row for the run in `run`, unless it belongs to another process and
/// only this one's are wanted.
fn task(
    run: &Path,
    runtime: &Path,
    previous: bool,
    owner: &str,
    budget: &mut TaskReadBudget,
) -> crate::Result<Option<Task>> {
    let (outcome, writer) =
        basecraft_protocol::records::decode_written::<OperationOutcome>(&budget.read(runtime)?)?;
    if outcome.schema != 1 {
        return Err(crate::error::invalid(
            basecraft_protocol::records::other_format(&writer, outcome.schema, 1),
        ));
    }
    let execution = budget
        .read(&run.join("execution.json"))
        .ok()
        .and_then(|bytes| {
            basecraft_protocol::records::decode::<basecraft_protocol::records::RunRecord>(&bytes)
                .ok()
        });
    let current = execution
        .as_ref()
        .is_some_and(|execution| execution.process_instance == owner);
    if !previous && !current {
        return Ok(None);
    }
    let source = execution.as_ref().and_then(|execution| {
        execution
            .inputs
            .values()
            .flatten()
            .find_map(|input| input.original_path.as_deref())
            .map(|path| run.join(path))
    });
    let primary = task_primary_output(&outcome, runtime, budget);
    let mut status = serde_json::to_value(outcome.state)?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if !outcome.state.terminal() && !current {
        // A PID alone is insufficient to prove a previous owner is still alive.
        status = "Previous owner; completion unconfirmed".into();
    }
    let mut diagnostics: Vec<_> = outcome
        .errors
        .iter()
        .map(|error| error.message.clone())
        .chain(outcome.warnings.iter().cloned())
        .collect();
    if let Err(error) = &primary {
        diagnostics.push(format!("Primary result unavailable: {error}"));
    }
    Ok(Some(Task {
        id: outcome.run_id.clone(),
        operation: outcome.operation.clone(),
        status,
        source: TaskSource { path: source },
        outputs: outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), artifact.digest.clone()))
            .collect(),
        primary_output: primary.ok().flatten(),
        diagnostics: diagnostics.join("\n"),
        effective_parameters: outcome.effective_parameters,
        record_path: runtime.to_path_buf(),
    }))
}

/// Charge both discovery and canonical evidence, including malformed JSON. The
/// bounded reader also caps a file that grows after metadata was inspected.
struct TaskReadBudget(u64);

impl TaskReadBudget {
    fn read(&mut self, path: &Path) -> crate::Result<Vec<u8>> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(crate::error::invalid("evidence is not a regular file"));
        }
        let size = metadata.len();
        if size > self.0 {
            self.0 = 0;
            return Err(crate::error::invalid(
                "retained task aggregate reading limit (64 MiB) exceeded",
            ));
        }
        crate::records::provenance::read_record_with_budget(path, &mut self.0)
    }
}

/// Publication outputs are ordered by the runner with exported results first.
/// Only offer a document that is BOTH a canonical published artifact and a
/// visible publication output, never an arbitrary retained support member.
fn task_primary_output(
    outcome: &basecraft_protocol::operation::OperationOutcome,
    runtime: &Path,
    budget: &mut TaskReadBudget,
) -> crate::Result<Option<String>> {
    if !outcome.state.terminal() || outcome.artifacts.is_empty() {
        return Ok(None);
    }
    let root = runtime
        .parent()
        .ok_or_else(|| crate::error::invalid("receipt has no directory"))?;
    // manifest.json names the run's latest record (D24).
    let latest: basecraft_protocol::provenance::LatestRecord =
        basecraft_protocol::records::decode(&budget.read(&root.join("manifest.json"))?)?;
    let record = budget.read(&crate::records::provenance::record_path(root, &latest.id)?)?;
    if basecraft_io::hash_bytes(&record) != latest.hash {
        return Err(crate::error::invalid("the run's latest record changed"));
    }
    let (publication, writer) =
        basecraft_protocol::records::decode_written::<Publication>(&record)?;
    if publication.schema_version != 1 {
        return Err(crate::error::invalid(
            basecraft_protocol::records::other_format(&writer, publication.schema_version, 1),
        ));
    }
    if publication.id != outcome.run_id {
        return Err(crate::error::invalid("publication run identity mismatch"));
    }
    // Use the accepted descriptor's exact primary role/member, not collection
    // ordering. A result collection can include primary and supporting trees.
    let descriptor: basecraft_protocol::operation::OperationDescriptor =
        basecraft_protocol::records::decode(&budget.read(&root.join("descriptor.json"))?)?;
    if basecraft_io::hash_bytes(&serde_json::to_vec(&descriptor)?) != outcome.pins.descriptor_digest
    {
        return Err(crate::error::invalid(
            "primary result descriptor identity mismatch",
        ));
    }
    let primary = descriptor
        .outputs
        .iter()
        .find_map(|(role, output)| output.primary.as_ref().map(|member| (role, member)));
    for retained_only in [false, true] {
        for output in &publication.outputs {
            let logical = Path::new(&output.logical_name);
            if !normal_task_path(logical) || crate::files::Format::from_path(logical).is_err() {
                continue;
            }
            if let Some(artifact) = outcome.artifacts.iter().find(|artifact| {
                use basecraft_protocol::operation::ArtifactVisibility;
                let path = Path::new(&artifact.path);
                if artifact.digest != output.hash || !path.is_absolute() {
                    return false;
                }
                let binding = descriptor.outputs.get(&artifact.role);
                let result_role =
                    binding.is_some_and(|binding| binding.visibility == ArtifactVisibility::Result);
                let visible = !path.starts_with(root)
                    && path.ends_with(logical)
                    && !logical
                        .components()
                        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'));
                let retained = output
                    .retained
                    .as_deref()
                    .map(Path::new)
                    .filter(|path| normal_task_path(path));
                let matches_primary = primary.is_some_and(|(role, member)| {
                    role == &artifact.role
                        && normal_task_path(Path::new(member))
                        && retained.is_some_and(|retained| retained == Path::new(role).join(member))
                });
                if !retained_only {
                    return visible
                        && (artifact.role == "workspace"
                            || (result_role && (primary.is_none() || matches_primary)));
                }
                result_role
                    && matches_primary
                    && outcome.state == basecraft_protocol::operation::RuntimeState::Completed
                    && retained.is_some_and(|retained| root.join(retained) == path)
            }) {
                return Ok(Some(artifact.path.clone()));
            }
        }
    }
    Ok(None)
}

fn normal_task_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, value: &serde_json::Value) {
        std::fs::write(path, basecraft_protocol::records::encode(value).unwrap()).unwrap();
    }

    /// The run record of a process `owner`, on `input.afa` in the home.
    fn execution(owner: &str) -> serde_json::Value {
        let record = basecraft_protocol::records::RunRecord {
            process_instance: owner.into(),
            inputs: BTreeMap::from([(
                "alignment".into(),
                vec![basecraft_protocol::operation::InvocationInputEvidence {
                    original_path: Some("../../../../input.afa".into()),
                    source_hash: None,
                    submitted_hash: "b3:x".into(),
                    transformation: "identity-v1".into(),
                    run_directory: ".".into(),
                }],
            )]),
            ..basecraft_protocol::records::RunRecord::new(
                "test/run@1".into(),
                "1".into(),
                "t".into(),
            )
        };
        serde_json::to_value(record).unwrap()
    }

    /// A run in the store of `home`, in `state`, made by process "current".
    fn run_fixture(home: &Path, state: &str) -> (PathBuf, PathBuf, serde_json::Value) {
        let runs = home.join(".basecraft/analyses-v1/runs");
        let receipt = runs.join("registered/runtime.json");
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        let outcome = serde_json::json!({"schema":1,"run_id":"registered","operation":"test/run@1","state":state,
            "pins":{"generation":"g","package_digest":"p","descriptor_digest":"d"},
            "requested_parameters":{},"effective_parameters":{"canonical":true},"data":null,
            "artifacts":[],"proposals":null,"warnings":["canonical warning"],
            "errors":[{"code":"tool_failure","message":"canonical error","operation":"test/run@1"}],"evidence":null});
        write(&receipt, &outcome);
        write(
            &receipt.with_file_name("execution.json"),
            &execution("current"),
        );
        (runs, receipt, outcome)
    }

    fn rows(runs: &Path, previous: bool, owner: &str) -> serde_json::Value {
        serde_json::to_value(job_records(&[runs], previous, owner)).unwrap()
    }

    #[test]
    fn tasks_list_each_runs_own_record_for_every_state_and_owner() {
        for state in [
            "queued",
            "running",
            "cancelling",
            "finalizing",
            "completed",
            "partial",
            "failed",
            "cancelled",
            "interrupted",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (runs, receipt, _) = run_fixture(temp.path(), state);
            let before = std::fs::read(&receipt).unwrap();
            let listed = rows(&runs, false, "current");
            let row = &listed["jobs"][0];
            assert_eq!(row["status"], state);
            assert_eq!(row["outputs"], serde_json::json!({}));
            assert!(row["primary_output"].is_null());
            assert_eq!(row["diagnostics"], "canonical error\ncanonical warning");
            assert_eq!(row["effective_parameters"]["canonical"], true);
            assert_eq!(
                row["source"]["path"],
                serde_json::json!(runs.join("registered/../../../../input.afa"))
            );
            assert_eq!(
                rows(&runs, false, "new-owner")["jobs"],
                serde_json::json!([])
            );
            let previous = rows(&runs, true, "new-owner");
            if ["queued", "running", "cancelling", "finalizing"].contains(&state) {
                assert_eq!(
                    previous["jobs"][0]["status"],
                    "Previous owner; completion unconfirmed"
                );
            } else {
                assert_eq!(previous["jobs"][0]["status"], state);
            }
            // Listing writes nothing.
            assert_eq!(std::fs::read(receipt).unwrap(), before);
        }
    }

    #[test]
    fn a_damaged_run_record_is_a_warning_and_other_records_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            rows(&temp.path().join("absent"), true, "current")["jobs"],
            serde_json::json!([])
        );
        assert!(!temp.path().join("absent").exists());
        let (runs, receipt, mut outcome) = run_fixture(temp.path(), "completed");
        // An input ledger has no runtime record and is no task.
        std::fs::create_dir_all(runs.join("ledger")).unwrap();
        std::fs::write(runs.join("ledger/input.json"), b"{}").unwrap();
        for damage in ["corrupt", "future"] {
            if damage == "corrupt" {
                std::fs::write(&receipt, "{\"schema\":1,").unwrap();
            } else {
                outcome["schema"] = serde_json::json!(999);
                write(&receipt, &outcome);
            }
            let listed = rows(&runs, true, "current");
            assert_eq!(listed["jobs"], serde_json::json!([]), "{damage}");
            assert_eq!(listed["warnings"].as_array().unwrap().len(), 1, "{damage}");
        }
        let file = std::fs::File::create(&receipt).unwrap();
        file.set_len((64 << 20) + 1).unwrap();
        let listed = rows(&runs, true, "current");
        assert!(
            listed["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("64 MiB"))
        );
    }

    #[test]
    fn task_primary_result_uses_real_distance_primary_member_not_bootstrap_order() {
        use basecraft_protocol::operation::{ArtifactVisibility, OperationContract};
        let temp = tempfile::tempdir().unwrap();
        let (runs, receipt, mut outcome) = run_fixture(temp.path(), "completed");
        let root = receipt.parent().unwrap();
        let tree = root.join("artifacts/tree.nwk");
        let bootstrap = root.join("artifacts/bootstrap-run.partial.nex");
        let contract: OperationContract = serde_json::from_slice(include_bytes!(
            "../../../../plugins/distance/operations.json"
        ))
        .unwrap();
        let mut descriptor = contract
            .operations
            .into_iter()
            .find(|descriptor| descriptor.name == "tree")
            .unwrap();
        let mut publication = Publication {
            schema_version: 1,
            id: "registered".into(),
            session: "current".into(),
            parents: vec![],
            parent_hashes: std::collections::BTreeMap::default(),
            created: String::new(),
            origin: String::new(),
            inputs: vec![],
            outputs: [
                ("artifacts/bootstrap-run.partial.nex", "support"),
                ("artifacts/tree.nwk", "canonical"),
            ]
            .into_iter()
            .map(
                |(member, hash)| basecraft_protocol::provenance::ArtifactRevision {
                    id: member.into(),
                    logical_name: format!(".basecraft/run/{member}"),
                    hash: hash.into(),
                    size: 0,
                    retained: Some(member.into()),
                    reference: None,
                },
            )
            .collect(),
            events: vec![],
            effective_edits: vec![],
            withdrawn: Vec::new(),
            completeness: basecraft_protocol::provenance::Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        // Actual SDK ordering: bootstrap first; both members have the SAME
        // artifacts/result role, including withheld partial bootstrap support.
        outcome["artifacts"] = serde_json::json!([
            {"role":"artifacts","path":bootstrap,"digest":"support"},
            {"role":"artifacts","path":tree,"digest":"canonical"}]);
        outcome["pins"]["descriptor_digest"] = serde_json::json!(basecraft_io::hash_bytes(
            &serde_json::to_vec(&descriptor).unwrap()
        ));
        let descriptor_path = root.join("descriptor.json");
        std::fs::write(
            &descriptor_path,
            basecraft_protocol::records::encode(&descriptor).unwrap(),
        )
        .unwrap();
        crate::records::provenance::publish(root, &publication).unwrap();
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rows(&runs, false, "current")["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        outcome["artifacts"][0]["path"] =
            serde_json::json!(root.join("artifacts/bootstrap-run.nex"));
        publication.outputs[0].logical_name = ".basecraft/run/artifacts/bootstrap-run.nex".into();
        publication.outputs[0].retained = Some("artifacts/bootstrap-run.nex".into());
        crate::records::provenance::publish(root, &publication).unwrap();
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rows(&runs, false, "current")["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        outcome["state"] = serde_json::json!("finalizing");
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert!(rows(&runs, false, "current")["jobs"][0]["primary_output"].is_null());
        outcome["state"] = serde_json::json!("completed");
        // A canonical visible export of that exact primary member wins.
        let export = temp.path().join("inferred.nwk");
        publication.outputs.insert(
            0,
            basecraft_protocol::provenance::ArtifactRevision {
                id: "export".into(),
                logical_name: "inferred.nwk".into(),
                hash: "canonical".into(),
                size: 0,
                retained: Some("artifacts/tree.nwk".into()),
                reference: None,
            },
        );
        outcome["artifacts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"role":"artifacts","path":export,"digest":"canonical"}));
        crate::records::provenance::publish(root, &publication).unwrap();
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rows(&runs, false, "current")["jobs"][0]["primary_output"],
            export.to_str().unwrap()
        );
        // Stale export digest cannot override the valid retained primary.
        outcome["artifacts"][2]["digest"] = serde_json::json!("changed");
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rows(&runs, false, "current")["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        // Missing primary must not substitute the bootstrap collection.
        outcome["artifacts"].as_array_mut().unwrap().remove(1);
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert!(rows(&runs, false, "current")["jobs"][0]["primary_output"].is_null());
        // A genuinely supporting-only descriptor never offers a primary.
        descriptor.outputs.get_mut("artifacts").unwrap().visibility =
            ArtifactVisibility::Supporting;
        outcome["artifacts"] =
            serde_json::json!([{ "role":"artifacts","path":tree,"digest":"canonical"}]);
        outcome["pins"]["descriptor_digest"] = serde_json::json!(basecraft_io::hash_bytes(
            &serde_json::to_vec(&descriptor).unwrap()
        ));
        std::fs::write(
            &descriptor_path,
            basecraft_protocol::records::encode(&descriptor).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert!(rows(&runs, false, "current")["jobs"][0]["primary_output"].is_null());
        descriptor.outputs.get_mut("artifacts").unwrap().visibility = ArtifactVisibility::Result;
        std::fs::write(
            &descriptor_path,
            basecraft_protocol::records::encode(&descriptor).unwrap(),
        )
        .unwrap();
        let rows = rows(&runs, false, "current");
        let row = &rows["jobs"][0];
        assert!(row["primary_output"].is_null());
        assert!(
            row["diagnostics"]
                .as_str()
                .unwrap()
                .contains("descriptor identity mismatch")
        );
        assert_eq!(row["status"], "completed");
    }

    #[cfg(unix)]
    #[test]
    fn a_record_that_is_no_regular_file_is_refused_without_opening_it() {
        let temp = tempfile::tempdir().unwrap();
        let (runs, receipt, _) = run_fixture(temp.path(), "completed");
        let fifo = receipt.with_file_name("execution.json");
        std::fs::remove_file(&fifo).unwrap();
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.unwrap().success());
        // Opening a FIFO would block; the budget looks before it opens.
        let mut budget = TaskReadBudget(1024);
        assert!(budget.read(&fifo).is_err());
        assert!(budget.read(temp.path()).is_err());
        assert_eq!(budget.0, 1024);
        // Without its run record a run counts as another process's.
        assert_eq!(rows(&runs, false, "current")["jobs"], serde_json::json!([]));
        assert_eq!(rows(&runs, true, "current")["jobs"][0]["id"], "registered");
    }
}
