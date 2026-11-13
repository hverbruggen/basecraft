//! Everything about a user's data lives in the project folder: deleting
//! basecraft's private folders loses no record, history or recovery.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, project::Project, records};
use basecraft_protocol::Op;
use std::path::{Path, PathBuf};

fn edit(session: &mut Session, path: &Path, residue: &str) -> basecraft_ops::DocId {
    let doc = session.open(path).unwrap();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 0,
                end: 1,
                residues: residue.into(),
            },
        )
        .unwrap();
    doc
}

fn files(folder: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.file_type().unwrap().is_dir() {
            all.extend(files(&entry.path()));
        } else {
            all.push(entry.path());
        }
    }
    all
}

#[test]
fn deleting_private_folders_loses_no_record() {
    let Some(root) = isolated_profile::enter("deleting_private_folders_loses_no_record") else {
        return;
    };
    let folder = root.join("project");
    std::fs::create_dir_all(folder.join("results")).unwrap();
    let saved = folder.join("saved.fasta");
    let unsaved = folder.join("unsaved.fasta");
    for path in [&saved, &unsaved] {
        std::fs::write(path, b">a\nACGT\n").unwrap();
    }
    let project = Project::new(&folder).unwrap();
    let mut session = Session::recoverable(&folder)
        .unwrap()
        .in_project(Some(project.clone()));
    let doc = edit(&mut session, &saved, "T");
    session.save(doc, None).unwrap();
    records::reference_input_with_dependencies(
        &folder.join("results/tree.nwk"),
        &saved,
        &basecraft_io::hash_file(&saved).unwrap(),
        &[],
        "analysis",
        Some(&project),
    )
    .unwrap();
    edit(&mut session, &unsaved, "G");
    assert!(
        session
            .recovery_root()
            .starts_with(folder.join(".basecraft"))
    );

    let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
    for private in [&paths.state, &paths.cache, &paths.data] {
        let _ = std::fs::remove_dir_all(private);
    }

    // The analysis still protects its input.
    let check = records::check(&saved).unwrap();
    assert!(check.is_complete(), "{:?}", check.issues);
    assert_eq!(check.conflict.analyses.len(), 1);
    // The saved document's history is in the project.
    let bundle = records::bundle_for_result(&saved, Some(&project));
    assert!(bundle.starts_with(&folder), "{bundle:?}");
    assert!(bundle.join("manifest.json").is_file());
    // The unsaved edit is recovered from the project.
    let recovered = root.join("recovered");
    let outputs = records::recover(session.recovery_root(), &recovered).unwrap();
    let unsaved_copy = outputs
        .iter()
        .find(|p| p.ends_with("unsaved.fasta"))
        .unwrap();
    assert_eq!(std::fs::read(unsaved_copy).unwrap(), b">a\nGCGT\n");
    // Basecraft wrote nothing but lock files to its own folders meanwhile.
    for file in files(&paths.state).iter().chain(&files(&paths.cache)) {
        assert!(
            file.parent().is_some_and(|p| p.ends_with("locks")),
            "{file:?}"
        );
    }
}

#[test]
fn recovery_writes_edits_not_copies() {
    let Some(root) = isolated_profile::enter("recovery_writes_edits_not_copies") else {
        return;
    };
    // A loose 1 MB file: recovery goes beside it, only once there is an edit.
    let folder = root.join("loose");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("large.fasta");
    let mut bytes = b">a\n".to_vec();
    bytes.extend(std::iter::repeat_n(b'A', 1 << 20));
    bytes.push(b'\n');
    std::fs::write(&file, &bytes).unwrap();
    let mut session = Session::recoverable(&folder).unwrap();
    let doc = session.open(&file).unwrap();
    assert!(!folder.join(".basecraft").exists());
    for position in 0..20 {
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: position,
                    end: position + 1,
                    residues: "C".into(),
                },
            )
            .unwrap();
    }
    let recovery = session.recovery_root().to_path_buf();
    assert!(recovery.starts_with(folder.join(".basecraft/recovery")));
    let written: u64 = files(&recovery)
        .iter()
        .map(|path| std::fs::metadata(path).unwrap().len())
        .sum();
    assert!(written < 64 * 1024, "recovery wrote {written} bytes");
    // Saving leaves nothing to recover; closing removes it.
    session.save(doc, None).unwrap();
    drop(session);
    assert!(!folder.join(".basecraft/recovery").exists());
}
