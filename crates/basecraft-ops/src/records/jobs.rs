//! The Tasks view's job index: one small record per run in the `.basecraft`
//! of the run's home (its project's root, or the folder it ran in outside a
//! project), each pointing at the run's own `runtime.json`, which stays
//! authoritative. Reading creates no folders.
use basecraft_protocol::provenance::Publication;
use std::path::{Path, PathBuf};

/// Where the Tasks view's record of job `id` lives, for a run whose records
/// belong to `home`: a project root, or the folder of a loose file.
pub fn job_index_path(home: &Path, id: &str) -> PathBuf {
    home.join(".basecraft/jobs").join(id).join("manifest.json")
}

/// The job records kept in the `.basecraft` of each of `homes`, whatever tabs
/// are open. Reading creates no directories.
pub fn jobs(previous: bool, homes: &[PathBuf]) -> crate::Result<serde_json::Value> {
    let mut folders: Vec<_> = homes
        .iter()
        .filter_map(|home| home.canonicalize().ok())
        .map(|home| home.join(".basecraft/jobs"))
        .collect();
    folders.sort();
    folders.dedup();
    let folders: Vec<_> = folders.iter().map(PathBuf::as_path).collect();
    job_records(&folders, previous, basecraft_platform::process_instance())
}

fn job_records(folders: &[&Path], previous: bool, owner: &str) -> crate::Result<serde_json::Value> {
    let mut runs = Vec::new();
    let mut budget = TaskReadBudget(64 << 20);
    let mut warnings = Vec::new();
    let mut listings = Vec::new();
    for &folder in folders {
        // The folder is `<home>/.basecraft/jobs`; its records' paths are
        // relative to the home.
        let home = folder.parent().and_then(Path::parent).unwrap_or(folder);
        match std::fs::read_dir(folder) {
            Ok(listing) => listings.push(listing.map(move |entry| (home, entry))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    for (index, (home, entry)) in listings.into_iter().flatten().enumerate() {
        if index >= 4096 {
            warnings.push("Showing at most 4096 retained runs".into());
            break;
        }
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("manifest.json");
        let result = budget.read(&path).and_then(|b| {
            basecraft_protocol::records::decode::<serde_json::Value>(&b).map_err(Into::into)
        });
        match result {
            Ok(mut run) => {
                if !run.is_object()
                    || run["schema_version"] != 1
                    || !run["id"].is_string()
                    || !run["status"].is_string()
                    || !run["operation"].is_string()
                    || !run["source"]["path"]
                        .as_str()
                        .is_some_and(|p| Path::new(p).is_relative())
                    || !run["outputs"].is_object()
                {
                    warnings.push(format!(
                        "Unsupported or malformed task record: {}",
                        path.display()
                    ));
                    continue;
                }
                let current = run["owner_instance"].as_str() == Some(owner);
                if !previous && !current {
                    continue;
                }
                let source = home.join(run["source"]["path"].as_str().unwrap_or_default());
                run["source"]["path"] = serde_json::json!(source);
                // A Sanger stage's own pointers; one outside the home is dropped.
                for field in ["primary_output", "artifact_run"] {
                    if let Some(path) = run.get(field).and_then(serde_json::Value::as_str) {
                        run[field] = serde_json::json!(inside(home, path));
                    }
                }
                if run.get("runtime_record").is_some()
                    && let Err(error) = reconcile_task_receipt(&mut run, home, &mut budget)
                {
                    let notice = format!(
                        "Canonical task receipt unavailable or unsupported for {}: {error}",
                        path.display()
                    );
                    run["status"] = "Completion unconfirmed; receipt unavailable".into();
                    run["outputs"] = serde_json::Value::Object(serde_json::Map::new());
                    run["artifacts"] = serde_json::Value::Array(Vec::new());
                    run["errors"] = serde_json::Value::Array(Vec::new());
                    run["warnings"] = serde_json::Value::Array(Vec::new());
                    run["effective_parameters"] = serde_json::Value::Null;
                    run["primary_output"] = serde_json::Value::Null;
                    run["diagnostics"] = serde_json::json!(notice);
                    run["receipt_confirmed"] = false.into();
                    warnings.push(notice);
                }
                if matches!(
                    run["status"].as_str(),
                    Some("queued" | "running" | "cancelling" | "finalizing")
                ) && !current
                {
                    // A PID alone is insufficient to prove a previous owner is still alive.
                    run["status"] = "Previous owner; completion unconfirmed".into();
                }
                if let Some(object) = run.as_object_mut() {
                    object.remove("source_record");
                }
                run["record_path"] = serde_json::json!(path);
                runs.push(run);
            }
            Err(e) => warnings.push(format!("Could not read {}: {e}", path.display())),
        }
        if budget.0 == 0 {
            warnings.push("Retained task display reached its 64 MiB reading limit, including canonical receipts. Evidence remains available at its recorded location.".into());
            break;
        }
    }
    runs.sort_by(|a, b| b["started"].as_str().cmp(&a["started"].as_str()));
    Ok(serde_json::json!({"jobs":runs, "warnings":warnings}))
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

/// A path a job record names, relative to its home and inside it.
fn inside(home: &Path, path: &str) -> Option<PathBuf> {
    normal_task_path(Path::new(path)).then(|| home.join(path))
}

fn reconcile_task_receipt(
    run: &mut serde_json::Value,
    home: &Path,
    budget: &mut TaskReadBudget,
) -> crate::Result<()> {
    use basecraft_protocol::operation::OperationOutcome;
    // The pointer is relative to the record's home, so a copied project reads
    // its own receipt. Listing never searches for or rewrites evidence.
    let path = run["runtime_record"]
        .as_str()
        .and_then(|path| inside(home, path))
        .ok_or_else(|| {
            crate::error::invalid("runtime receipt pointer must be a path inside the record's home")
        })?;
    run["runtime_record"] = serde_json::json!(path);
    let path = path.as_path();
    let (outcome, writer) =
        basecraft_protocol::records::decode_written::<OperationOutcome>(&budget.read(path)?)?;
    if outcome.schema != 1 {
        return Err(crate::error::invalid(
            basecraft_protocol::records::other_format(&writer, outcome.schema, 1),
        ));
    }
    if run["id"] != outcome.run_id || run["operation"] != outcome.operation {
        return Err(crate::error::invalid("operation receipt identity mismatch"));
    }
    let primary = task_primary_output(&outcome, path, budget);
    run["status"] = serde_json::to_value(outcome.state)?;
    run["effective_parameters"] = outcome.effective_parameters;
    run["outputs"] = serde_json::to_value(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), artifact.digest.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
    )?;
    run["artifacts"] = serde_json::to_value(&outcome.artifacts)?;
    run["primary_output"] =
        serde_json::json!(primary.as_ref().ok().and_then(|path| path.as_deref()));
    run["diagnostics"] = serde_json::json!(
        outcome
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .chain(outcome.warnings.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    );
    run["errors"] = serde_json::to_value(outcome.errors)?;
    run["warnings"] = serde_json::to_value(outcome.warnings)?;
    if let Err(error) = primary {
        let notice = format!("Primary result unavailable: {error}");
        run["diagnostics"] = serde_json::json!(
            format!(
                "{}\n{notice}",
                run["diagnostics"].as_str().unwrap_or_default()
            )
            .trim()
        );
    }
    run["receipt_confirmed"] = true.into();
    Ok(())
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

    fn registered_fixture(
        root: &Path,
        state: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf, serde_json::Value) {
        let jobs = root.join(".basecraft/jobs");
        let receipt = root.join("run/runtime.json");
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        std::fs::create_dir_all(jobs.join("registered")).unwrap();
        let outcome = serde_json::json!({"schema":1,"run_id":"registered","operation":"test/run@1","state":state,
            "pins":{"generation":"g","package_digest":"p","descriptor_digest":"d"},
            "requested_parameters":{},"effective_parameters":{"canonical":true},"data":null,
            "artifacts":[],"proposals":null,"warnings":["canonical warning"],
            "errors":[{"code":"tool_failure","message":"canonical error","operation":"test/run@1"}],"evidence":null});
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        let index = serde_json::json!({"schema_version":1,"id":"registered","status":"completed","operation":"test/run@1",
            "owner_instance":"current","source":{"path":"input.afa"},"outputs":{"stale.nwk":"stale"},
            "primary_output":"stale.nwk","diagnostics":"stale error","runtime_record":"run/runtime.json"});
        std::fs::write(
            jobs.join("registered/manifest.json"),
            basecraft_protocol::records::encode(&index).unwrap(),
        )
        .unwrap();
        (jobs, receipt, outcome)
    }

    #[test]
    fn task_receipts_override_stale_index_for_every_state_and_previous_owner() {
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
            let (jobs, receipt, outcome) = registered_fixture(temp.path(), state);
            let index = jobs.join("registered/manifest.json");
            let before = std::fs::read(&index).unwrap();
            let rows = job_records(&[&jobs], false, "current").unwrap();
            let row = &rows["jobs"][0];
            assert_eq!(row["status"], state);
            assert_eq!(row["outputs"], serde_json::json!({}));
            assert!(row["primary_output"].is_null());
            assert_eq!(row["diagnostics"], "canonical error\ncanonical warning");
            assert_eq!(row["effective_parameters"]["canonical"], true);
            assert_eq!(row["receipt_confirmed"], true);
            assert_eq!(
                job_records(&[&jobs], false, "new-owner").unwrap()["jobs"],
                serde_json::json!([])
            );
            let previous = job_records(&[&jobs], true, "new-owner").unwrap();
            if ["queued", "running", "cancelling", "finalizing"].contains(&state) {
                assert_eq!(
                    previous["jobs"][0]["status"],
                    "Previous owner; completion unconfirmed"
                );
            } else {
                assert_eq!(previous["jobs"][0]["status"], state);
            }
            assert_eq!(std::fs::read(index).unwrap(), before);
            assert_eq!(
                std::fs::read(receipt).unwrap(),
                basecraft_protocol::records::encode(&outcome).unwrap()
            );
        }
    }

    #[test]
    fn task_receipts_refuse_missing_moved_corrupt_future_and_mismatched_evidence() {
        for damage in [
            "missing",
            "moved",
            "corrupt",
            "future",
            "run",
            "operation",
            "bad-pointer",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
            match damage {
                "missing" => std::fs::remove_file(&receipt).unwrap(),
                "moved" => std::fs::rename(&receipt, receipt.with_extension("moved")).unwrap(),
                "corrupt" => std::fs::write(&receipt, "{\"schema\":1,").unwrap(),
                "bad-pointer" => {
                    let index = jobs.join("registered/manifest.json");
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
                    value["runtime_record"] = serde_json::json!(42);
                    std::fs::write(index, basecraft_protocol::records::encode(&value).unwrap())
                        .unwrap();
                }
                field => {
                    match field {
                        "future" => outcome["schema"] = serde_json::json!(999),
                        "run" => outcome["run_id"] = serde_json::json!("other"),
                        _ => outcome["operation"] = serde_json::json!("other"),
                    }
                    std::fs::write(
                        &receipt,
                        basecraft_protocol::records::encode(&outcome).unwrap(),
                    )
                    .unwrap();
                }
            }
            let rows = job_records(&[&jobs], false, "current").unwrap();
            let row = &rows["jobs"][0];
            assert_eq!(
                row["status"], "Completion unconfirmed; receipt unavailable",
                "{damage}"
            );
            assert_eq!(row["receipt_confirmed"], false);
            assert!(row["primary_output"].is_null());
            assert_eq!(row["outputs"], serde_json::json!({}));
            assert_eq!(rows["warnings"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn task_primary_result_uses_real_distance_primary_member_not_bootstrap_order() {
        use basecraft_protocol::operation::{ArtifactVisibility, OperationContract};
        let temp = tempfile::tempdir().unwrap();
        let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
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
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"],
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
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        outcome["state"] = serde_json::json!("finalizing");
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert!(
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
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
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"],
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
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        // Missing primary must not substitute the bootstrap collection.
        outcome["artifacts"].as_array_mut().unwrap().remove(1);
        std::fs::write(
            &receipt,
            basecraft_protocol::records::encode(&outcome).unwrap(),
        )
        .unwrap();
        assert!(
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
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
        assert!(
            job_records(&[&jobs], false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
        descriptor.outputs.get_mut("artifacts").unwrap().visibility = ArtifactVisibility::Result;
        std::fs::write(
            &descriptor_path,
            basecraft_protocol::records::encode(&descriptor).unwrap(),
        )
        .unwrap();
        let row = &job_records(&[&jobs], false, "current").unwrap()["jobs"][0];
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
    fn task_nonregular_evidence_is_rejected_in_bounded_subprocess() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "BASECRAFT_TASK_FIFO_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            let rows = job_records(
                &[&Path::new(&root).join(".basecraft/jobs")],
                false,
                "current",
            )
            .unwrap();
            let text = rows.to_string();
            assert!(text.contains("not a regular file"), "{text}");
            assert!(!text.contains("reading limit"), "{text}");
            assert!(
                rows["jobs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == "regular")
            );
            return;
        }
        for location in ["index", "runtime", "publication"] {
            let temp = tempfile::tempdir().unwrap();
            let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
            let path = match location {
                "index" => jobs.join("registered/manifest.json"),
                "runtime" => receipt.clone(),
                _ => {
                    outcome["artifacts"] = serde_json::json!([{ "role":"artifacts","path":"/tree.nwk","digest":"hash"}]);
                    std::fs::write(
                        &receipt,
                        basecraft_protocol::records::encode(&outcome).unwrap(),
                    )
                    .unwrap();
                    receipt.parent().unwrap().join("manifest.json")
                }
            };
            if path.exists() {
                std::fs::remove_file(&path).unwrap();
            }
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
            std::fs::create_dir_all(jobs.join("regular")).unwrap();
            std::fs::write(jobs.join("regular/manifest.json"), basecraft_protocol::records::encode(&serde_json::json!({"schema_version":1,"id":"regular","operation":"legacy","status":"completed","source":{"path":"input"},"outputs":{},"owner_instance":"current"})).unwrap()).unwrap();
            let test = crate::isolated_profile::test_name();
            let mut child = crate::isolated_profile::relaunch(&test)
                .env(CHILD, temp.path())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if child.try_wait().unwrap().is_some() {
                    let output = child.wait_with_output().unwrap();
                    crate::isolated_profile::assert_one_passed(&test, &output);
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("{location} FIFO hung the retained Tasks query");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let regular = temp.path().join("regular.json");
        let link = temp.path().join("regular-link.json");
        std::fs::write(&regular, "{}").unwrap();
        std::os::unix::fs::symlink(&regular, &link).unwrap();
        assert_eq!(
            crate::records::provenance::read_record(&link).unwrap(),
            b"{}"
        );
        let mut budget = TaskReadBudget(1024);
        assert!(budget.read(temp.path()).is_err());
        assert_eq!(budget.0, 1024);
        let mut permissions = std::fs::metadata(&regular).unwrap().permissions();
        permissions.set_mode(0o0);
        std::fs::set_permissions(&regular, permissions).unwrap();
        // Root may bypass permissions; normal validation runs as an unprivileged user.
        if std::fs::File::open(&regular).is_err() {
            assert!(budget.read(&regular).is_err());
            assert_eq!(budget.0, 1024);
        }
    }

    #[test]
    fn registered_legacy_and_sanger_tasks_coexist_without_listing_writes() {
        let temp = tempfile::tempdir().unwrap();
        let (jobs, _, _) = registered_fixture(temp.path(), "partial");
        for (id, primary) in [
            ("legacy", serde_json::Value::Null),
            ("sanger", serde_json::json!("called.fa")),
        ] {
            let directory = jobs.join(id);
            std::fs::create_dir_all(&directory).unwrap();
            let value = serde_json::json!({"schema_version":1,"id":id,"status":"completed","operation":id,
                "source":{"path":"input"},"outputs":{},"primary_output":primary,"owner_instance":"current"});
            std::fs::write(
                directory.join("manifest.json"),
                basecraft_protocol::records::encode(&value).unwrap(),
            )
            .unwrap();
        }
        let first = job_records(&[&jobs], false, "current").unwrap();
        let second = job_records(&[&jobs], true, "current").unwrap();
        assert_eq!(first, second);
        let rows = first["jobs"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().filter(|row| row["id"] == "registered").count(),
            1
        );
        assert_eq!(
            rows.iter().find(|row| row["id"] == "sanger").unwrap()["primary_output"],
            serde_json::json!(temp.path().join("called.fa"))
        );
        assert_eq!(std::fs::read_dir(&jobs).unwrap().count(), 3);
        assert_eq!(job_records(&[&jobs], false, "current").unwrap(), first);
    }

    #[test]
    fn task_read_budget_includes_receipts_and_caps_each_file() {
        let temp = tempfile::tempdir().unwrap();
        let (jobs, receipt, _) = registered_fixture(temp.path(), "completed");
        let index = jobs.join("registered/manifest.json");
        let size = std::fs::metadata(&index).unwrap().len();
        let mut budget = TaskReadBudget(size + 1);
        budget.read(&index).unwrap();
        assert!(budget.read(&receipt).is_err());
        assert_eq!(budget.0, 0);
        let file = std::fs::File::create(&receipt).unwrap();
        file.set_len((64 << 20) + 1).unwrap();
        assert!(crate::records::provenance::read_record(&receipt).is_err());
        let rows = job_records(&[&jobs], false, "current").unwrap();
        assert_eq!(rows["jobs"][0]["receipt_confirmed"], false);
        assert!(
            rows["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("64 MiB"))
        );
    }

    #[test]
    fn a_moved_home_reads_its_own_receipt_and_absolute_pointers_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let (_, receipt, _) = registered_fixture(&temp.path().join("a"), "completed");
        let moved = temp.path().join("b");
        std::fs::rename(temp.path().join("a"), &moved).unwrap();
        let jobs = moved.join(".basecraft/jobs");
        let row = &job_records(&[&jobs], false, "current").unwrap()["jobs"][0];
        assert_eq!(row["receipt_confirmed"], true, "{row}");
        assert_eq!(
            row["runtime_record"],
            serde_json::json!(moved.join("run/runtime.json"))
        );
        assert_eq!(
            row["source"]["path"],
            serde_json::json!(moved.join("input.afa"))
        );
        let index = jobs.join("registered/manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
        value["runtime_record"] = serde_json::json!(receipt);
        std::fs::write(&index, value.to_string()).unwrap();
        let row = &job_records(&[&jobs], false, "current").unwrap()["jobs"][0];
        assert_eq!(row["receipt_confirmed"], false, "{row}");
        value["runtime_record"] = serde_json::json!("../a/run/runtime.json");
        value["source"]["path"] = serde_json::json!(moved.join("input.afa"));
        std::fs::write(&index, value.to_string()).unwrap();
        let rows = job_records(&[&jobs], false, "current").unwrap();
        assert_eq!(rows["jobs"], serde_json::json!([]), "{rows}");
    }

    #[test]
    fn task_records_handle_missing_malformed_and_previous_owners() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = dir.path().join(".basecraft/jobs");
        assert_eq!(
            job_records(&[&jobs], false, "current").unwrap()["jobs"],
            serde_json::json!([])
        );
        assert!(!jobs.exists());
        for (id, value) in [
            ("broken", serde_json::json!([])),
            (
                "current",
                serde_json::json!({"schema_version":1,"id":"current","status":"running","operation":"align","source":{"path":"a.fasta"},"outputs":{},"owner_instance":"current"}),
            ),
            (
                "previous",
                serde_json::json!({"schema_version":1,"id":"previous","status":"running","operation":"align","source":{"path":"a.fasta"},"outputs":{},"owner_instance":"previous"}),
            ),
        ] {
            std::fs::create_dir_all(jobs.join(id)).unwrap();
            // A list can't carry the stamp: it is a broken record either way.
            let bytes = basecraft_protocol::records::encode(&value)
                .unwrap_or_else(|_| value.to_string().into_bytes());
            std::fs::write(jobs.join(id).join("manifest.json"), bytes).unwrap();
        }
        let current = job_records(&[&jobs], false, "current").unwrap();
        assert_eq!(current["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(current["jobs"][0]["status"], "running");
        assert_eq!(current["warnings"].as_array().unwrap().len(), 1);
        let all = job_records(&[&jobs], true, "current").unwrap();
        assert_eq!(all["jobs"].as_array().unwrap().len(), 2);
        assert!(
            all["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|j| j["id"] == "previous"
                    && j["status"] == "Previous owner; completion unconfirmed")
        );
    }
}
