//! D23: a document's companion lives in the project's `.basecraft`, keyed by
//! the document's project-relative path, or in the `.basecraft` beside a
//! loose file. Nothing is written beside the document, and Save As carries
//! the companion to the new name.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use std::path::{Path, PathBuf};

use basecraft_ops::{DocId, Session, files, project::Project};
use basecraft_protocol::Op;
use basecraft_protocol::facts::{Alphabet, BranchLengths, StatedAlphabet};

/// The four kinds that keep facts in a companion: file name, contents, and
/// the edit that gives it a stated fact (a species workspace always has one).
fn kinds() -> Vec<(&'static str, String, Option<Op>)> {
    vec![
        (
            "tree.nwk",
            "((a:1,b:1):1,c:2);\n".into(),
            Some(Op::TreeSetBranchLengths {
                meaning: BranchLengths::Substitutions,
            }),
        ),
        (
            "species.spart",
            include_str!("../../../testdata/fixtures/delimitation/manual.spart").into(),
            None,
        ),
        (
            "genes.afa",
            ">a\nACGT-A\n>b\nACGTTA\n".into(),
            Some(Op::SetAlphabet {
                alphabet: StatedAlphabet::Rna,
            }),
        ),
        (
            "gene.fasta",
            ">g\nACGTACGT\n".into(),
            Some(Op::SetAlphabet {
                alphabet: StatedAlphabet::Rna,
            }),
        ),
    ]
}

/// The fact the edit stated, read back from a reopened document.
fn stated(session: &Session, doc: DocId, name: &str) -> String {
    match name {
        "tree.nwk" => format!("{:?}", session.tree(doc).unwrap().branch_lengths()),
        "species.spart" => format!("{:?}", session.delimitation(doc).unwrap().hypotheses.len()),
        "genes.afa" => format!("{:?}", session.alignment(doc).unwrap().stated_alphabet()),
        _ => format!("{:?}", session.doc(doc).unwrap().stated_alphabet()),
    }
}

/// Every file name under `folder`, outside any `.basecraft`.
fn visible(folder: &Path) -> Vec<String> {
    let mut names: Vec<_> = walk(folder)
        .into_iter()
        .filter(|path| !path.components().any(|c| c.as_os_str() == ".basecraft"))
        .map(|path| path.strip_prefix(folder).unwrap().display().to_string())
        .collect();
    names.sort();
    names
}

fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Save each kind with a stated fact, check where its companion went and
/// that nothing sits beside it, reopen it, then Save As and reopen the copy.
fn round_trip(project: Option<&Project>, folder: &Path, records: &Path, key: &str) {
    for (name, contents, edit) in kinds() {
        let path = folder.join(name);
        std::fs::write(&path, &contents).unwrap();
        let session = || Session::new(folder).in_project(project.cloned());
        let mut first = session();
        let doc = first.open(&path).unwrap();
        if let Some(edit) = edit.clone() {
            first.apply(doc, edit).unwrap();
        }
        first.save(doc, None).unwrap();
        let fact = stated(&first, doc, name);
        let copy = folder.join(format!("copy-{name}"));
        // A species workspace keeps its companion with the analysis run its
        // save binds it to; that placement is the analysis store's.
        if name != "species.spart" {
            let companion = records.join(format!("{key}{name}.json"));
            assert!(companion.is_file(), "{name}: no {}", companion.display());
            assert_eq!(files::companion_path(project, &path), companion);
        }
        first.save(doc, Some(&copy)).unwrap();
        if name != "species.spart" {
            assert!(records.join(format!("{key}copy-{name}.json")).is_file());
        }
        drop(first);

        let mut reopened = session();
        for file in [&path, &copy] {
            let doc = reopened.open(file).unwrap();
            assert_eq!(stated(&reopened, doc, name), fact, "{}", file.display());
        }
    }
    // No companion sits beside any document. (Provenance bundles, alignment
    // links and FASTA's GFF3 are other records, outside this check.)
    let beside: Vec<_> = visible(folder)
        .into_iter()
        .filter(|name| name.ends_with(".basecraft.json") || name.ends_with(".nwk.json"))
        .collect();
    assert_eq!(
        beside,
        Vec::<String>::new(),
        "a companion sits beside a document"
    );
}

#[test]
fn a_loose_file_keeps_its_companion_in_the_basecraft_beside_it() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("loose");
    std::fs::create_dir(&folder).unwrap();
    let records = folder.canonicalize().unwrap().join(".basecraft/documents");
    round_trip(None, &folder.canonicalize().unwrap(), &records, "");
}

#[test]
fn a_project_file_keeps_its_companion_in_the_project_by_relative_path() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let folder = root.join("analysis/run1");
    std::fs::create_dir_all(&folder).unwrap();
    let project = Project::new(&root).unwrap();
    let records = project.root().join(".basecraft/documents");
    round_trip(
        Some(&project),
        &folder.canonicalize().unwrap(),
        &records,
        "analysis/run1/",
    );
    // The subfolder keeps no companions of its own.
    assert!(!folder.join(".basecraft/documents").exists());
}

#[test]
fn a_stated_alphabet_is_read_from_the_project_record_only() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    let project = Project::new(&root).unwrap();
    let path = project.root().join("seqs/gene.fasta");
    std::fs::write(&path, ">g\nACGTACGT\n").unwrap();
    let mut session = Session::new(project.root()).in_project(Some(project.clone()));
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: StatedAlphabet::Rna,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    drop(session);
    // Opened as a loose file, the project's record is not found.
    let mut loose = Session::new(project.root());
    let doc = loose.open(&path).unwrap();
    assert_eq!(loose.doc(doc).unwrap().stated_alphabet(), None);
    let mut inside = Session::new(project.root()).in_project(Some(project));
    let doc = inside.open(&path).unwrap();
    assert_eq!(inside.doc(doc).unwrap().alphabet().value, Alphabet::Rna);
}

#[test]
fn copying_a_document_inside_basecraft_carries_its_companion() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap();
    let path = folder.join("tree.nwk");
    std::fs::write(&path, "((a:1,b:1):1,c:2);\n").unwrap();
    let mut session = Session::new(&folder);
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::TreeSetBranchLengths {
                meaning: BranchLengths::Substitutions,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    std::fs::create_dir(folder.join("elsewhere")).unwrap();
    let copy = folder.join("elsewhere/tree.nwk");
    files::copy_document(None, &path, &copy).unwrap();
    assert!(
        folder
            .join("elsewhere/.basecraft/documents/tree.nwk.json")
            .is_file()
    );
    let tree = files::load_tree(&copy).unwrap().doc;
    assert_eq!(
        tree.branch_lengths().map(|fact| &fact.value),
        Some(&BranchLengths::Substitutions)
    );
    // A second copy onto the same name is refused.
    assert!(files::copy_document(None, &path, &copy).is_err());
}
