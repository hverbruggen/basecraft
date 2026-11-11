//! Names are identity: renames carry their links, names stay unique, clade
//! labels know their group, and new workspaces link to the roster.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_docs::TreeDoc;
use basecraft_ops::renames::{self, RenameRefusal};
use basecraft_ops::{Session, metadata};
use basecraft_protocol::delimitation::{DelimitationWorkspace, Specimen};
use basecraft_protocol::metadata::{ChangeSet, RecordLink};
use basecraft_protocol::{CladeAnnotation, Op};
use std::collections::BTreeMap;
use std::path::Path;

fn project(root: &Path) -> (Session, basecraft_ops::DocId, basecraft_ops::DocId) {
    std::fs::write(root.join("aln.fasta"), ">a\nACGT\n>b\nACGA\n").unwrap();
    let mut session = Session::new(root);
    let meta = metadata::open(&mut session, true).unwrap().unwrap();
    metadata::apply_plan(
        &mut session,
        meta,
        ChangeSet {
            specimens: vec!["S1".into()],
            links: vec![RecordLink {
                document: "aln.fasta".into(),
                record: "a".into(),
                specimen: "S1".into(),
            }],
            ..ChangeSet::default()
        },
    )
    .unwrap();
    let aln = session.open(&root.join("aln.fasta")).unwrap();
    (session, aln, meta)
}

fn row_names(session: &Session, doc: basecraft_ops::DocId) -> Vec<String> {
    session
        .alignment(doc)
        .unwrap()
        .rows()
        .map(|r| r.name.clone())
        .collect()
}

fn linked(session: &Session, meta: basecraft_ops::DocId) -> Vec<String> {
    let mut records: Vec<_> = session
        .metadata(meta)
        .unwrap()
        .links
        .iter()
        .map(|l| l.record.clone())
        .collect();
    records.sort();
    records
}

#[test]
fn row_rename_moves_its_specimen_link_in_one_undo_step() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, aln, meta) = project(dir.path());
    let plan = renames::plan(&session, aln, "a", "a2").unwrap().unwrap();
    assert_eq!(plan.links.len(), 1);
    renames::apply(&mut session, plan).unwrap();
    assert_eq!(row_names(&session, aln), ["a2", "b"]);
    assert_eq!(linked(&session, meta), ["a2"]);
    session.undo(aln).unwrap();
    assert_eq!(row_names(&session, aln), ["a", "b"]);
    assert_eq!(linked(&session, meta), ["a"]);
    session.redo(aln).unwrap();
    assert_eq!(row_names(&session, aln), ["a2", "b"]);
    assert_eq!(linked(&session, meta), ["a2"]);
}

#[test]
fn renames_that_would_break_identity_are_refused() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, aln, meta) = project(dir.path());
    let refusal = |s: &Session, old, new| renames::plan(s, aln, old, new).unwrap().err();
    assert_eq!(
        refusal(&session, "a", "b"),
        Some(RenameRefusal::NameTaken { name: "b".into() })
    );
    assert_eq!(
        refusal(&session, "zz", "y"),
        Some(RenameRefusal::NotFound { name: "zz".into() })
    );
    // The edit itself refuses a duplicate too, whatever route it comes by.
    assert!(
        session
            .apply(
                aln,
                Op::AlnRenameRow {
                    row: 1,
                    name: "a".into()
                }
            )
            .is_err()
    );
    // Links in metadata that isn't open can't move with the rename.
    session.save(meta, None).unwrap();
    let mut other = Session::new(dir.path());
    let aln = other.open(&dir.path().join("aln.fasta")).unwrap();
    assert!(matches!(
        renames::plan(&other, aln, "a", "a2").unwrap(),
        Err(RenameRefusal::MetadataNotOpen { .. })
    ));
    // An unlinked row renames freely.
    let plan = renames::plan(&other, aln, "b", "b2").unwrap().unwrap();
    assert_eq!(plan.links, Vec::<RecordLink>::new());
    renames::apply(&mut other, plan).unwrap();
    assert_eq!(row_names(&other, aln), ["a", "b2"]);
}

#[test]
fn duplicate_names_are_offered_numbered_names() {
    let names: Vec<String> = ["a", "a", "a.1", "b"].map(String::from).into();
    let found = renames::duplicate_names(&names).unwrap();
    assert_eq!(found.names, ["a"]);
    assert_eq!(
        found.renames,
        [("a".into(), "a.2".into()), ("a".into(), "a.3".into())]
    );
    assert!(renames::duplicate_names(&["a".into(), "b".into()]).is_none());
}

#[test]
fn making_names_unique_is_one_undo_step() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dup.fasta"), ">a\nAC\n>a\nAG\n>b\nAT\n").unwrap();
    let mut session = Session::new(dir.path());
    let aln = session.open(&dir.path().join("dup.fasta")).unwrap();
    let found = renames::duplicates(&session, aln).unwrap().unwrap();
    renames::make_unique(&mut session, aln, &found).unwrap();
    assert_eq!(row_names(&session, aln), ["a.1", "a.2", "b"]);
    assert!(renames::duplicates(&session, aln).unwrap().is_none());
    session.undo(aln).unwrap();
    assert_eq!(row_names(&session, aln), ["a", "a", "b"]);
}

fn annotate(tree: &mut TreeDoc, id: u32, clade: &[&str]) {
    tree.apply_edit(&Op::TreeAnnotate {
        id,
        annotation: Some(CladeAnnotation::new(
            clade.iter().map(|t| (*t).to_owned()).collect(),
            "label",
        )),
    })
    .unwrap();
}

#[test]
fn a_reroot_that_moves_a_label_to_another_group_is_reported() {
    let mut tree = TreeDoc::from_newick("t", "((a,b),(c,d),e);").unwrap();
    annotate(&mut tree, 1, &["a", "b"]);
    annotate(&mut tree, 2, &["c", "d"]);
    assert_eq!(
        tree.annotations()[&1].tips,
        ["a", "b"].map(String::from).into()
    );
    tree.apply_edit(&Op::TreeRotate {
        clade: vec!["a".into(), "b".into()],
    })
    .unwrap();
    assert_eq!(tree.displaced_annotations(), Vec::<u32>::new());
    // Rooting on a puts a and b on either side of the root: the pair now
    // names the whole tree, not the group the label was made for.
    let undo = tree
        .apply_edit(&Op::TreeReroot {
            clade: vec!["a".into()],
        })
        .unwrap();
    assert_eq!(tree.displaced_annotations(), [1]);
    tree.undo_edit(&undo);
    assert_eq!(tree.displaced_annotations(), Vec::<u32>::new());
}

#[test]
fn a_tip_rename_carries_label_tip_sets() {
    let mut tree = TreeDoc::from_newick("t", "((a,b),(c,d),e);").unwrap();
    annotate(&mut tree, 1, &["a", "b"]);
    tree.apply_edit(&Op::TreeRenameTip {
        from: "a".into(),
        to: "a2".into(),
    })
    .unwrap();
    assert_eq!(
        tree.annotations()[&1].tips,
        ["a2", "b"].map(String::from).into()
    );
    assert_eq!(tree.displaced_annotations(), Vec::<u32>::new());
}

#[test]
fn a_new_workspace_links_specimens_through_record_links() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, _, meta) = project(dir.path());
    session.save(meta, None).unwrap();
    let specimen = |id: &str| Specimen {
        id: id.into(),
        label: id.into(),
        sources: [("aln.fasta".to_owned(), id.to_owned())].into(),
        attributes: BTreeMap::new(),
    };
    let mut work = DelimitationWorkspace {
        version: 1,
        name: "w".into(),
        specimens: vec![specimen("a"), specimen("b")],
        hypotheses: Vec::new(),
        view: basecraft_protocol::delimitation::DelimitationView::default(),
        exchange_fields: BTreeMap::new(),
    };
    let target = dir.path().join("w.spart");
    let run = basecraft_ops::records::ArtifactRun::create(&target, "roster").unwrap();
    let links = basecraft_ops::delimitation::link_roster(dir.path(), &run, &mut work);
    assert_eq!(links.linked, 1, "{links:?}");
    assert_eq!(links.unlinked, ["b"]);
    assert_eq!(
        work.specimens[0].attributes[basecraft_docs::delimitation::ROSTER_ID],
        "S1"
    );
    // Workspace ids stay as the SPART file has them.
    assert_eq!(work.specimens[0].id, "a");
}

#[test]
fn submission_rows_follow_a_renamed_alignment_row() {
    use basecraft_protocol::submission::{Decision, RowDecision, SubmissionEdit};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, aln, _) = project(dir.path());
    let submission = basecraft_ops::submission::create(
        &mut session,
        &dir.path().join("aln.submission.json"),
        &dir.path().join("aln.fasta"),
    )
    .unwrap();
    session
        .apply(
            submission,
            Op::SubmissionEdit {
                edit: Box::new(SubmissionEdit::SetRows {
                    rows: vec![RowDecision {
                        record: "a".into(),
                        decision: Decision::Existing {
                            accession: "AB123456".into(),
                        },
                    }],
                }),
            },
        )
        .unwrap();
    let rows = |s: &Session| -> Vec<String> {
        s.submission(submission)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.record.clone())
            .collect()
    };
    let plan = renames::plan(&session, aln, "a", "a2").unwrap().unwrap();
    assert_eq!(plan.submissions, [submission]);
    renames::apply(&mut session, plan).unwrap();
    assert_eq!(rows(&session), ["a2"]);
    session.undo(aln).unwrap();
    assert_eq!(rows(&session), ["a"]);
}

#[test]
fn duplicate_records_in_a_file_get_numbered_names_and_nothing_else_changes() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let fasta = dir.path().join("reads.fasta");
    std::fs::write(&fasta, ">x first read\nAC\n>x\nAG\n>y\nAT\n").unwrap();
    let found = renames::duplicate_records(&fasta).unwrap().unwrap();
    assert_eq!(found.names, ["x"]);
    // An open copy would write the old names back, so numbering waits.
    let mut session = Session::new(dir.path());
    session.open(&fasta).unwrap();
    assert!(renames::number_records(&session, &fasta, &found).is_err());
    let closed = Session::new(dir.path());
    renames::number_records(&closed, &fasta, &found).unwrap();
    assert_eq!(
        std::fs::read_to_string(&fasta).unwrap(),
        ">x.1 first read\nAC\n>x.2\nAG\n>y\nAT\n"
    );
    assert!(renames::duplicate_records(&fasta).unwrap().is_none());

    let genbank = dir.path().join("two.gb");
    let record = "LOCUS       ABC          4 bp    DNA\nORIGIN\n        1 acgt\n//\n";
    std::fs::write(&genbank, record.repeat(2)).unwrap();
    let found = renames::duplicate_records(&genbank).unwrap().unwrap();
    renames::number_records(&closed, &genbank, &found).unwrap();
    // The longer name takes two of the spaces after it, so columns stay put.
    assert_eq!(
        std::fs::read_to_string(&genbank).unwrap(),
        record.replace("ABC   ", "ABC.1 ") + &record.replace("ABC   ", "ABC.2 ")
    );
}
