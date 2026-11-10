use super::*;
use std::{cell::RefCell, os::unix::fs::symlink};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Seam {
    BeforePrecheck,
    BeforeRename,
    AfterRename,
}
type Hook = (Seam, Box<dyn FnOnce()>);
thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static EVENTS: RefCell<Vec<Seam>> = const { RefCell::new(Vec::new()) };
}
pub(super) fn seam(point: Seam) {
    EVENTS.with_borrow_mut(|events| events.push(point));
    let hook = HOOK.with_borrow_mut(|hook| {
        if hook.as_ref().is_some_and(|(at, _)| *at == point) {
            hook.take()
        } else {
            None
        }
    });
    if let Some((_, action)) = hook {
        action();
    }
}
fn hook(point: Seam, action: impl FnOnce() + 'static) {
    HOOK.with_borrow_mut(|hook| *hook = Some((point, Box::new(action))));
}
fn events() -> Vec<Seam> {
    EVENTS.with_borrow_mut(std::mem::take)
}
fn observe(path: &Path) -> TargetNamespace {
    inspect_target_namespace(path).unwrap()
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    events();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    (root, source, destination)
}
#[test]
fn moves_once_retains_full_identity_and_only_probes_endpoints_and_ancestors() {
    let (_root, source, destination) = fixture();
    std::fs::write(source.join("unprobed-content"), b"data").unwrap();
    let before = observe(&source);
    let identity = before.metadata().unwrap().identity().clone();
    let absent = observe(&destination);
    super::tests::lock_trace();
    let moved = before.promote_directory_to(absent).unwrap();
    moved.destination().recheck().unwrap();
    assert_eq!(
        moved.destination().metadata().unwrap().identity(),
        &identity
    );
    let trace = super::tests::lock_trace();
    assert!(trace.iter().all(|(content, name)| !content
        && (source.components().any(|c| c.as_os_str() == name) || name == "destination")));
    assert_eq!(
        events(),
        [Seam::BeforePrecheck, Seam::BeforeRename, Seam::AfterRename]
    );
    assert!(!source.exists());
    assert_eq!(
        std::fs::read(destination.join("unprobed-content")).unwrap(),
        b"data"
    );
    let next = moved.into_destination();
    next.recheck().unwrap();
}
#[test]
fn existing_and_raced_destinations_are_preserved() {
    for at in [None, Some(Seam::BeforePrecheck), Some(Seam::BeforeRename)] {
        let (_root, source, destination) = fixture();
        let src = observe(&source);
        if at.is_none() {
            std::fs::create_dir(&destination).unwrap();
        }
        let dst = observe(&destination);
        if let Some(at) = at {
            let path = destination.clone();
            hook(at, move || {
                std::fs::create_dir(path).unwrap();
            });
        }
        let result = src.promote_directory_to(dst);
        assert!(matches!(result, Err(DirectoryPromotionError::NotMoved(_))));
        assert!(source.is_dir());
        assert!(destination.is_dir());
        assert!(!events().contains(&Seam::AfterRename));
    }
}
#[test]
fn source_parent_and_above_root_substitutions_refuse_before_rename() {
    for endpoint in ["source", "parent", "ancestor"] {
        for redirect in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let ancestor = root.path().join("ancestor");
            let parent = ancestor.join("parent");
            std::fs::create_dir_all(&parent).unwrap();
            let source = parent.join("source");
            std::fs::create_dir(&source).unwrap();
            let destination = parent.join("destination");
            let src = observe(&source);
            let dst = observe(&destination);
            let replaced = match endpoint {
                "source" => source.clone(),
                "parent" => parent,
                _ => ancestor,
            };
            let retained = root.path().join("retained");
            hook(Seam::BeforePrecheck, move || {
                std::fs::rename(&replaced, &retained).unwrap();
                if redirect {
                    symlink(&retained, &replaced).unwrap();
                } else {
                    std::fs::create_dir(&replaced).unwrap();
                }
            });
            events();
            assert!(matches!(
                src.promote_directory_to(dst),
                Err(DirectoryPromotionError::NotMoved(_))
            ));
            assert_eq!(events(), [Seam::BeforePrecheck]);
            assert!(!destination.exists());
        }
    }
}
#[test]
fn missing_parents_and_wrong_source_kinds_refuse() {
    let (root, source, destination) = fixture();
    assert!(matches!(
        inspect_target_namespace(&root.path().join("missing/leaf")),
        Err(NamespaceReadError::Io(_))
    ));
    for kind in ["file", "absent"] {
        let path = root.path().join(kind);
        if kind == "file" {
            std::fs::write(&path, b"data").unwrap();
        }
        assert!(matches!(
            observe(&path).promote_directory_to(observe(&destination)),
            Err(DirectoryPromotionError::NotMoved(
                NamespaceReadError::WrongKind
            ))
        ));
        assert!(!destination.exists());
    }
    let link = root.path().join("link");
    symlink(&source, &link).unwrap();
    assert!(matches!(
        inspect_target_namespace(&link),
        Err(NamespaceReadError::Redirected)
    ));
}
#[test]
fn raw_rename_failure_preserves_os_error() {
    let (root, source, destination) = fixture();
    let src = observe(&source);
    let dst = observe(&destination);
    let retained = root.path().join("retained");
    let old = source.clone();
    hook(Seam::BeforeRename, move || {
        std::fs::rename(old, retained).unwrap();
    });
    let Err(DirectoryPromotionError::NotMoved(NamespaceReadError::Io(error))) =
        src.promote_directory_to(dst)
    else {
        panic!("expected actual rename failure")
    };
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.raw_os_error().is_some());
    assert!(!destination.exists());
    assert_eq!(events(), [Seam::BeforePrecheck, Seam::BeforeRename]);
}
#[test]
fn postcheck_failure_remains_moved_without_cleanup_or_retry() {
    for parent_change in [false, true] {
        let (root, source, destination) = fixture();
        let src = observe(&source);
        let dst = observe(&destination);
        // Retain an independently owned temporary container for substitutions.
        let holding = tempfile::tempdir().unwrap();
        let saved = holding.path().join("saved");
        let target = if parent_change {
            root.path().to_path_buf()
        } else {
            destination.clone()
        };
        let saved_hook = saved.clone();
        hook(Seam::AfterRename, move || {
            std::fs::rename(&target, saved_hook).unwrap();
            std::fs::create_dir(&target).unwrap();
        });
        let result = src.promote_directory_to(dst);
        assert!(matches!(
            result,
            Err(DirectoryPromotionError::MovedWithPostcheckError(
                NamespaceReadError::Changed
            ))
        ));
        assert_eq!(
            events(),
            [Seam::BeforePrecheck, Seam::BeforeRename, Seam::AfterRename]
        );
        assert!(!source.exists());
        if parent_change {
            assert!(saved.join("destination").is_dir());
        } else {
            assert!(saved.is_dir());
            assert!(destination.is_dir());
        }
    }
}

#[test]
fn source_replaced_at_syscall_seam_cannot_mint_valid_observation() {
    let (root, source, destination) = fixture();
    let src = observe(&source);
    let dst = observe(&destination);
    let retained = root.path().join("retained");
    let old = source.clone();
    hook(Seam::BeforeRename, move || {
        std::fs::rename(&old, retained).unwrap();
        std::fs::create_dir(&old).unwrap();
    });
    assert!(matches!(
        src.promote_directory_to(dst),
        Err(DirectoryPromotionError::MovedWithPostcheckError(
            NamespaceReadError::Changed
        ))
    ));
    assert!(!source.exists());
    assert!(destination.is_dir());
    assert!(root.path().join("retained").is_dir());
    assert_eq!(
        events(),
        [Seam::BeforePrecheck, Seam::BeforeRename, Seam::AfterRename]
    );
}

#[test]
fn distinct_retained_parents_and_destination_parent_substitution() {
    for replace in [false, true] {
        let (root, source, _) = fixture();
        let parent = root.path().join("other-parent");
        std::fs::create_dir(&parent).unwrap();
        let destination = parent.join("destination");
        let src = observe(&source);
        let identity = src.metadata().unwrap().identity().clone();
        let dst = observe(&destination);
        if replace {
            let saved = root.path().join("saved-parent");
            hook(Seam::BeforePrecheck, move || {
                std::fs::rename(&parent, saved).unwrap();
                std::fs::create_dir(parent).unwrap();
            });
        }
        let result = src.promote_directory_to(dst);
        if replace {
            assert!(matches!(result, Err(DirectoryPromotionError::NotMoved(_))));
            assert!(source.is_dir());
            assert!(!destination.exists());
            assert_eq!(events(), [Seam::BeforePrecheck]);
        } else {
            let moved = result.unwrap();
            moved.destination().recheck().unwrap();
            assert_eq!(
                moved.destination().metadata().unwrap().identity(),
                &identity
            );
            assert!(!source.exists());
            assert!(destination.is_dir());
        }
    }
}
