//! The project is the folder the user chose: membership, provenance placement
//! and analysis folders never change it.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, project::Project, records};
use basecraft_protocol::Op;
use std::path::Path;

fn edit_and_save(session: &mut Session, file: &Path) {
    let doc = session.open(file).unwrap();
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.save(doc, None).unwrap();
}

#[test]
fn files_inside_and_outside_the_chosen_project() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir_all(chosen.join("data")).unwrap();
    let inside = chosen.join("data/inside.fasta");
    let outside = root.path().join("outside.fasta");
    for file in [&inside, &outside] {
        std::fs::write(file, b">a\nACGT\n").unwrap();
    }
    let project = Project::new(&chosen).unwrap();
    let mut session = Session::new(&chosen).in_project(Some(project.clone()));
    edit_and_save(&mut session, &inside);
    edit_and_save(&mut session, &outside);

    let in_project = project.provenance_bundle(&inside).unwrap();
    assert!(in_project.join("manifest.json").is_file());
    assert!(!records::companion(&inside).exists());
    assert!(records::companion(&outside).join("manifest.json").is_file());
    assert_eq!(
        records::verify(&inside, Some(&project)).unwrap(),
        Vec::<String>::new()
    );

    // An analysis folder in a subfolder makes no new project.
    records::ArtifactRun::create(&chosen.join("data/result.txt"), "analysis").unwrap();
    assert!(chosen.join("data/.basecraft").is_dir());
    assert_eq!(session.project(), Some(&project));
    edit_and_save(&mut session, &inside);
    assert_eq!(
        records::bundle_for_result(&inside, Some(&project)),
        in_project
    );
    // Without a project every file is loose.
    let mut loose = Session::new(root.path());
    assert!(loose.project().is_none());
    let other = root.path().join("other.fasta");
    std::fs::write(&other, b">a\nACGT\n").unwrap();
    edit_and_save(&mut loose, &other);
    assert!(records::companion(&other).join("manifest.json").is_file());
}

#[test]
fn a_project_file_packages_with_its_history() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir(&chosen).unwrap();
    let file = chosen.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let project = Project::new(&chosen).unwrap();
    let mut session = Session::new(&chosen).in_project(Some(project.clone()));
    edit_and_save(&mut session, &file);
    let package = root.path().join("package");
    records::package(&file, &package, Some(&project)).unwrap();
    let copied = package.join("gene.fasta");
    assert_eq!(
        records::verify(&copied, None).unwrap(),
        Vec::<String>::new()
    );
    // Without the project, the reader says where else to look.
    let error = records::inspect(&file, None).unwrap_err().to_string();
    assert!(error.contains("--project"), "{error}");
}

#[test]
fn local_only_history_of_a_project_file_is_found() {
    use basecraft_platform::settings::{Change, ProvenancePolicy, SettingsStore};
    crate::isolated_profile::isolate!();
    let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
    SettingsStore::new(paths.preferences())
        .update(Change::Provenance(ProvenancePolicy::LocalOnly))
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir(&chosen).unwrap();
    let file = chosen.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let project = Project::new(&chosen).unwrap();
    let mut session = Session::recoverable(&chosen)
        .unwrap()
        .in_project(Some(project.clone()));
    edit_and_save(&mut session, &file);
    assert!(!project.provenance_bundle(&file).unwrap().exists());
    let publication = records::inspect(&file, Some(&project)).unwrap();
    assert_ne!(publication.events.len(), 0);
    assert_eq!(
        records::verify(&file, Some(&project)).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_referenced_input_inside_the_project_keeps_its_history_link() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir(&chosen).unwrap();
    let first = chosen.join("first.fasta");
    std::fs::write(&first, b">a\nACGT\n").unwrap();
    let project = Project::new(&chosen).unwrap();
    let mut session = Session::new(&chosen).in_project(Some(project.clone()));
    edit_and_save(&mut session, &first);
    let bundle = project.provenance_bundle(&first).unwrap();
    let hash = basecraft_io::hash_file(&first).unwrap();

    let run = records::reference_input_with_dependencies(
        &chosen.join("second.fasta"),
        &first,
        &hash,
        &[],
        "second analysis",
        Some(&project),
    )
    .unwrap();
    let upstream = records::read_input(run.directory()).unwrap().upstream;
    assert_eq!(upstream.len(), 1, "{upstream:?}");
    assert_eq!(
        upstream[0].bundle.file_name(),
        bundle.file_name(),
        "the second run links the first run's history"
    );
}
