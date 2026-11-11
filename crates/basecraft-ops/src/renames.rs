//! Rows, tips and records are known by name. A rename changes the name and
//! every stored link to it in one undo step, and names stay unique within a
//! document.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use basecraft_protocol::Op;
use basecraft_protocol::metadata::{ChangeSet, MetadataEdit, RecordLink};

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
    Ok(Ok(RenamePlan { edits, links }))
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
