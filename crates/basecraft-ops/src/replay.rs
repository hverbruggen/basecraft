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
            Op::Save { .. } => match session.save_hashes(doc) {
                Ok(got) => {
                    let want: Vec<&String> = e.result.hashes.iter().map(|(_, h)| h).collect();
                    if got.iter().collect::<Vec<_>>() != want {
                        problem(format!("saved content differs: {got:?} vs {want:?}"));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            Op::Extract { start, end, .. } => match session.extract_hash(doc, *start, *end) {
                Ok(got) => {
                    let want = e.result.hashes.first().map(|(_, h)| h.as_str());
                    if Some(got.as_str()) != want {
                        problem(format!("extracted content differs: {got} vs {want:?}"));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            Op::Undo { of } | Op::Redo { of } => {
                let r = if matches!(e.op, Op::Undo { .. }) {
                    session.undo(doc)
                } else {
                    session.redo(doc)
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
        let state = session.doc(doc);
        if state.len() != e.state.len || state.feature_count() as u64 != e.state.features {
            report.problems.push(format!(
                "{} {}: state len={} features={}, expected len={} features={}",
                e.id,
                e.op.name(),
                state.len(),
                state.feature_count(),
                e.state.len,
                e.state.features
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
        let stored = objects_dir(root).join(hash.trim_start_matches("b3:"));
        let original = root.join(rel);
        let source = if stored.exists() {
            stored
        } else if basecraft_io::cache::content_hash(&original).ok().as_deref() == Some(hash) {
            original
        } else {
            return Err(format!("input {rel} changed and no stored copy exists"));
        };
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
