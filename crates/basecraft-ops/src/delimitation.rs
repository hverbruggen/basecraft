//! Combining saved hypotheses preserves identities, lineage and exact source evidence.
use crate::{DocId, Result, Session, error::invalid, files};
use basecraft_protocol::{Op, delimitation::DelimitationEdit};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Publish a new canonical exchange file without replacing a user file. The
/// authoritative workspace is hidden; native per-fit SPARTs stay in their runs.
pub fn publish_workspace(
    target: &std::path::Path,
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    run: &crate::analysis_store::ArtifactRun,
) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("SPART filename must be UTF-8"))?;
    let staged = run.staging(name)?;
    basecraft_io::write_atomic(&staged, |w| {
        files::write_delimitation(work, files::delimitation_xml(target), w)
    })?;
    let hash = basecraft_io::hash_file(&staged)?;
    basecraft_io::write_atomic(&run.artifact(&format!("{name}.basecraft.json"))?, |w| {
        files::write_delimitation_sidecar_at(work, &hash, target, w)
    })?;
    run.publish_primary(name)?;
    run.bind(target)
}

/// Explicitly attach a clean file to a species workspace using the common
/// reference lifecycle. Reading a saved workspace never calls this writer.
pub fn reference_input(
    anchor: &std::path::Path,
    source: &std::path::Path,
    hash: &str,
) -> Result<crate::analysis_store::ArtifactRun> {
    reference_input_with_dependencies(anchor, source, hash, &[])
}

fn reference_input_with_dependencies(
    anchor: &std::path::Path,
    source: &std::path::Path,
    hash: &str,
    dependencies: &[PathBuf],
) -> Result<crate::analysis_store::ArtifactRun> {
    crate::input_evidence::reference_input(
        anchor,
        source,
        hash,
        dependencies,
        "Explicit species workspace input reference",
    )
}

/// Retain declared contributing ledgers, not copies of clean original inputs.
pub(crate) fn dependencies(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<Vec<PathBuf>> {
    let mut runs = BTreeSet::new();
    for field in [
        "basecraft_display_evidence_run",
        "basecraft_alignment_evidence_run",
    ] {
        if let Some(run) = work.exchange_fields.get(field) {
            runs.insert(PathBuf::from(run));
        }
    }
    for hypothesis in &work.hypotheses {
        if let Some(report) = hypothesis.provenance.parameters.get("analysis_report") {
            let path = std::path::Path::new(
                report["path"]
                    .as_str()
                    .ok_or_else(|| invalid("Species report has no path"))?,
            );
            if report["hash"].as_str() != Some(basecraft_io::hash_file(path)?.as_str()) {
                return Err(invalid(
                    "Species analysis report changed; saved evidence was not replaced",
                ));
            }
            if let Some(run) = crate::analysis_store::ArtifactRun::for_result(path)? {
                runs.insert(run);
            }
        }
        if let Some(source) = hypothesis.provenance.parameters.get("combined_source") {
            if let Some(run) = source["evidence_run"].as_str() {
                runs.insert(PathBuf::from(run));
            }
        }
    }
    Ok(runs.into_iter().collect())
}

/// Resolve the default alignment only when submitting a job. Passive reopen
/// keeps the original link, while preservation decisions stay with their owner.
pub fn checked_alignment(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    workspace_path: &std::path::Path,
) -> Result<Option<PathBuf>> {
    let Some(path) = work.exchange_fields.get("basecraft_alignment_snapshot") else {
        return Ok(None);
    };
    let hash = work
        .exchange_fields
        .get("basecraft_alignment_hash")
        .ok_or_else(|| invalid("Retained alignment hash is missing"))?;
    if let Some(run) = work.exchange_fields.get("basecraft_alignment_evidence_run") {
        return checked_workspace_input(std::path::Path::new(run), hash).map(Some);
    }
    // Workspaces written before the owning field was introduced still have a
    // bound input ledger (or its Save As delegation / contributing ancestry).
    let bundle = crate::provenance::bundle_for_result(workspace_path);
    if bundle.join("input.json").is_file() {
        let mut candidates = vec![bundle.clone()];
        candidates.extend(
            crate::input_evidence::ancestors(&bundle)?
                .into_iter()
                .map(|(p, _)| p),
        );
        for candidate in candidates {
            if !candidate.join("input.json").is_file() {
                continue;
            }
            let input = crate::input_evidence::read_input(&candidate)?;
            let accepted = input
                .source
                .as_ref()
                .map_or(&input.submitted_hash, |r| &r.hash);
            if accepted == hash {
                return checked_workspace_input(&candidate, hash).map(Some);
            }
        }
    }
    let path = PathBuf::from(path);
    if basecraft_io::hash_file(&path)? != *hash {
        return Err(invalid(
            "The retained alignment changed; select an intact input explicitly",
        ));
    }
    Ok(Some(path))
}

fn checked_workspace_input(run: &std::path::Path, hash: &str) -> Result<PathBuf> {
    let input = crate::input_evidence::read_input(run)?;
    if input.state == "broken" {
        return Err(invalid("Workspace input evidence was explicitly broken"));
    }
    let source = if let Some(reference) = &input.source {
        crate::input_evidence::verified_source(run, reference)?
    } else {
        run.join("submitted.fasta")
    };
    if basecraft_io::hash_file(&source)? != hash {
        return Err(invalid(
            "Workspace input changed; select an unchanged input explicitly",
        ));
    }
    Ok(source)
}

pub fn checked_display_tree(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<Option<PathBuf>> {
    let Some(path) = &work.view.display_tree else {
        return Ok(None);
    };
    let Some(run) = work.exchange_fields.get("basecraft_display_evidence_run") else {
        if let Some(hash) = work.exchange_fields.get("basecraft_display_hash") {
            if &basecraft_io::hash_file(std::path::Path::new(path))? != hash {
                return Err(invalid(
                    "Display tree changed; select an unchanged tree explicitly",
                ));
            }
        }
        return Ok(Some(PathBuf::from(path)));
    };
    let run = std::path::Path::new(run);
    let input = crate::input_evidence::read_input(run)?;
    if input.state == "broken" {
        return Err(invalid("Display tree evidence was explicitly broken"));
    }
    let source = if let Some(reference) = &input.source {
        crate::input_evidence::verified_source(run, reference)?
    } else {
        run.join("submitted.fasta")
    };
    if work
        .exchange_fields
        .get("basecraft_display_hash")
        .map(String::as_str)
        != Some(basecraft_io::hash_file(&source)?.as_str())
    {
        return Err(invalid(
            "Display tree changed; select an unchanged tree explicitly",
        ));
    }
    Ok(Some(source))
}

/// Apply an immutable method result to the current canonical workspace. GUI,
/// CLI and MCP callers share roster validation and report identity semantics.
pub fn add_analysis_result(
    session: &mut Session,
    doc: DocId,
    output: &std::path::Path,
) -> Result<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(output)?)?;
    if value
        .get("workspace")
        .is_none_or(serde_json::Value::is_null)
    {
        return Ok(value);
    }
    let work: basecraft_protocol::delimitation::DelimitationWorkspace =
        serde_json::from_value(value["workspace"].clone())?;
    let current = session.delimitation(doc);
    if current
        .specimens
        .iter()
        .map(|s| &s.id)
        .collect::<BTreeSet<_>>()
        != work.specimens.iter().map(|s| &s.id).collect()
    {
        return Err(invalid(
            "Analysis specimen mapping differs; result saved separately",
        ));
    }
    let report_hash = basecraft_io::hash_file(output)?;
    let mut edits = Vec::new();
    let mut view = current.view.clone();
    for mut hypothesis in work.hypotheses {
        view.visible.push(hypothesis.id.clone());
        hypothesis.provenance.parameters.insert(
            "analysis_report".into(),
            serde_json::json!({"path":output,"hash":report_hash}),
        );
        edits.push(DelimitationEdit::Add { hypothesis });
    }
    if edits.is_empty() {
        return Ok(value);
    }
    edits.push(DelimitationEdit::SetView { view });
    let mut checked = current.clone();
    for edit in &edits {
        basecraft_docs::delimitation::apply(&mut checked, edit).map_err(|e| invalid(e.0))?;
    }
    for edit in edits {
        session.apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(edit),
            },
        )?;
    }
    Ok(value)
}

pub fn combine(session: &mut Session, doc: DocId, paths: &[PathBuf]) -> Result<()> {
    combine_selected(session, doc, paths, None)
}

/// Combine only the listed hypothesis IDs of each file; `None` keeps all.
pub fn combine_selected(
    session: &mut Session,
    doc: DocId,
    paths: &[PathBuf],
    keep: Option<&BTreeMap<PathBuf, BTreeSet<String>>>,
) -> Result<()> {
    if !session.is_delimitation(doc) {
        return Err(invalid("Choose a species workspace"));
    }
    let roster: BTreeSet<_> = session
        .delimitation(doc)
        .specimens
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let mut hypotheses = vec![];
    for path in paths {
        let hash = basecraft_io::hash_file(path)?;
        let sidecar = files::tree_sidecar(path);
        let side_hash = if sidecar.exists() {
            Some(basecraft_io::hash_file(&sidecar)?)
        } else {
            None
        };
        let work = files::load_delimitation(path)?;
        if basecraft_io::hash_file(path)? != hash
            || (if sidecar.exists() {
                Some(basecraft_io::hash_file(&sidecar)?)
            } else {
                None
            }) != side_hash
        {
            return Err(invalid(
                "A source changed while combining; retry with unchanged files",
            ));
        }
        let anchor = session.root().join(session.doc_key(doc));
        if work
            .specimens
            .iter()
            .map(|s| s.id.clone())
            .collect::<BTreeSet<_>>()
            != roster
        {
            return Err(invalid(format!(
                "{} has a different specimen roster; combine requires matching sequence IDs",
                path.display()
            )));
        }
        let side_evidence = side_hash
            .as_ref()
            .map(|hash| {
                reference_input(&anchor, &sidecar, hash).map(|run| run.directory().to_path_buf())
            })
            .transpose()?;
        let evidence = reference_input_with_dependencies(
            &anchor,
            path,
            &hash,
            &side_evidence.into_iter().collect::<Vec<_>>(),
        )?;
        let ids: BTreeMap<_, _> = work
            .hypotheses
            .iter()
            .map(|h| (h.id.clone(), ulid::Ulid::generate().to_string()))
            .collect();
        let wanted = keep.map(|keep| keep.get(path).cloned().unwrap_or_default());
        for mut hypothesis in work.hypotheses {
            let original = hypothesis.id.clone();
            if wanted
                .as_ref()
                .is_some_and(|wanted| !wanted.contains(&original))
            {
                continue;
            }
            // An edited copy keeps its lineage only if its source came along.
            if hypothesis.parent.as_ref().is_some_and(|parent| {
                wanted
                    .as_ref()
                    .is_some_and(|wanted| !wanted.contains(parent))
            }) {
                hypothesis.parent = None;
            }
            hypothesis.id.clone_from(
                ids.get(&original)
                    .ok_or_else(|| invalid("Missing hypothesis identity"))?,
            );
            hypothesis.parent = hypothesis
                .parent
                .map(|p| {
                    ids.get(&p)
                        .cloned()
                        .ok_or_else(|| invalid("Missing hypothesis parent"))
                })
                .transpose()?;
            hypothesis.provenance.parameters.insert("combined_source".into(), serde_json::json!({"path":path,"hash":hash,"sidecar_hash":side_hash,"hypothesis":original,"evidence_run":evidence.directory()}));
            hypotheses.push(hypothesis);
        }
    }
    if hypotheses.is_empty() {
        return Err(invalid("The selected files contain no hypotheses"));
    }
    session.apply(
        doc,
        Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Combine { hypotheses }),
        },
    )?;
    Ok(())
}
