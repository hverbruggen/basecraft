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
    use basecraft_protocol::provenance::{
        ArtifactRevision, Completeness, InputReference, Publication,
    };
    let run =
        crate::analysis_store::ArtifactRun::create(anchor, &ulid::Ulid::generate().to_string())?;
    let source = source.canonicalize()?;
    let reference = InputReference {
        path: crate::input_evidence::relative(run.directory(), &source)?,
        stamp: crate::input_evidence::stamp(&source)?,
        hash: hash.into(),
        extra: BTreeMap::new(),
    };
    let input = crate::input_evidence::SubmittedInput {
        schema_version: 1,
        source: Some(reference.clone()),
        submitted_hash: hash.into(),
        submitted_size: reference.stamp.size,
        transformation: "identity-v1".into(),
        state: "referenced".into(),
        extra: BTreeMap::new(),
    };
    crate::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
    crate::input_evidence::write_json(
        &run.artifact("run.json")?,
        &serde_json::json!({"schema_version":1,"id":run.id(),"result":anchor.file_name().and_then(|n| n.to_str()),"status":"referenced"}),
    )?;
    crate::input_evidence::register(&run)?;
    let publication = Publication {
        schema_version: 1,
        id: run.id().into(),
        session: run.id().into(),
        parents: vec![],
        parent_hashes: BTreeMap::new(),
        created: basecraft_platform::now_rfc3339(),
        origin: "Explicit species workspace input reference".into(),
        inputs: vec![ArtifactRevision {
            id: format!("{}:source", run.id()),
            logical_name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            hash: hash.into(),
            size: reference.stamp.size,
            retained: None,
            reference: Some(reference),
        }],
        outputs: vec![],
        events: vec![],
        effective_edits: vec![],
        completeness: Completeness {
            ancestry_known: false,
            input_bytes_available: true,
            execution_specification_available: false,
            replay_verified: false,
        },
        report_template_version: 2,
    };
    crate::provenance::publish(run.directory(), &publication)?;
    Ok(run)
}

/// Retain declared contributing ledgers, not copies of clean original inputs.
pub(crate) fn dependencies(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<Vec<PathBuf>> {
    let mut runs = BTreeSet::new();
    if let Some(run) = work.exchange_fields.get("basecraft_display_evidence_run") {
        runs.insert(PathBuf::from(run));
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

pub fn checked_display_tree(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<Option<PathBuf>> {
    let Some(path) = &work.view.display_tree else {
        return Ok(None);
    };
    let Some(run) = work.exchange_fields.get("basecraft_display_evidence_run") else {
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

pub fn combine(session: &mut Session, doc: DocId, paths: &[PathBuf]) -> Result<()> {
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
        let _lock = crate::input_evidence::lock()?;
        let anchor = session.root().join(session.doc_key(doc));
        let evidence = reference_input(&anchor, path, &hash)?;
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
        let ids: BTreeMap<_, _> = work
            .hypotheses
            .iter()
            .map(|h| (h.id.clone(), ulid::Ulid::generate().to_string()))
            .collect();
        for mut hypothesis in work.hypotheses {
            let original = hypothesis.id.clone();
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
