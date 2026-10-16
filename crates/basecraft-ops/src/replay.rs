//! Replay a project's log and check every step.
//!
//! Inputs come from the object store, or from the project if the file still
//! has the recorded hash. Each edit must give the same feature ids and
//! document state as recorded; each save must produce files with the
//! recorded hashes. Nothing in the project is written.

use std::collections::HashMap;
use std::path::Path;

use basecraft_protocol::{LogEntry, Op};

use crate::session::{DocId, Session, objects_dir};
use crate::{Result, log};

#[derive(Debug, Default)]
pub struct Report {
    pub sessions: usize,
    pub entries: usize,
    pub problems: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

pub fn replay(root: &Path) -> Result<Report> {
    let entries = log::read_all(root)?;
    let mut report = Report::default();
    let mut order: Vec<&str> = Vec::new();
    let mut by_session: HashMap<&str, Vec<&LogEntry>> = HashMap::new();
    for e in &entries {
        let list = by_session.entry(&e.session).or_default();
        if list.is_empty() {
            order.push(&e.session);
        }
        list.push(e);
    }
    for sid in order {
        report.sessions += 1;
        replay_session(root, &by_session[sid], &mut report)?;
    }
    Ok(report)
}

fn replay_session(root: &Path, entries: &[&LogEntry], report: &mut Report) -> Result<()> {
    let work = tempfile::tempdir()?;
    let mut session = Session::unlogged(work.path());
    let mut docs: HashMap<&str, DocId> = HashMap::new();
    // Replay entry id -> logged entry id, to check undo and redo targets.
    let mut ids: HashMap<String, String> = HashMap::new();
    for e in entries {
        report.entries += 1;
        let mut problem = |msg: String| {
            report
                .problems
                .push(format!("{} {} {}: {msg}", e.id, e.op.name(), e.doc));
        };
        if let Op::Open { path } = &e.op {
            if !safe_key(path) || !safe_key(&e.doc) || path != &e.doc {
                problem("unsafe or inconsistent document identity".into());
                continue;
            }
            match materialise(root, work.path(), &e.result.hashes) {
                Ok(()) => match session.open(&work.path().join(path)) {
                    Ok(id) => {
                        docs.insert(&e.doc, id);
                    }
                    Err(err) => problem(format!("can't open: {err}")),
                },
                Err(msg) => problem(msg),
            }
            continue;
        }
        let Some(&doc) = docs.get(e.doc.as_str()) else {
            problem("document wasn't opened".into());
            continue;
        };
        match &e.op {
            // Species metadata records the original folder as its relocation
            // base. Hash against that destination, not replay's disposable root.
            Op::Save { path } => match session.save_hashes_at(doc, &root.join(path)) {
                Ok(got) => {
                    let want: Vec<&String> = e.result.hashes.iter().map(|(_, h)| h).collect();
                    if got.iter().collect::<Vec<_>>() != want {
                        problem(format!("saved content differs: {got:?} vs {want:?}"));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            Op::Extract {
                start,
                end,
                segments,
                reverse,
                ..
            } => match session.extract_hash(doc, (*start, *end), segments, *reverse) {
                Ok(got) => {
                    let want = e.result.hashes.first().map(|(_, h)| h.as_str());
                    if Some(got.as_str()) != want {
                        problem(format!("extracted content differs: {got} vs {want:?}"));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            // External tools aren't re-run: their outputs are new files,
            // and opening one later checks it against its stored copy.
            // Exports are figures; drawing isn't part of replay.
            Op::ToolRun { .. } | Op::Export { .. } | Op::JobFinished { .. } => {}
            Op::Undo { of } | Op::Redo { of } => {
                let r = if matches!(e.op, Op::Undo { .. }) {
                    session.undo_single(doc)
                } else {
                    session.redo_single(doc)
                };
                match r {
                    Ok(replayed) if ids.get(&replayed) == Some(of) => {}
                    Ok(replayed) => {
                        problem(format!("targets {:?}, expected {of}", ids.get(&replayed)));
                    }
                    Err(err) => problem(err.to_string()),
                }
            }
            op => match session.apply_as(doc, op.clone(), e.actor.clone()) {
                Ok(out) => {
                    ids.insert(out.entry_id, e.id.clone());
                    if out.feature.map(|f| f.0) != e.result.feature {
                        problem(format!(
                            "feature id {:?}, expected {:?}",
                            out.feature, e.result.feature
                        ));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
        }
        let state = session.state(doc);
        if state != e.state {
            report.problems.push(format!(
                "{} {}: state {state:?}, expected {:?}",
                e.id,
                e.op.name(),
                e.state
            ));
        }
    }
    Ok(())
}

/// Put each recorded input into `work` under its project-relative path.
fn materialise(
    root: &Path,
    work: &Path,
    inputs: &[(String, String)],
) -> std::result::Result<(), String> {
    for (rel, hash) in inputs {
        if Path::new(rel)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe replay input path: {rel}"));
        }
        let Some(digest) = hash.strip_prefix("b3:") else {
            return Err("unsupported input hash".into());
        };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid input hash".into());
        }
        let stored = objects_dir(root).join(hash.trim_start_matches("b3:"));
        let original = root.join(rel);
        let source = if stored.exists() {
            stored
        } else if basecraft_io::cache::content_hash(&original).ok().as_deref() == Some(hash) {
            original
        } else if let Some(retained) =
            crate::analysis_store::retained_source(root, hash).map_err(|e| e.to_string())?
        {
            retained
        } else {
            return Err(format!("input {rel} changed and no stored copy exists"));
        };
        if basecraft_io::hash_file(&source).map_err(|e| e.to_string())? != *hash {
            return Err(format!("retained input {rel} hash differs"));
        }
        let dest = work.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if std::fs::hard_link(&source, &dest).is_err() {
            std::fs::copy(&source, &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn safe_key(key: &str) -> bool {
    !key.is_empty()
        && Path::new(key)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Reconstruct unsaved documents into a new directory without modifying sources
/// or executing any tools. Strict reading rejects torn/corrupt journals; callers
/// can inspect `verified_prefix` separately before explicitly salvaging a prefix.
pub fn recover(root: &Path, destination: &Path) -> Result<Vec<std::path::PathBuf>> {
    if root.join("recovery-integrity-error.json").exists() {
        return Err(crate::error::invalid(
            "journal durability was uncertain; inspect the preserved verified prefix before recovery",
        ));
    }
    if destination.exists() {
        return Err(crate::error::invalid("recovery destination already exists"));
    }
    let entries = log::read_all(root)?;
    if entries.is_empty() {
        return Err(crate::error::invalid(
            "recovery contains no verified events",
        ));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-recover-")
        .tempdir_in(parent)?;
    let mut session = Session::unlogged(staging.path());
    let mut documents = std::collections::BTreeMap::<String, DocId>::new();
    for entry in entries {
        if let Op::Open { path } = &entry.op {
            if !safe_key(path) || !safe_key(&entry.doc) || path != &entry.doc {
                return Err(crate::error::invalid(
                    "unsafe or inconsistent recovery document identity",
                ));
            }
            materialise(root, staging.path(), &entry.result.hashes)
                .map_err(crate::error::invalid)?;
            let doc = session.open(&staging.path().join(path))?;
            documents.insert(entry.doc.clone(), doc);
            continue;
        }
        let doc = *documents
            .get(&entry.doc)
            .ok_or_else(|| crate::error::invalid("recovery event precedes its baseline"))?;
        match entry.op {
            Op::Save { .. }
            | Op::ToolRun { .. }
            | Op::JobFinished { .. }
            | Op::Export { .. }
            | Op::Extract { .. } => {}
            Op::Undo { .. } => {
                session.undo_single(doc)?;
            }
            Op::Redo { .. } => {
                session.redo_single(doc)?;
            }
            operation => {
                session.apply_as(doc, operation, entry.actor)?;
            }
        }
        if session.state(doc) != entry.state {
            return Err(crate::error::invalid(format!(
                "recovery state differs at {}",
                entry.id
            )));
        }
    }
    let mut outputs = Vec::new();
    for (relative, doc) in documents {
        let target = staging.path().join(&relative);
        session.save(doc, Some(&target))?;
        outputs.push(destination.join(relative));
    }
    drop(session);
    std::fs::rename(staging.path(), destination)?;
    Ok(outputs)
}
