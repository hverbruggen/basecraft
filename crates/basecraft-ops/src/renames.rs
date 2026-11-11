//! Rows, tips and records are known by name. A rename changes the name and
//! every stored link to it in one undo step, and names stay unique within a
//! document.
//!
//! What stores a row, tip or record name, and what a rename does to it:
//! project metadata record links (moved), open submissions drawn from the
//! alignment (rows, frames, rules and accessions moved), clade labels and
//! tip labels in the tree itself (moved by the tree). Workspace specimen
//! `sources` are provenance and keep the name they were made from; the
//! roster id is the identity. Sanger reviews refer to files by path and
//! hash, never by record name. Submission export mappings are history.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use basecraft_protocol::Op;
use basecraft_protocol::metadata::{ChangeSet, MetadataEdit, RecordLink};
use basecraft_protocol::submission::SubmissionEdit;

use crate::session::Session;
use crate::{DocId, Result, metadata};

/// Why a rename can't go ahead, for the window to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameRefusal {
    /// Nothing in the document has the old name.
    NotFound { name: String },
    /// Another row or tip already has the new name.
    NameTaken { name: String },
    /// The old name is shared by several rows or tips; make them unique first.
    Ambiguous { name: String },
    /// The project metadata links the old name, but it isn't open, so its
    /// link can't be updated in the same undo step. Open it and try again.
    MetadataNotOpen { path: PathBuf },
}

impl std::fmt::Display for RenameRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { name } => write!(f, "nothing is named {name:?}"),
            Self::NameTaken { name } => write!(f, "{name:?} is already used in this document"),
            Self::Ambiguous { name } => {
                write!(
                    f,
                    "several entries are named {name:?}; make the names unique first"
                )
            }
            Self::MetadataNotOpen { path } => write!(
                f,
                "the specimen metadata in {} links this name; open it so the link is renamed too",
                path.display()
            ),
        }
    }
}

/// The edits a rename makes, in order: the rename itself, then the links.
pub struct RenamePlan {
    edits: Vec<(DocId, Op)>,
    /// Specimen links that will follow the new name.
    pub links: Vec<RecordLink>,
    /// Open submissions whose row decisions will follow the new name.
    pub submissions: Vec<DocId>,
}

/// Plan renaming the row or tip `old` of `doc` to `new`.
pub fn plan(
    session: &Session,
    doc: DocId,
    old: &str,
    new: &str,
) -> Result<std::result::Result<RenamePlan, RenameRefusal>> {
    let names = names(session, doc)?;
    let count = |name: &str| names.iter().filter(|n| *n == name).count();
    match count(old) {
        0 => return Ok(Err(RenameRefusal::NotFound { name: old.into() })),
        1 => {}
        _ => return Ok(Err(RenameRefusal::Ambiguous { name: old.into() })),
    }
    if old != new && count(new) > 0 {
        return Ok(Err(RenameRefusal::NameTaken { name: new.into() }));
    }
    let op = if session.is_tree(doc) {
        Op::TreeRenameTip {
            from: old.into(),
            to: new.into(),
        }
    } else {
        Op::AlnRenameRow {
            row: row_number(names.iter().position(|n| n == old).unwrap_or_default())?,
            name: new.into(),
        }
    };
    let mut edits = vec![(doc, op)];
    let document =
        metadata::document_key(session.root(), &session.root().join(session.doc_key(doc)?))?;
    let linked = |links: &[RecordLink]| -> Vec<RecordLink> {
        links
            .iter()
            .filter(|l| l.document == document && l.record == old)
            .cloned()
            .collect()
    };
    let links = if let Some(meta) = session.project_metadata_id() {
        linked(&session.metadata(meta)?.links)
    } else {
        let manifest = metadata::manifest_path(session.root());
        if manifest.is_file() && !linked(&metadata::load(&manifest)?.metadata.links).is_empty() {
            return Ok(Err(RenameRefusal::MetadataNotOpen { path: manifest }));
        }
        Vec::new()
    };
    if let (Some(meta), false) = (session.project_metadata_id(), links.is_empty()) {
        edits.push((
            meta,
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::Unlink {
                    records: links.iter().map(RecordLink::record_ref).collect(),
                }),
            },
        ));
        edits.push((
            meta,
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::Apply {
                    changes: ChangeSet {
                        links: links
                            .iter()
                            .map(|l| RecordLink {
                                record: new.into(),
                                ..l.clone()
                            })
                            .collect(),
                        ..ChangeSet::default()
                    },
                }),
            },
        ));
    }
    // Open submissions drawn from this alignment keep their row decisions.
    let mut submissions = Vec::new();
    for other in (0..).map(DocId).take_while(|d| session.is_open(*d)) {
        if !session.is_submission(other) {
            continue;
        }
        let submission = session.submission(other)?;
        let uses = submission
            .source
            .as_ref()
            .is_some_and(|source| source.document == document);
        let names_old = submission.rows.iter().any(|r| r.record == old)
            || submission.frames.iter().any(|f| f.record == old)
            || submission.accessions.iter().any(|a| a.record == old)
            || submission.row_rules.contains_key(old);
        if uses && names_old {
            submissions.push(other);
            edits.push((
                other,
                Op::SubmissionEdit {
                    edit: Box::new(SubmissionEdit::RenameRecord {
                        from: old.into(),
                        to: new.into(),
                    }),
                },
            ));
        }
    }
    Ok(Ok(RenamePlan {
        edits,
        links,
        submissions,
    }))
}

/// Apply an edit from a script or an agent. A row or tip rename takes its
/// links along, as it does in the window; anything else applies as it is.
pub fn apply_op(session: &mut Session, doc: DocId, op: Op) -> Result<()> {
    let (old, new) = match &op {
        Op::TreeRenameTip { from, to } => (from.clone(), to.clone()),
        Op::AlnRenameRow { row, name } if session.is_alignment(doc) => {
            let names = names(session, doc)?;
            let old = names
                .get(*row as usize)
                .ok_or_else(|| crate::error::invalid(format!("no row {row}")))?;
            (old.clone(), name.clone())
        }
        _ => return session.apply(doc, op).map(|_| ()),
    };
    match plan(session, doc, &old, &new)? {
        Ok(plan) => apply(session, plan),
        Err(refusal) => Err(crate::error::invalid(refusal.to_string())),
    }
}

/// Apply a plan as one undo step. If any edit fails, the ones already made
/// are undone, so no link is left pointing at a name that is gone.
pub fn apply(session: &mut Session, plan: RenamePlan) -> Result<()> {
    session.begin_group();
    let mut done = Vec::new();
    for (doc, op) in plan.edits {
        if let Err(error) = session.apply(doc, op) {
            session.end_group();
            for doc in done.into_iter().rev() {
                session.undo_single(doc)?;
            }
            return Err(error);
        }
        done.push(doc);
    }
    session.end_group();
    Ok(())
}

/// Names that occur more than once, and the renames that would make every
/// name unique by numbering each occurrence: `a`, `a` become `a.1`, `a.2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateNames {
    pub names: Vec<String>,
    /// Applied in order; each renames the first entry still called `from`.
    pub renames: Vec<(String, String)>,
}

/// Duplicate row or tip names in `doc`, for a warning that offers numbered
/// names or editing by hand. None when every name is unique.
pub fn duplicates(session: &Session, doc: DocId) -> Result<Option<DuplicateNames>> {
    Ok(duplicate_names(&names(session, doc)?))
}

pub fn duplicate_names(names: &[String]) -> Option<DuplicateNames> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    let repeated: Vec<String> = counts
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(name, _)| (*name).to_owned())
        .collect();
    if repeated.is_empty() {
        return None;
    }
    let mut taken: BTreeSet<String> = names.iter().cloned().collect();
    let mut renames = Vec::new();
    for name in &repeated {
        let mut n = 0;
        for _ in 0..counts[name.as_str()] {
            let to = loop {
                n += 1;
                let candidate = format!("{name}.{n}");
                if !taken.contains(&candidate) {
                    break candidate;
                }
            };
            taken.insert(to.clone());
            renames.push((name.clone(), to));
        }
    }
    Some(DuplicateNames {
        names: repeated,
        renames,
    })
}

/// Give every duplicated name its number, as one undo step. Links to a
/// duplicated name never told the entries apart, so they are left alone.
pub fn make_unique(session: &mut Session, doc: DocId, duplicates: &DuplicateNames) -> Result<()> {
    let tree = session.is_tree(doc);
    session.begin_group();
    let mut applied = 0;
    let result = (|| {
        for (from, to) in &duplicates.renames {
            let op = if tree {
                Op::TreeRenameTip {
                    from: from.clone(),
                    to: to.clone(),
                }
            } else {
                let row = names(session, doc)?
                    .iter()
                    .position(|n| n == from)
                    .ok_or_else(|| crate::error::invalid(format!("no row named {from:?}")))?;
                Op::AlnRenameRow {
                    row: row_number(row)?,
                    name: to.clone(),
                }
            };
            session.apply(doc, op)?;
            applied += 1;
        }
        Ok(())
    })();
    session.end_group();
    if result.is_err() {
        for _ in 0..applied {
            session.undo_single(doc)?;
        }
    }
    result
}

fn row_number(row: usize) -> Result<u32> {
    u32::try_from(row).map_err(|_| crate::error::invalid("too many rows"))
}

fn names(session: &Session, doc: DocId) -> Result<Vec<String>> {
    if session.is_tree(doc) {
        let tree = session.tree(doc)?;
        return Ok(tree
            .tips_in_order()
            .into_iter()
            .map(|i| tree.node(i).name.clone())
            .collect());
    }
    Ok(session
        .alignment(doc)?
        .rows()
        .map(|row| row.name.clone())
        .collect())
}

/// Duplicate record names in a multi-record FASTA or GenBank file, for a
/// warning that offers numbered names or editing by hand.
pub fn duplicate_records(path: &Path) -> Result<Option<DuplicateNames>> {
    let bytes = std::fs::read(path)?;
    let names = record_names(path, &bytes)?
        .into_iter()
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    Ok(duplicate_names(&names))
}

/// Give each duplicated record its number in the file itself. Only the names
/// change; every other byte stays as it was. Refused while the file is open,
/// because saving the open copy would undo it.
pub fn number_records(session: &Session, path: &Path, duplicates: &DuplicateNames) -> Result<()> {
    let target = path.canonicalize()?;
    for doc in (0..).map(DocId).take_while(|d| session.is_open(*d)) {
        if session
            .root()
            .join(session.doc_key(doc)?)
            .canonicalize()
            .ok()
            == Some(target.clone())
        {
            return Err(crate::Error::Busy(format!(
                "close {} before numbering its records",
                path.display()
            )));
        }
    }
    let bytes = std::fs::read(path)?;
    let mut queue = BTreeMap::<&str, std::collections::VecDeque<&str>>::new();
    for (from, to) in &duplicates.renames {
        queue.entry(from).or_default().push_back(to);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut copied = 0;
    for (start, name) in record_names(path, &bytes)? {
        let Some(to) = queue.get_mut(name.as_str()).and_then(std::collections::VecDeque::pop_front) else {
            continue;
        };
        let end = start + name.len();
        out.extend_from_slice(&bytes[copied..start]);
        out.extend_from_slice(to.as_bytes());
        // A GenBank LOCUS line keeps its columns when the name grows into
        // the spaces after it; one space always stays.
        let spaces = bytes[end..].iter().take_while(|&&b| b == b' ').count();
        let grow = to.len().saturating_sub(name.len());
        copied = end + if spaces > 1 { grow.min(spaces - 1) } else { 0 };
    }
    out.extend_from_slice(&bytes[copied..]);
    basecraft_io::write_atomic(path, |w| w.write_all(&out))?;
    Ok(())
}

/// Each record's name and the byte offset where it is written.
fn record_names(path: &Path, bytes: &[u8]) -> Result<Vec<(usize, String)>> {
    let mut names = Vec::new();
    let mut offset = 0;
    let genbank = crate::files::Format::from_path(path)? == crate::files::Format::GenBank;
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        let name_at = if genbank {
            line.strip_prefix(b"LOCUS")
                .filter(|rest| rest.first().is_some_and(u8::is_ascii_whitespace))
                .map(|rest| 5 + rest.iter().take_while(|b| b.is_ascii_whitespace()).count())
        } else {
            line.starts_with(b">").then_some(1)
        };
        if let Some(at) = name_at {
            let name: Vec<u8> = line[at..]
                .iter()
                .take_while(|b| !b.is_ascii_whitespace())
                .copied()
                .collect();
            names.push((
                offset + at,
                String::from_utf8(name)
                    .map_err(|_| crate::error::invalid("record names must be UTF-8"))?,
            ));
        }
        offset += line.len();
    }
    Ok(names)
}
