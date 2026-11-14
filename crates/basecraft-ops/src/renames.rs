//! Rows, tips and records are known by name. A rename changes the name and
//! every stored link to it in one undo step, and names stay unique within a
//! document.
//!
//! What stores a row, tip or record name, and what a rename does to it:
//! project metadata record links (moved; refused while the saved metadata
//! isn't open), submissions drawn from the alignment (rows, frames, rules
//! and accessions moved; refused while a saved one isn't open), clade labels
//! and tip labels in the tree itself (moved by the tree). Species workspaces
//! match a tree's tips to specimens by name and keep their SPART names, so a
//! tip rename on a tree a workspace uses is refused. A workspace or submission
//! in the project that can't be read refuses the rename too, since it can't
//! be checked. Workspace specimen
//! `sources` of alignments are provenance and keep the name they were made
//! from; the roster id is the identity. Sanger reviews refer to files by path
//! and hash, never by record name. Submission export mappings are history.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use basecraft_protocol::Op;
use basecraft_protocol::metadata::{ChangeSet, MetadataEdit, RecordLink};
use basecraft_protocol::submission::{Submission, SubmissionEdit};

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
    /// A saved submission drawn from this alignment names the old row, but
    /// it isn't open. Open it and try again.
    SubmissionNotOpen { path: PathBuf },
    /// A submission already has a row with the new name.
    SubmissionRowTaken { path: PathBuf },
    /// A species workspace matches its specimens to this tree's tips by name.
    UsedByWorkspace { path: PathBuf },
    /// A workspace or submission in the project couldn't be read, so it
    /// can't be checked for the old name.
    Unreadable { path: PathBuf },
}

impl std::error::Error for RenameRefusal {}

impl std::fmt::Display for RenameRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { name } => write!(f, "nothing is named {name:?}"),
            Self::NameTaken { name } => write!(f, "{name:?} is already used in this document"),
            Self::Ambiguous { name } => write!(
                f,
                "several entries are named {name:?}; make the names unique first"
            ),
            Self::MetadataNotOpen { path } => write!(
                f,
                "the specimen metadata in {} links this name; open it so the link is renamed too",
                path.display()
            ),
            Self::SubmissionNotOpen { path } => write!(
                f,
                "the submission {} uses this row; open it so its decision is renamed too",
                path.display()
            ),
            Self::SubmissionRowTaken { path } => write!(
                f,
                "the submission {} already has a row with the new name",
                path.display()
            ),
            Self::UsedByWorkspace { path } => write!(
                f,
                "the species workspace {} matches specimens to these tips by name",
                path.display()
            ),
            Self::Unreadable { path } => write!(
                f,
                "can't check {}: it couldn't be read; fix or move it, then rename",
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
    let file = session.root().join(session.doc_key(doc)?);
    let mut unreadable = None;
    let op = if session.is_tree(doc) {
        let users = workspaces_using(session, &file, &mut unreadable)?;
        if let Some(path) = unreadable {
            return Ok(Err(RenameRefusal::Unreadable { path }));
        }
        if let Some(path) = users.into_iter().next() {
            return Ok(Err(RenameRefusal::UsedByWorkspace { path }));
        }
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
    let document = metadata::document_key(session.root(), &file)?;
    let wanted = BTreeSet::from([old]);
    let links = match linked(session, &document, &wanted)? {
        Linked::Open(meta, links) => {
            if !links.is_empty() {
                edits.extend(move_links(meta, &links, new));
            }
            links
        }
        Linked::Closed(path) => return Ok(Err(RenameRefusal::MetadataNotOpen { path })),
        Linked::None => Vec::new(),
    };
    let mut submissions = Vec::new();
    let naming = submissions_naming(session, &document, &wanted, &mut unreadable)?;
    if let Some(path) = unreadable {
        return Ok(Err(RenameRefusal::Unreadable { path }));
    }
    for (path, open) in naming {
        let Some(other) = open else {
            return Ok(Err(RenameRefusal::SubmissionNotOpen { path }));
        };
        if names_any(session.submission(other)?, &BTreeSet::from([new])) {
            return Ok(Err(RenameRefusal::SubmissionRowTaken { path }));
        }
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
    Ok(Ok(RenamePlan {
        edits,
        links,
        submissions,
    }))
}

fn move_links(meta: DocId, links: &[RecordLink], new: &str) -> [(DocId, Op); 2] {
    let unlink = MetadataEdit::Unlink {
        records: links.iter().map(RecordLink::record_ref).collect(),
    };
    let relink = MetadataEdit::Apply {
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
    };
    [unlink, relink].map(|edit| {
        (
            meta,
            Op::MetadataEdit {
                edit: Box::new(edit),
            },
        )
    })
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
        Err(refusal) => Err(refusal.into()),
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DuplicateNames {
    pub names: Vec<String>,
    /// In order of the entries; each renames the next entry called `from`.
    pub renames: Vec<(String, String)>,
    /// Specimen links that name a duplicated name. Numbering leaves them
    /// pointing at nothing, so the user has to relink them.
    pub links: Vec<RecordLink>,
    /// Submissions with rows named by a duplicated name, left the same way.
    pub submissions: Vec<PathBuf>,
}

/// Duplicate row or tip names in `doc`, for a warning that offers numbered
/// names or editing by hand. None when every name is unique.
pub fn duplicates(session: &Session, doc: DocId) -> Result<Option<DuplicateNames>> {
    let file = session.root().join(session.doc_key(doc)?);
    with_references(session, &file, duplicate_names(&names(session, doc)?))
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
        ..DuplicateNames::default()
    })
}

fn with_references(
    session: &Session,
    file: &Path,
    found: Option<DuplicateNames>,
) -> Result<Option<DuplicateNames>> {
    let Some(mut found) = found else {
        return Ok(None);
    };
    let document = metadata::document_key(session.root(), file)?;
    let names: BTreeSet<&str> = found.names.iter().map(String::as_str).collect();
    found.links = match linked(session, &document, &names)? {
        Linked::Open(_, links) => links,
        Linked::Closed(path) => metadata::load(&path)?
            .metadata
            .links
            .into_iter()
            .filter(|l| l.document == document && names.contains(l.record.as_str()))
            .collect(),
        Linked::None => Vec::new(),
    };
    found.submissions = submissions_naming(session, &document, &names, &mut None)?
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    Ok(Some(found))
}

/// Give every duplicated name its number, as one undo step. Links to a
/// duplicated name never told the entries apart, so none is guessed; the
/// warning lists them in `duplicates.links` for relinking.
pub fn make_unique(session: &mut Session, doc: DocId, duplicates: &DuplicateNames) -> Result<()> {
    let mut queue = numbering(duplicates);
    let edits = if session.is_tree(doc) {
        duplicates
            .renames
            .iter()
            .map(|(from, to)| {
                let (from, to) = (from.clone(), to.clone());
                (doc, Op::TreeRenameTip { from, to })
            })
            .collect()
    } else {
        let mut edits = Vec::new();
        for (row, name) in names(session, doc)?.iter().enumerate() {
            if let Some(to) = queue.get_mut(name.as_str()).and_then(VecDeque::pop_front) {
                let row = row_number(row)?;
                edits.push((
                    doc,
                    Op::AlnRenameRow {
                        row,
                        name: to.into(),
                    },
                ));
            }
        }
        edits
    };
    apply(
        session,
        RenamePlan {
            edits,
            links: Vec::new(),
            submissions: Vec::new(),
        },
    )
}

fn numbering(duplicates: &DuplicateNames) -> BTreeMap<&str, VecDeque<&str>> {
    let mut queue = BTreeMap::<&str, VecDeque<&str>>::new();
    for (from, to) in &duplicates.renames {
        queue.entry(from).or_default().push_back(to);
    }
    queue
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

enum Linked {
    /// The project metadata is open; these of its links name the records.
    Open(DocId, Vec<RecordLink>),
    /// The saved metadata links one of the records but isn't open.
    Closed(PathBuf),
    None,
}

fn linked(session: &Session, document: &str, names: &BTreeSet<&str>) -> Result<Linked> {
    let naming = |links: &[RecordLink]| -> Vec<RecordLink> {
        links
            .iter()
            .filter(|l| l.document == document && names.contains(l.record.as_str()))
            .cloned()
            .collect()
    };
    if let Some(meta) = session.project_metadata_id() {
        return Ok(Linked::Open(meta, naming(&session.metadata(meta)?.links)));
    }
    let manifest = metadata::manifest_path(session.root());
    if manifest.is_file() && !naming(&metadata::load(&manifest)?.metadata.links).is_empty() {
        return Ok(Linked::Closed(manifest));
    }
    Ok(Linked::None)
}

fn names_any(submission: &Submission, names: &BTreeSet<&str>) -> bool {
    submission
        .rows
        .iter()
        .any(|r| names.contains(r.record.as_str()))
        || submission
            .frames
            .iter()
            .any(|f| names.contains(f.record.as_str()))
        || submission
            .accessions
            .iter()
            .any(|a| names.contains(a.record.as_str()))
        || submission
            .row_rules
            .keys()
            .any(|r| names.contains(r.as_str()))
}

/// Submissions drawn from `document` that name one of `names`, open ones
/// with their id and saved ones found in the project. A saved one that can't
/// be read lands in `unreadable`.
fn submissions_naming(
    session: &Session,
    document: &str,
    names: &BTreeSet<&str>,
    unreadable: &mut Option<PathBuf>,
) -> Result<Vec<(PathBuf, Option<DocId>)>> {
    let draws = |s: &Submission| {
        s.source
            .as_ref()
            .is_some_and(|source| source.document == document)
            && names_any(s, names)
    };
    let open = open_files(session)?;
    let mut found = Vec::new();
    for (doc, path) in &open {
        if session.is_submission(*doc) && draws(session.submission(*doc)?) {
            found.push((path.clone(), Some(*doc)));
        }
    }
    for path in project_files(session.root(), &open) {
        if !crate::submission::is_submission(&path) {
            continue;
        }
        match crate::submission::load(&path) {
            Ok(submission) if draws(&submission) => found.push((path, None)),
            Ok(_) => {}
            Err(_) => note_unreadable(session, unreadable, path),
        }
    }
    Ok(found)
}

/// Species workspaces, open or saved in the project, that use the tree in
/// `tree`: as their display tree, as a specimen source, or as the tree an
/// analysis ran on. A saved one that can't be read lands in `unreadable`.
fn workspaces_using(
    session: &Session,
    tree: &Path,
    unreadable: &mut Option<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let Ok(tree) = tree.canonicalize() else {
        return Ok(Vec::new());
    };
    let hash = basecraft_io::hash_file(&tree).ok();
    let uses = |work: &basecraft_protocol::delimitation::DelimitationWorkspace, at: &Path| {
        let dir = at.parent().unwrap_or(Path::new("."));
        let names_tree = |s: &str| dir.join(s).canonicalize().is_ok_and(|p| p == tree);
        work.view.display_tree.as_deref().is_some_and(names_tree)
            || work
                .specimens
                .iter()
                .any(|s| s.sources.keys().any(|k| names_tree(k)))
            || work
                .hypotheses
                .iter()
                .any(|h| h.provenance.analysis_tree.is_some() && h.provenance.analysis_tree == hash)
    };
    let open = open_files(session)?;
    let mut found = Vec::new();
    for (doc, path) in &open {
        if session.is_delimitation(*doc) && uses(session.delimitation(*doc)?, path) {
            found.push(path.clone());
        }
    }
    for path in project_files(session.root(), &open) {
        if crate::files::Format::from_path(&path).ok() != Some(crate::files::Format::Delimitation) {
            continue;
        }
        match crate::files::load_delimitation_in(session.companion_project(), &path) {
            Ok(work) if uses(&work, &path) => found.push(path),
            Ok(_) => {}
            Err(_) => note_unreadable(session, unreadable, path),
        }
    }
    Ok(found)
}

fn open_files(session: &Session) -> Result<Vec<(DocId, PathBuf)>> {
    (0..)
        .map(DocId)
        .take_while(|d| session.is_open(*d))
        .map(|d| {
            let path = session.root().join(session.doc_key(d)?);
            Ok((d, path.canonicalize().unwrap_or(path)))
        })
        .collect()
}

/// A file the user keeps that couldn't be read. Records inside `.basecraft`
/// that a past analysis left are history, not documents, and don't block.
fn note_unreadable(session: &Session, unreadable: &mut Option<PathBuf>, path: PathBuf) {
    let store = session.root().join(".basecraft");
    let store = store.canonicalize().unwrap_or(store);
    if unreadable.is_none() && !path.starts_with(store) {
        *unreadable = Some(path);
    }
}

/// Files in the project, including results bound inside `.basecraft`, other
/// than the ones open in the session. Git's folder and unfinished analysis
/// output (`pending`) are skipped.
fn project_files(root: &Path, open: &[(DocId, PathBuf)]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if matches!(entry.file_name().to_str(), Some(".git" | "pending")) {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(path),
                Ok(kind) if kind.is_file() => {
                    let path = path.canonicalize().unwrap_or(path);
                    if !open.iter().any(|(_, p)| *p == path) {
                        found.push(path);
                    }
                }
                _ => {}
            }
        }
    }
    found
}

/// Duplicate record names in a multi-record FASTA or GenBank file, for a
/// warning that offers numbered names or editing by hand.
pub fn duplicate_records(session: &Session, path: &Path) -> Result<Option<DuplicateNames>> {
    let bytes = std::fs::read(path)?;
    let names = record_names(path, bytes)?
        .into_iter()
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    with_references(session, path, duplicate_names(&names))
}

/// Give each duplicated record its number in the file itself. Only the names
/// change; every other byte stays as it was. Refused while the file is open,
/// because saving the open copy would undo it.
pub fn number_records(session: &Session, path: &Path, duplicates: &DuplicateNames) -> Result<()> {
    let target = path.canonicalize()?;
    if open_files(session)?.iter().any(|(_, p)| *p == target) {
        return Err(crate::Error::Busy(format!(
            "close {} before numbering its records",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)?;
    let mut queue = numbering(duplicates);
    let mut out = Vec::with_capacity(bytes.len());
    let mut copied = 0;
    for (start, name) in record_names(path, bytes.clone())? {
        let Some(to) = queue.get_mut(name.as_str()).and_then(VecDeque::pop_front) else {
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

/// Each record's name and the byte offset where it is written. FASTA names
/// come from the FASTA index, so they agree with how the file opens.
fn record_names(path: &Path, bytes: Vec<u8>) -> Result<Vec<(usize, String)>> {
    if crate::files::Format::from_path(path)? != crate::files::Format::GenBank {
        let fasta =
            basecraft_io::fasta::IndexedFasta::from_source(basecraft_io::Source::from_vec(bytes))?;
        return fasta
            .index
            .records
            .into_iter()
            .map(|r| Ok((offset(r.header_offset)? + 1, r.name)))
            .collect();
    }
    crate::files::locus_names(&bytes)
        .map(|(at, name)| {
            String::from_utf8(name.to_vec())
                .map(|name| (at, name))
                .map_err(|_| crate::error::invalid("record names must be UTF-8"))
        })
        .collect()
}

fn offset(at: u64) -> Result<usize> {
    usize::try_from(at).map_err(|_| crate::error::invalid("file too large"))
}
