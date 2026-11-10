//! Exercise the actual retention owner; hooks exist only for one test invocation.
use super::*;
use std::cell::RefCell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    BeforeRead,
    AfterRead,
}
type Hook = Box<dyn FnMut(Stage, &Path)>;
thread_local! {
    static HOOK: RefCell<Option<Hook>> = RefCell::new(None);
}
pub(super) fn boundary(stage: Stage, path: &Path) {
    HOOK.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook(stage, path);
        }
    });
}
struct HookGuard;
impl Drop for HookGuard {
    fn drop(&mut self) {
        HOOK.with_borrow_mut(|hook| *hook = None);
    }
}
fn hook(f: impl FnMut(Stage, &Path) + 'static) -> HookGuard {
    HOOK.with_borrow_mut(|hook| {
        assert!(hook.is_none());
        *hook = Some(Box::new(f));
    });
    HookGuard
}
fn edit() -> Op {
    Op::SeqReplace {
        start: 0,
        end: 1,
        residues: "G".into(),
    }
}
const MAIN: &[u8] = b">a description\nACGT\n>b retained\nTTAA\n";
const GFF: &[u8] = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1\n";
fn fixture(root: &Path) -> (Session, DocId, PathBuf) {
    std::fs::create_dir_all(root).unwrap();
    let path = root.join("records.fasta");
    std::fs::write(&path, MAIN).unwrap();
    std::fs::write(files::gff_sidecar(&path), GFF).unwrap();
    let mut session = Session::new(root);
    let id = session.open(&path).unwrap();
    (session, id, path)
}
fn object(session: &Session, bytes: &[u8]) -> PathBuf {
    objects_dir(&session.storage_root)
        .join(basecraft_io::hash_bytes(bytes).trim_start_matches("b3:"))
}
fn unchanged(session: &Session, id: DocId, before: &basecraft_io::genbank::Record) {
    assert_eq!(&session.doc(id).unwrap().to_genbank(), before);
    let slot = &session.docs[id.0];
    assert!(slot.baseline.is_some());
    assert!(slot.undo.is_empty());
    assert!(slot.redo.is_empty());
    assert_eq!(slot.generation, 0);
    assert!(session.events.is_empty());
    assert!(session.pending.is_none());
}

#[test]
fn current_source_is_verified_even_with_a_known_object() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::current_source_is_verified_even_with_a_known_object",
    ) else {
        return;
    };
    for known in [false, true] {
        for companion in [false, true] {
            for mutation in 0..if cfg!(unix) { 5 } else { 4 } {
                let dir = root.join(format!("{known}-{companion}-{mutation}"));
                let (mut session, id, main) = fixture(&dir);
                if known {
                    session.snapshot_bytes(MAIN).unwrap();
                    session.snapshot_bytes(GFF).unwrap();
                }
                let path = if companion {
                    files::gff_sidecar(&main)
                } else {
                    main
                };
                let before = session.doc(id).unwrap().to_genbank();
                let original = std::fs::read(&path).unwrap();
                let modified = path.metadata().unwrap().modified().unwrap();
                match mutation {
                    0 | 1 => {
                        let mut changed = original.clone();
                        changed[0] = b'!';
                        if mutation == 0 {
                            std::fs::write(&path, changed).unwrap();
                            std::fs::File::options()
                                .write(true)
                                .open(&path)
                                .unwrap()
                                .set_modified(modified)
                                .unwrap();
                        } else {
                            let replacement = path.with_extension("replacement");
                            std::fs::write(&replacement, changed).unwrap();
                            std::fs::rename(replacement, &path).unwrap();
                        }
                    }
                    2 => std::fs::remove_file(&path).unwrap(),
                    3 => {
                        std::fs::remove_file(&path).unwrap();
                        std::fs::create_dir(&path).unwrap();
                    }
                    _ => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0))
                                .unwrap();
                        }
                    }
                }
                let err = session.apply(id, edit()).unwrap_err();
                if mutation < 2 {
                    assert!(matches!(err, Error::Invalid(_)));
                    assert_eq!(
                        err.to_string(),
                        "invalid operation: source changed since opening; reopen it before editing"
                    );
                } else {
                    assert!(
                        matches!(err, Error::Format(basecraft_io::Error::Io(_))),
                        "{err:?}"
                    );
                }
                unchanged(&session, id, &before);
                if companion || known {
                    assert_eq!(std::fs::read(object(&session, MAIN)).unwrap(), MAIN);
                }
                assert!(!dir.join("recovery.json").exists());
                assert!(!crate::log::log_dir(&dir).exists());
                #[cfg(unix)]
                if mutation == 4 {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
                }
            }
        }
    }
}

#[test]
fn acquisition_boundary_keeps_verified_bytes_not_later_path_contents() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::acquisition_boundary_keeps_verified_bytes_not_later_path_contents",
    ) else {
        return;
    };
    for stage in [Stage::BeforeRead, Stage::AfterRead] {
        let dir = root.join(if stage == Stage::BeforeRead {
            "before"
        } else {
            "after"
        });
        let (mut session, id, main) = fixture(&dir);
        let before = session.doc(id).unwrap().to_genbank();
        let target = main.clone();
        let _guard = hook(move |at, path| {
            if at == stage && path == target {
                std::fs::write(path, b">changed\nTTTT\n").unwrap();
            }
        });
        let result = session.apply(id, edit());
        if stage == Stage::BeforeRead {
            assert!(matches!(result, Err(Error::Invalid(_))));
            unchanged(&session, id, &before);
            assert!(!object(&session, MAIN).exists());
        } else {
            result.unwrap();
            assert_eq!(std::fs::read(object(&session, MAIN)).unwrap(), MAIN);
            assert_eq!(std::fs::read(object(&session, GFF)).unwrap(), GFF);
            assert!(session.save(id, None).is_err());
        }
    }
}

#[test]
fn real_write_failures_preserve_baseline_and_retry_once() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::real_write_failures_preserve_baseline_and_retry_once",
    ) else {
        return;
    };
    for failure in 0..4 {
        let dir = root.join(format!("failure-{failure}"));
        let (mut session, id, _) = fixture(&dir);
        let before = session.doc(id).unwrap().to_genbank();
        let blocked = match failure {
            0 => Some(objects_dir(&session.storage_root)),
            1 => Some(dir.join("recovery.json")),
            2 => Some(crate::log::log_dir(&dir)),
            _ => None,
        };
        if let Some(path) = &blocked {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if failure == 1 {
                std::fs::create_dir(path).unwrap();
            } else {
                std::fs::write(path, b"preserve obstruction").unwrap();
            }
        } else {
            session.log.as_mut().unwrap().faults.after_bytes = Some(17);
        }
        let err = session.apply(id, edit()).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Io(_) | Error::Format(basecraft_io::Error::Io(_))
            ),
            "{err:?}"
        );
        unchanged(&session, id, &before);
        if failure != 0 {
            assert_eq!(std::fs::read(object(&session, MAIN)).unwrap(), MAIN);
            assert_eq!(std::fs::read(object(&session, GFF)).unwrap(), GFF);
        }
        if let Some(path) = &blocked {
            if failure == 1 {
                std::fs::remove_dir(path).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), b"preserve obstruction");
                std::fs::remove_file(path).unwrap();
            }
        }
        session.apply(id, edit()).unwrap();
        session.flush().unwrap();
        assert_eq!(session.docs[id.0].undo.len(), 1);
        assert!(session.docs[id.0].baseline.is_none());
        assert!(crate::replay::replay(&dir).unwrap().ok());
    }
}

#[test]
fn actual_limit_growth_and_changed_oversize_precedence() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::actual_limit_growth_and_changed_oversize_precedence",
    ) else {
        return;
    };
    let session = Session::new(&root);
    let path = root.join("input");
    for size in [
        OBJECT_STORE_LIMIT - 1,
        OBJECT_STORE_LIMIT,
        OBJECT_STORE_LIMIT + 1,
    ] {
        let bytes = vec![b'A'; usize::try_from(size).unwrap()];
        std::fs::write(&path, &bytes).unwrap();
        let hash = basecraft_io::hash_bytes(&bytes);
        let result = session.store_object(&path, &hash);
        if size <= OBJECT_STORE_LIMIT {
            result.unwrap();
            assert_eq!(std::fs::read(object(&session, &bytes)).unwrap(), bytes);
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .starts_with("invalid operation: original input is ")
            );
            assert!(!object(&session, &bytes).exists());
            assert_eq!(
                session
                    .store_object(&path, "b3:changed")
                    .unwrap_err()
                    .to_string(),
                "invalid operation: source changed since opening; reopen it before editing"
            );
            // The hint sees a tiny source, acquisition sees >limit accepted bytes,
            // and the post-read metadata sees a tiny source again. Actual length wins.
            std::fs::write(&path, b"small").unwrap();
            let _guard = hook(move |stage, path| {
                if stage == Stage::BeforeRead {
                    std::fs::write(path, &bytes).unwrap();
                } else {
                    std::fs::write(path, b"small").unwrap();
                }
            });
            assert!(
                session
                    .store_object(&path, &hash)
                    .unwrap_err()
                    .to_string()
                    .starts_with("invalid operation: original input is ")
            );
            assert!(
                !objects_dir(&root)
                    .join(hash.trim_start_matches("b3:"))
                    .exists()
            );
        }
    }
}

#[test]
fn retention_modes_and_known_objects_keep_their_read_counts() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::retention_modes_and_known_objects_keep_their_read_counts",
    ) else {
        return;
    };
    for mode in 0..5 {
        let dir = root.join(format!("mode-{mode}"));
        let (mut session, id, main) = fixture(&dir);
        if mode == 1 {
            session.snapshot_bytes(MAIN).unwrap();
            session.snapshot_bytes(GFF).unwrap();
        } else if mode >= 2 {
            session = match mode {
                2 => Session::detached(&dir),
                3 => {
                    let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
                        (key == "BASECRAFT_PROFILE_DIR")
                            .then(|| dir.join("profile").into_os_string())
                    })
                    .unwrap();
                    basecraft_platform::settings::SettingsStore::new(paths.preferences())
                        .update(basecraft_platform::settings::Change::Recovery(false))
                        .unwrap();
                    Session::recoverable_with_paths(&dir, &paths).unwrap()
                }
                _ => Session::unlogged(&dir),
            };
            assert_eq!(session.open(&main).unwrap(), id);
        }
        #[cfg(feature = "test-counters")]
        basecraft_io::counters::reset();
        session.apply(id, edit()).unwrap();
        #[cfg(feature = "test-counters")]
        {
            let counts = basecraft_io::counters::snapshot();
            let expected = match mode {
                0 | 1 => 1,
                2 | 3 => 2,
                _ => 0,
            };
            for (path, bytes) in [(main.clone(), MAIN), (files::gff_sidecar(&main), GFF)] {
                let reads = counts
                    .reads
                    .get(&path.canonicalize().unwrap())
                    .copied()
                    .unwrap_or_default();
                assert_eq!(reads.full_reads, expected);
                assert_eq!(reads.bytes, expected * bytes.len() as u64);
            }
            assert_eq!(counts.unresolved_reads, 0);
        }
        let _ = main;
        if mode < 2 {
            assert_eq!(std::fs::read(object(&session, MAIN)).unwrap(), MAIN);
        } else if mode < 4 {
            assert_eq!(
                session.memory_objects.lock().unwrap()[&basecraft_io::hash_bytes(MAIN)],
                MAIN
            );
        } else {
            assert!(!objects_dir(&dir).exists());
        }
    }
}

#[test]
fn post_verification_errors_keep_their_native_stage() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::post_verification_errors_keep_their_native_stage",
    ) else {
        return;
    };
    for mode in 0..if cfg!(unix) { 3 } else { 2 } {
        let dir = root.join(format!("stage-{mode}"));
        let (mut session, id, main) = fixture(&dir);
        let before = session.doc(id).unwrap().to_genbank();
        let dest = object(&session, MAIN);
        let parent = dest.parent().unwrap().to_path_buf();
        if mode == 1 {
            session.snapshot_bytes(MAIN).unwrap();
        }
        if mode == 2 {
            std::fs::create_dir_all(&parent).unwrap();
        }
        let target = main.clone();
        let parent_hook = parent.clone();
        let guard = hook(move |stage, path| {
            if stage != Stage::AfterRead || path != target {
                return;
            }
            match mode {
                0 => std::fs::remove_file(path).unwrap(),
                1 => {
                    std::fs::remove_file(&dest).unwrap();
                    std::fs::write(path, b"changed after streaming verification").unwrap();
                }
                _ => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            &parent_hook,
                            std::fs::Permissions::from_mode(0o500),
                        )
                        .unwrap();
                    }
                }
            }
        });
        let error = session.apply(id, edit()).unwrap_err();
        drop(guard);
        match mode {
            0 => assert!(matches!(error, Error::Io(_))),
            1 => assert_eq!(
                error.to_string(),
                "invalid operation: source changed while retaining its snapshot"
            ),
            _ => {
                assert!(
                    matches!(error, Error::Format(basecraft_io::Error::Io(_))),
                    "{error:?}"
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                        .unwrap();
                }
            }
        }
        unchanged(&session, id, &before);
        assert!(!dir.join("recovery.json").exists());
        assert!(!object(&session, MAIN).exists());
    }
}

#[test]
fn proposals_export_save_and_empty_creation_use_the_shared_promotion() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::proposals_export_save_and_empty_creation_use_the_shared_promotion",
    ) else {
        return;
    };
    for route in 0..3 {
        for changed in [false, true] {
            let dir = root.join(format!("route-{route}-{changed}"));
            let (mut session, id, main) = fixture(&dir);
            let before = session.doc(id).unwrap().to_genbank();
            let destination = dir.join("output.fasta");
            if changed {
                std::fs::write(&main, b"changed").unwrap();
            }
            let result = match route {
                0 => session
                    .apply_sequence_proposals(
                        id,
                        session.generation(id),
                        session.revision(id),
                        &[edit()],
                        &session.actor.clone(),
                    )
                    .map(|_| ()),
                1 => session.export(id, &destination, "text", |w| w.write_all(b"derived")),
                _ => session.save(id, Some(&destination)),
            };
            if changed {
                assert!(result.is_err());
                unchanged(&session, id, &before);
                assert!(!destination.exists());
            } else {
                result.unwrap();
                assert!(session.docs[id.0].baseline.is_none());
                assert_eq!(std::fs::read(object(&session, MAIN)).unwrap(), MAIN);
                assert_eq!(std::fs::read(object(&session, GFF)).unwrap(), GFF);
            }
        }
    }
    let mut session = Session::recoverable(root.join("creation")).unwrap();
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    let id = session.create_sequence("new", "ACGT").unwrap();
    assert!(session.docs[id.0].baseline.is_none());
    #[cfg(feature = "test-counters")]
    assert!(basecraft_io::counters::snapshot().reads.is_empty());
}
