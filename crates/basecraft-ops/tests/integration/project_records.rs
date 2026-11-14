//! Records about a user's data live in the project folder. Job records of
//! plugin runs (`state/jobs`, card j3sxh item 3) and `LocalOnly` history
//! (`state/provenance`, item 4) are not covered yet.
use crate::isolated_profile;
use basecraft_ops::{Session, project::Project, records};
use basecraft_protocol::Op;
use std::path::{Path, PathBuf};

fn edit(session: &mut Session, path: &Path, residue: &str) -> basecraft_ops::DocId {
    let doc = session.open(path).unwrap();
    session.apply(doc, Op::seq_replace(0, 1, residue)).unwrap();
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
fn deleting_private_folders_keeps_index_history_and_recovery() {
    let Some(root) = isolated_profile::enter() else {
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
    records::reference_input(
        &folder.join("results/tree.nwk"),
        &saved,
        &basecraft_io::hash_file(&saved).unwrap(),
        None,
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
    let outputs = records::recover(session.recovery_root(), &recovered, None).unwrap();
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
    let Some(root) = isolated_profile::enter() else {
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
            .apply(doc, Op::seq_replace(position, position + 1, "C"))
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

fn reference(anchor: &Path, source: &Path) -> records::ArtifactRun {
    records::reference_input(
        anchor,
        source,
        &basecraft_io::hash_file(source).unwrap(),
        None,
        &[],
        "analysis",
        None,
    )
    .unwrap()
}

/// `root/data` for inputs and `root/results` for results.
fn data_and_results(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("results")).unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    root.join("data")
}

fn index_files(folder: &Path) -> std::collections::BTreeSet<String> {
    let mut all = std::collections::BTreeSet::new();
    for path in files(&folder.join(".basecraft/analysis-index")) {
        let index: basecraft_protocol::records::AnalysisIndex =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        all.extend(index.files);
    }
    all
}

#[test]
fn crashed_loose_session_is_found_and_offered() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("loose");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let mut crashed = Session::recoverable(&folder).unwrap();
    edit(&mut crashed, &file, "G");
    let recovery = crashed.recovery_root().to_path_buf();
    // A crash: the session never closes.
    std::mem::forget(crashed);
    let found = records::recovery_sessions(std::slice::from_ref(&folder)).unwrap();
    assert_eq!(found, std::slice::from_ref(&recovery));
    let mut session = Session::recoverable(&folder).unwrap();
    let doc = session.open(&file).unwrap();
    session
        .find_earlier_recovery(doc, std::slice::from_ref(&folder))
        .unwrap();
    let notice = session.recovery_warning(doc).unwrap();
    assert!(notice.contains(&recovery.display().to_string()), "{notice}");
    let other = folder.join("other.fasta");
    std::fs::write(&other, b">a\nACGT\n").unwrap();
    let doc = session.open(&other).unwrap();
    session.find_earlier_recovery(doc, &[folder]).unwrap();
    assert!(session.recovery_warning(doc).is_none());
}

#[test]
fn two_profiles_add_to_one_index_at_once() {
    let name = isolated_profile::test_name();
    if let Some(who) = std::env::var_os("BASECRAFT_INDEX_WRITER") {
        let data = PathBuf::from(std::env::var_os("BASECRAFT_INDEX_DATA").unwrap());
        for i in 0..20 {
            let source = data.join(format!("{}-{i}.fasta", who.to_string_lossy()));
            std::fs::write(&source, b">a\nACGT\n").unwrap();
            reference(&data.parent().unwrap().join("results/out.txt"), &source);
        }
        return;
    }
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let data = data_and_results(&root);
    // Both writers add to one store, so they write the same index file.
    records::ArtifactRun::create(None, &root.join("results/out.txt"), "first").unwrap();
    let writers: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|who| {
            let profile = root.join(format!("profile-{who}"));
            isolated_profile::relaunch(&name)
                .env("BASECRAFT_INDEX_WRITER", who)
                .env("BASECRAFT_INDEX_DATA", &data)
                .env("BASECRAFT_PROFILE_DIR", &profile)
                .env("BASECRAFT_STATE_DIR", profile.join("state"))
                .env("BASECRAFT_CACHE_DIR", profile.join("cache"))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for writer in writers {
        isolated_profile::assert_one_passed(&name, &writer.wait_with_output().unwrap());
    }
    assert_eq!(index_files(&data).len(), 40);
    for entry in std::fs::read_dir(&data).unwrap().flatten() {
        if entry.path().extension().is_some_and(|e| e == "fasta") {
            let check = records::check(&entry.path()).unwrap();
            assert_eq!(check.conflict.analyses.len(), 1, "{:?}", entry.path());
        }
    }
}

#[test]
fn a_sync_conflict_copy_of_the_index_still_counts() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let data = data_and_results(&root);
    let source = data.join("reads.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let run = reference(&root.join("results/out.txt"), &source);
    // Another machine's version wins the name; ours becomes a conflict copy.
    let shards = data.join(".basecraft/analysis-index");
    let shard = files(&shards).pop().unwrap();
    let copy = shard.with_file_name(
        shard
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".json", ".sync-conflict-20261113-101010-ABCDEFG.json"),
    );
    std::fs::rename(&shard, &copy).unwrap();
    let mut theirs: basecraft_protocol::records::AnalysisIndex =
        serde_json::from_slice(&std::fs::read(&copy).unwrap()).unwrap();
    theirs.files = ["their.fasta".into()].into();
    std::fs::write(
        &shard,
        basecraft_protocol::records::encode(&theirs).unwrap(),
    )
    .unwrap();
    let check = records::check(&source).unwrap();
    assert!(check.is_complete(), "{:?}", check.issues);
    assert_eq!(check.conflict.analyses[0].run, run.directory());
}

#[test]
fn a_lost_index_makes_a_project_save_ask_until_reindexed() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("project");
    std::fs::create_dir_all(folder.join("data")).unwrap();
    let source = folder.join("data/reads.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    // The analysis keeps its run in the project's own store.
    let project = Project::new(&folder).unwrap();
    let run = records::reference_input(
        &folder.join("summary.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        None,
        &[],
        "analysis",
        Some(&project),
    )
    .unwrap();
    std::fs::remove_dir_all(folder.join(".basecraft/analysis-index")).unwrap();
    let mut session = Session::new(&folder).in_project(Some(project));
    let doc = edit(&mut session, &source, "T");
    let check = match session.save(doc, None).unwrap_err() {
        basecraft_ops::Error::SaveCheckRequired(check) => check,
        other => panic!("{other:?}"),
    };
    assert!(!check.evidence().is_complete());
    assert_eq!(check.evidence().conflict.analyses[0].run, run.directory());
    assert_eq!(std::fs::read(&source).unwrap(), b">a\nACGT\n");
    let done = records::reindex(&folder).unwrap();
    assert_eq!((done.stores, done.folders), (1, 1));
    let check = match session.save(doc, None).unwrap_err() {
        basecraft_ops::Error::EvidenceConflict(conflict) => conflict,
        other => panic!("{other:?}"),
    };
    assert_eq!(check.analyses[0].run, run.directory());
}

#[test]
fn a_foreign_basecraft_file_beside_an_input_is_not_skipped() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let data = data_and_results(&root);
    std::fs::write(data.join(".basecraft"), b"another program").unwrap();
    let source = data.join("reads.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let error = records::reference_input(
        &root.join("results/out.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        None,
        &[],
        "analysis",
        None,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(&data.display().to_string()),
        "{error}"
    );
    assert!(!records::check(&source).unwrap().is_complete());
}

#[test]
fn a_project_keeps_one_index_and_a_loose_input_is_indexed_beside_it() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("project");
    for sub in ["project/data/deep", "project/results", "outside"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    let inside = folder.join("data/deep/reads.fasta");
    let outside = root.join("outside/other.fasta");
    for path in [&inside, &outside] {
        std::fs::write(path, b">a\nACGT\n").unwrap();
    }
    let project = Project::new(&folder).unwrap();
    for source in [&inside, &outside] {
        records::reference_input(
            &folder.join("results/out.txt"),
            source,
            &basecraft_io::hash_file(source).unwrap(),
            None,
            &[],
            "analysis",
            Some(&project),
        )
        .unwrap();
    }
    // One index, at the project's root, keyed by project-relative path.
    assert_eq!(
        index_files(&folder),
        ["data/deep/reads.fasta".to_owned()].into()
    );
    assert!(!folder.join("data/.basecraft").exists());
    assert!(!folder.join("data/deep/.basecraft").exists());
    // The file outside the project is indexed beside it.
    assert_eq!(
        index_files(&root.join("outside")),
        ["other.fasta".to_owned()].into()
    );
    // A save from the project session finds the analysis.
    let mut session = Session::new(&folder).in_project(Some(project));
    let doc = edit(&mut session, &inside, "T");
    assert!(matches!(
        session.save(doc, None),
        Err(basecraft_ops::Error::EvidenceConflict(_))
    ));
}

#[test]
fn the_first_edit_in_a_folder_that_cannot_hold_recovery_is_refused() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("shared");
    std::fs::create_dir(&folder).unwrap();
    // Another program's `.basecraft`, which basecraft must not claim.
    std::fs::write(folder.join(".basecraft"), b"another program").unwrap();
    let file = folder.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let mut session = Session::recoverable(&folder).unwrap();
    let doc = session.open(&file).unwrap();
    let error = session.apply(doc, Op::seq_replace(0, 1, "G")).unwrap_err();
    let basecraft_ops::Error::ReadOnlyFolder { folder: named, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(named, &folder);
    assert_eq!(error.code(), "read_only_folder");
    assert!(error.to_string().contains("Copy the file"), "{error}");
    // The document is unedited and nothing was written.
    assert!(!session.can_undo(doc).unwrap());
    assert_eq!(session.doc(doc).unwrap().read(0, 4), b"ACGT");
    assert_eq!(
        std::fs::read(folder.join(".basecraft")).unwrap(),
        b"another program"
    );
    drop(session);
    assert_eq!(std::fs::read(&file).unwrap(), b">a\nACGT\n");
}

#[test]
fn a_hard_link_in_a_project_subfolder_is_still_matched() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("project");
    std::fs::create_dir_all(folder.join("data")).unwrap();
    let source = folder.join("data/reads.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let project = Project::new(&folder).unwrap();
    let run = records::reference_input(
        &folder.join("summary.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        None,
        &[],
        "analysis",
        Some(&project),
    )
    .unwrap();
    let alias = folder.join("data/alias.fasta");
    std::fs::hard_link(&source, &alias).unwrap();
    let mut session = Session::new(&folder).in_project(Some(project));
    let doc = edit(&mut session, &alias, "T");
    let conflict = match session.save(doc, None).unwrap_err() {
        basecraft_ops::Error::EvidenceConflict(conflict) => conflict,
        other => panic!("{other:?}"),
    };
    assert_eq!(conflict.analyses[0].run, run.directory());
    assert_eq!(std::fs::read(&source).unwrap(), b">a\nACGT\n");
}

#[cfg(unix)]
#[test]
fn a_symlinked_basecraft_is_named_as_a_link() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("linked");
    std::fs::create_dir_all(root.join("elsewhere")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), folder.join(".basecraft")).unwrap();
    let file = folder.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let mut session = Session::recoverable(&folder).unwrap();
    let doc = session.open(&file).unwrap();
    let error = session.apply(doc, Op::seq_replace(0, 1, "G")).unwrap_err();
    assert!(error.to_string().contains("symbolic link"), "{error}");
    assert_eq!(
        std::fs::read_dir(root.join("elsewhere")).unwrap().count(),
        0
    );
}
