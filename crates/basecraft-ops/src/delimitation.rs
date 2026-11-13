//! Combining saved hypotheses preserves identities, lineage and exact source evidence.
use crate::{DocId, Result, Session, error::invalid, files};
use basecraft_protocol::{Op, delimitation::DelimitationEdit};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Inspect the loaded workspace's unverified linked-alignment hint.
///
/// An explicit snapshot wins even when empty. Otherwise use the first ordered
/// FASTA-like source key shared by every specimen. Preserve the stored spelling;
/// this does no I/O, resolution, hashing or mutation. Analysis execution must
/// still use `checked_alignment` and the registered runner's input checks.
pub fn linked_alignment(session: &Session, doc: DocId) -> Result<Option<&str>> {
    let workspace = session.delimitation(doc)?;
    if let Some(snapshot) = workspace
        .exchange_fields
        .get("basecraft_alignment_snapshot")
    {
        return Ok(Some(snapshot));
    }
    Ok(workspace.specimens.first().and_then(|first| {
        first
            .sources
            .keys()
            .find(|path| {
                Path::new(path).extension().is_some_and(|ext| {
                    ["fa", "fasta", "afa", "fas", "fna"]
                        .iter()
                        .any(|e| ext.eq_ignore_ascii_case(e))
                }) && workspace
                    .specimens
                    .iter()
                    .all(|specimen| specimen.sources.contains_key(*path))
            })
            .map(String::as_str)
    }))
}

/// Publish a new canonical exchange file without replacing a user file. The
/// authoritative workspace is hidden; native per-fit SPARTs stay in their runs.
pub fn publish_workspace(
    target: &std::path::Path,
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    run: &crate::records::analysis_store::ArtifactRun,
) -> Result<()> {
    let name = stage_workspace(target, work, run)?;
    run.publish_primary(&name)?;
    run.bind(target)
}

/// Which specimens of a new workspace were linked to the project roster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RosterLinks {
    /// False when the project keeps no specimen metadata.
    pub roster: bool,
    pub linked: usize,
    /// Workspace specimens whose source record has no specimen link, such
    /// as a row renamed after the analysis started.
    pub unlinked: Vec<String>,
}

impl RosterLinks {
    /// A line for the job's history, when some specimens stayed unlinked.
    pub fn summary(&self) -> Option<String> {
        (self.roster && !self.unlinked.is_empty()).then(|| {
            format!(
                "{} of {} specimens linked to the project roster; no specimen link for {}",
                self.linked,
                self.linked + self.unlinked.len(),
                self.unlinked.join(", ")
            )
        })
    }
}

/// Link each specimen of a workspace being made to its roster specimen,
/// through the project metadata's link for the record it came from. Names
/// are never matched. Workspace ids stay as the SPART file has them. A source
/// inside `run` is the run's retained input and stands for the document it
/// was taken from. Damaged metadata is an error, not an unlinked workspace.
pub fn link_roster(
    project: &Path,
    run: &crate::records::analysis_store::ArtifactRun,
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<RosterLinks> {
    let mut reader = Session::unlogged(project);
    let Some(meta) = crate::metadata::open(&mut reader, false)? else {
        return Ok(RosterLinks::default());
    };
    let original = crate::records::read_input(run.directory())
        .ok()
        .and_then(|input| input.source)
        .map(|source| source.path);
    let by_record: BTreeMap<(&str, &str), &str> = reader
        .metadata(meta)?
        .links
        .iter()
        .map(|l| {
            (
                (l.document.as_str(), l.record.as_str()),
                l.specimen.as_str(),
            )
        })
        .collect();
    let links: BTreeMap<String, String> = work
        .specimens
        .iter()
        .filter_map(|s| {
            s.sources.iter().find_map(|(path, record)| {
                let path = match &original {
                    Some(original) if Path::new(path).starts_with(run.directory()) => original,
                    _ => path,
                };
                let key = crate::metadata::document_key(project, Path::new(path)).ok()?;
                let roster = by_record.get(&(key.as_str(), record.as_str()))?;
                Some((s.id.clone(), (*roster).to_owned()))
            })
        })
        .collect();
    let unlinked = work
        .specimens
        .iter()
        .filter(|s| !links.contains_key(&s.id))
        .map(|s| s.id.clone())
        .collect();
    let linked = links.len();
    if linked > 0 {
        let edit = DelimitationEdit::SetRosterLinks {
            links,
            metadata: crate::metadata::consumer_source(&reader, meta).ok(),
        };
        basecraft_docs::delimitation::apply(work, &edit).map_err(|e| invalid(e.0))?;
    }
    Ok(RosterLinks {
        roster: true,
        linked,
        unlinked,
    })
}

/// Publish a run's species workspace as a new SPART file at `target`, which
/// may lie anywhere in the project; never replaces an existing file.
pub fn publish_workspace_to(
    target: &std::path::Path,
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    run: &crate::records::analysis_store::ArtifactRun,
) -> Result<()> {
    let name = stage_workspace(target, work, run)?;
    run.publish(
        &name,
        crate::records::analysis_store::ArtifactDestination::Metadata,
    )?;
    run.publish_secondary(&name, target)?;
    run.bind(target)
}

/// Stage the SPART and its sidecar in the run; returns the staged name.
fn stage_workspace(
    target: &std::path::Path,
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    run: &crate::records::analysis_store::ArtifactRun,
) -> Result<String> {
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
        files::delimitation_companion(work, &hash, target)?.write(w)
    })?;
    Ok(name.into())
}

/// Explicitly attach a clean file to a species workspace using the common
/// reference lifecycle. Reading a saved workspace never calls this writer.
pub fn reference_input(
    anchor: &std::path::Path,
    source: &std::path::Path,
    hash: &str,
    project: Option<&crate::project::Project>,
) -> Result<crate::records::analysis_store::ArtifactRun> {
    reference_input_with_dependencies(anchor, source, hash, &[], project)
}

fn reference_input_with_dependencies(
    anchor: &std::path::Path,
    source: &std::path::Path,
    hash: &str,
    dependencies: &[PathBuf],
    project: Option<&crate::project::Project>,
) -> Result<crate::records::analysis_store::ArtifactRun> {
    crate::records::input_evidence::reference_input_with_dependencies(
        anchor,
        source,
        hash,
        dependencies,
        "Explicit species workspace input reference",
        project,
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
                return Err(crate::Error::ChangedOnDisk {
                    path: path.to_path_buf(),
                });
            }
            if let Some(run) = crate::records::analysis_store::ArtifactRun::for_result(path)? {
                runs.insert(run);
            }
        }
        if let Some(source) = hypothesis.provenance.parameters.get("combined_source")
            && let Some(run) = source["evidence_run"].as_str()
        {
            runs.insert(PathBuf::from(run));
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
    let bundle = crate::records::provenance::bundle_for_result(workspace_path, None);
    if bundle.join("input.json").is_file() {
        let mut candidates = vec![bundle.clone()];
        candidates.extend(
            crate::records::input_evidence::ancestors(&bundle)?
                .into_iter()
                .map(|(p, _)| p),
        );
        for candidate in candidates {
            if !candidate.join("input.json").is_file() {
                continue;
            }
            let input = crate::records::input_evidence::read_input(&candidate)?;
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
        return Err(crate::Error::ChangedOnDisk { path: path.clone() });
    }
    Ok(Some(path))
}

fn checked_workspace_input(run: &std::path::Path, hash: &str) -> Result<PathBuf> {
    let input = crate::records::input_evidence::read_input(run)?;
    if input.state == basecraft_protocol::input_records::EvidenceState::Broken {
        return Err(invalid("Workspace input evidence was explicitly broken"));
    }
    let source = if let Some(reference) = &input.source {
        crate::records::input_evidence::verified_source(run, reference)?
    } else {
        crate::records::analysis_store::submitted_input(run)
            .ok_or_else(|| invalid("Retained workspace input is missing"))?
    };
    if basecraft_io::hash_file(&source)? != hash {
        return Err(crate::Error::ChangedOnDisk {
            path: source.clone(),
        });
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
        if let Some(hash) = work.exchange_fields.get("basecraft_display_hash")
            && &basecraft_io::hash_file(std::path::Path::new(path))? != hash
        {
            return Err(crate::Error::ChangedOnDisk {
                path: PathBuf::from(path),
            });
        }
        return Ok(Some(PathBuf::from(path)));
    };
    let run = std::path::Path::new(run);
    let input = crate::records::input_evidence::read_input(run)?;
    if input.state == basecraft_protocol::input_records::EvidenceState::Broken {
        return Err(invalid("Display tree evidence was explicitly broken"));
    }
    let source = if let Some(reference) = &input.source {
        crate::records::input_evidence::verified_source(run, reference)?
    } else {
        crate::records::analysis_store::submitted_input(run)
            .ok_or_else(|| invalid("Retained workspace input is missing"))?
    };
    if work
        .exchange_fields
        .get("basecraft_display_hash")
        .map(String::as_str)
        != Some(basecraft_io::hash_file(&source)?.as_str())
    {
        return Err(crate::Error::ChangedOnDisk { path: source });
    }
    Ok(Some(source))
}

/// Why a file cannot be a species workspace's display tree.
#[derive(Debug, thiserror::Error)]
pub enum DisplayTreeError {
    #[error(
        "Attach a display tree first (delimitation display-tree WORKSPACE --tree FILE, or Attach tree in the window)"
    )]
    NoneAttached,
    #[error("{} is not a text tree file", .0.display())]
    NotText(PathBuf),
    #[error("Choose a file containing exactly one display tree; {} holds {count}", path.display())]
    NotOneTree { path: PathBuf, count: usize },
}

/// A display tree as read for a species workspace.
pub struct DisplayTreeFile {
    /// The file read: the one named, or for the tree already attached, its
    /// kept copy.
    pub path: PathBuf,
    pub tree: basecraft_io::newick::ParsedTree,
    pub hash: String,
    /// The workspace edit that attaches the tree, with the file kept as its
    /// evidence; `None` when the workspace already shows this tree.
    pub edit: Option<DelimitationEdit>,
}

/// Read `named` as the display tree of species workspace `doc`: one tree,
/// kept as evidence when it is new to the workspace. The window, the CLI and
/// MCP attach a display tree through this.
pub fn display_tree(session: &Session, doc: DocId, named: &Path) -> Result<DisplayTreeFile> {
    let work = session.delimitation(doc)?;
    let same = work.view.display_tree.as_deref() == named.to_str();
    let path = if same {
        checked_display_tree(work)?.unwrap_or_else(|| named.to_path_buf())
    } else {
        named.to_path_buf()
    };
    let bytes = std::fs::read(&path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| DisplayTreeError::NotText(path.clone()))?;
    let mut trees = basecraft_io::newick::parse_trees(text)?;
    if trees.len() != 1 {
        return Err(DisplayTreeError::NotOneTree {
            path,
            count: trees.len(),
        }
        .into());
    }
    let hash = basecraft_io::hash_bytes(&bytes);
    let edit = if same {
        None
    } else {
        let run = reference_input(
            &session.root().join(session.doc_key(doc)?),
            &path,
            &hash,
            session.project(),
        )?;
        Some(DelimitationEdit::SetDisplayTree {
            path: path.to_string_lossy().into_owned(),
            hash: hash.clone(),
            evidence_run: run.directory().display().to_string(),
        })
    };
    Ok(DisplayTreeFile {
        path,
        tree: trees.remove(0),
        hash,
        edit,
    })
}

/// The display tree attached to species workspace `doc`, read from its kept
/// copy; refused by name when none is attached.
pub fn attached_display_tree(session: &Session, doc: DocId) -> Result<DisplayTreeFile> {
    let named = session
        .delimitation(doc)?
        .view
        .display_tree
        .clone()
        .ok_or(DisplayTreeError::NoneAttached)?;
    display_tree(session, doc, Path::new(&named))
}

/// Attach the tree in `tree` as the display tree of species workspace `doc`.
pub fn attach_display_tree(session: &mut Session, doc: DocId, tree: &Path) -> Result<()> {
    if let Some(edit) = display_tree(session, doc, tree)?.edit {
        session.apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(edit),
            },
        )?;
    }
    Ok(())
}

/// Apply an immutable method result to the current canonical workspace. GUI,
/// CLI and MCP callers share roster validation and report identity semantics.
pub fn add_analysis_result(
    session: &mut Session,
    doc: DocId,
    output: &std::path::Path,
) -> Result<serde_json::Value> {
    let (value, edits) = analysis_result_edits(session, doc, output)?;
    session.begin_group();
    let applied = edits.into_iter().try_for_each(|edit| {
        session
            .apply(
                doc,
                Op::DelimitationEdit {
                    edit: Box::new(edit),
                },
            )
            .map(|_| ())
    });
    session.end_group();
    applied?;
    Ok(value)
}

/// Validate and plan result attachment before any typed changes. Job owners
/// retain this plan and advance one edit at a time across repair attempts.
pub fn analysis_result_edits(
    session: &Session,
    doc: DocId,
    output: &std::path::Path,
) -> Result<(serde_json::Value, Vec<DelimitationEdit>)> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(output)?)?;
    if value
        .get("workspace")
        .is_none_or(serde_json::Value::is_null)
    {
        return Ok((value, Vec::new()));
    }
    let work: basecraft_protocol::delimitation::DelimitationWorkspace =
        serde_json::from_value(value["workspace"].clone())?;
    let current = session.delimitation(doc)?;
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
        return Ok((value, Vec::new()));
    }
    edits.push(DelimitationEdit::SetView { view });
    let mut checked = current.clone();
    for edit in &edits {
        basecraft_docs::delimitation::apply(&mut checked, edit).map_err(|e| invalid(e.0))?;
    }
    Ok((value, edits))
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
    session.delimitation(doc)?;
    let generation = session.generation(doc)?;
    let revision = session.revision(doc)?;
    let hypotheses = combined_hypotheses(session, doc, paths, keep)?;
    if hypotheses.is_empty() {
        if keep.is_some() && !session.delimitation(doc)?.hypotheses.is_empty() {
            return Ok(());
        }
        return Err(invalid("The selected files contain no hypotheses"));
    }
    session.apply_delimitation_proposals(
        doc,
        generation,
        revision,
        &[Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Combine { hypotheses }),
        }],
    )?;
    Ok(())
}

/// Plan exact imported payloads and source evidence without changing the target.
fn combined_hypotheses(
    session: &Session,
    doc: DocId,
    paths: &[PathBuf],
    keep: Option<&CombineSelection>,
) -> Result<Vec<basecraft_protocol::delimitation::Hypothesis>> {
    let roster: BTreeSet<_> = session
        .delimitation(doc)?
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
            return Err(crate::Error::ChangedOnDisk { path: path.clone() });
        }
        if let Some(wanted) = keep.and_then(|keep| keep.get(path)) {
            let available = work.hypotheses.iter().map(|h| h.id.clone()).collect();
            if !wanted.is_subset(&available) {
                return Err(invalid(format!(
                    "Selected hypothesis no longer exists in {}",
                    path.display()
                )));
            }
        }
        let anchor = session.root().join(session.doc_key(doc)?);
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
                reference_input(&anchor, &sidecar, hash, session.project())
                    .map(|run| run.directory().to_path_buf())
            })
            .transpose()?;
        let evidence = reference_input_with_dependencies(
            &anchor,
            path,
            &hash,
            &side_evidence.into_iter().collect::<Vec<_>>(),
            session.project(),
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
    Ok(hypotheses)
}

/// Explicit hypothesis IDs by source path. Missing files in a supplied map keep
/// no hypotheses; an absent map keeps all hypotheses.
pub type CombineSelection = BTreeMap<PathBuf, BTreeSet<String>>;

/// Candidate data, independent of GUI labels or Qt. This is a transient query,
/// not a new saved format.
#[derive(Debug, serde::Serialize)]
pub struct CombineCandidate {
    pub hypothesis: basecraft_protocol::delimitation::Hypothesis,
    pub best: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct CombineFile {
    pub path: PathBuf,
    pub file: Option<String>,
    pub specimens: usize,
    pub hypotheses: Vec<CombineCandidate>,
}

/// Inspect saved hypotheses in stable recommendation order without writing.
pub fn combine_candidates(paths: &[PathBuf]) -> Result<Vec<CombineFile>> {
    use basecraft_protocol::delimitation::HypothesisOrigin;
    paths
        .iter()
        .map(|path| {
            let work = files::load_delimitation(path)?;
            let mut hypotheses: Vec<_> = work
                .hypotheses
                .into_iter()
                .map(|hypothesis| {
                    let best = ["asap_rank", "gmyc_rank"]
                        .iter()
                        .any(|key| hypothesis.attributes.get(*key).is_some_and(|r| r == "1"))
                        || (hypothesis.origin == HypothesisOrigin::Inferred
                            && !hypothesis.attributes.contains_key("asap_rank")
                            && !hypothesis.attributes.contains_key("gmyc_rank"));
                    CombineCandidate { hypothesis, best }
                })
                .collect();
            hypotheses.sort_by_key(|entry| {
                (
                    !entry.best,
                    !matches!(
                        entry.hypothesis.origin,
                        HypothesisOrigin::Inferred | HypothesisOrigin::Imported
                    ),
                )
            });
            Ok(CombineFile {
                path: path.clone(),
                file: path.file_name().map(|n| n.to_string_lossy().into_owned()),
                specimens: work.specimens.len(),
                hypotheses,
            })
        })
        .collect()
}

/// Combine saved workspaces into a new file using the caller's recoverable
/// session and actor. Source edits and Save As retain their existing journal,
/// evidence and partial-save recovery semantics. No source is overwritten.
pub fn combine_to(
    session: &mut Session,
    paths: &[PathBuf],
    output: &std::path::Path,
    keep: Option<&CombineSelection>,
) -> Result<DocId> {
    if paths.len() < 2 {
        return Err(invalid(
            "Choose at least two saved species hypothesis files",
        ));
    }
    if output.try_exists()? || files::tree_sidecar(output).try_exists()? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Choose a new filename for the combined hypotheses",
        )
        .into());
    }
    // Match aliases of existing files without collapsing input entries or
    // identifying distinct files by their names or contents. Output handling
    // remains with the existing Save As guards.
    let paths = paths
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<Vec<_>>>()?;
    let keep = keep
        .map(|keep| -> Result<CombineSelection> {
            let mut normalized = CombineSelection::new();
            for (path, ids) in keep {
                let path = std::fs::canonicalize(path)?;
                if normalized.insert(path, ids.clone()).is_some() {
                    return Err(invalid(
                        "Hypothesis selection names the same input through multiple path aliases",
                    ));
                }
            }
            Ok(normalized)
        })
        .transpose()?;
    let keep = keep.as_ref();
    if let Some(keep) = keep {
        if keep.keys().any(|path| !paths.contains(path)) {
            return Err(invalid(
                "Hypothesis selection names a file outside the chosen inputs",
            ));
        }
        if keep.values().all(BTreeSet::is_empty) {
            return Err(invalid("Choose at least one hypothesis to combine"));
        }
    }
    let doc = session.open(&paths[0])?;
    if !session.is_delimitation(doc) {
        return Err(invalid("Choose saved species hypothesis files"));
    }
    let generation = session.generation(doc)?;
    let revision = session.revision(doc)?;
    let mut ops = Vec::new();
    let current = session.delimitation(doc)?;
    let retained = if let Some(keep) = keep {
        let wanted = keep.get(&paths[0]).cloned().unwrap_or_default();
        let available: BTreeSet<_> = current.hypotheses.iter().map(|h| h.id.clone()).collect();
        if !wanted.is_subset(&available) {
            return Err(invalid(
                "Selected hypothesis no longer exists in the first input",
            ));
        }
        let dropped = available
            .difference(&wanted)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !dropped.is_empty() {
            ops.push(Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Remove {
                    hypotheses: dropped,
                }),
            });
        }
        wanted.len()
    } else {
        current.hypotheses.len()
    };
    let hypotheses = combined_hypotheses(session, doc, &paths[1..], keep)?;
    if hypotheses.is_empty() {
        if keep.is_none() || retained == 0 {
            return Err(invalid("The selected files contain no hypotheses"));
        }
    } else {
        ops.push(Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Combine { hypotheses }),
        });
    }
    session.apply_delimitation_proposals(doc, generation, revision, &ops)?;
    session.save(doc, Some(output))?;
    Ok(doc)
}

/// A free `<common root>.combinedNN.spart` beside the last input.
pub fn combined_path(paths: &[PathBuf]) -> PathBuf {
    let Some(last) = paths.last() else {
        return PathBuf::new();
    };
    let directory = last.parent().unwrap_or(Path::new("."));
    let names: Vec<String> = paths
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let first = names.first().map_or("", String::as_str);
    let mut shared = first.len();
    for name in names.iter().skip(1) {
        shared = first
            .char_indices()
            .zip(name.chars())
            .find(|((_, a), b)| a != b)
            .map_or(shared.min(name.len()), |((i, _), _)| i.min(shared));
    }
    // Keep whole name parts: "specimens.afa.asap0" becomes "specimens.afa".
    let root = if shared == first.len() && names.iter().all(|n| n == first) {
        first.rsplit_once('.').map_or(first, |(stem, _)| stem)
    } else {
        first[..shared]
            .rsplit_once('.')
            .map_or("", |(stem, _)| stem)
    };
    (1..=u32::MAX)
        .map(|n| {
            directory.join(if root.is_empty() {
                format!("combined{n:02}.spart")
            } else {
                format!("{root}.combined{n:02}.spart")
            })
        })
        .find(|path| !path.exists())
        .unwrap_or_default()
}
