use super::*;
use std::{
    cell::RefCell,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

type Hook = Box<dyn FnOnce()>;
thread_local! {
    static EFFECTS: RefCell<Vec<LockStep>> = const { RefCell::new(Vec::new()) };
    static BEFORE: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static OPENED: RefCell<Option<Hook>> = const { RefCell::new(None) };
}
pub(super) fn effect(step: LockStep) {
    EFFECTS.with_borrow_mut(|events| events.push(step));
    if let Some(hook) = BEFORE.with_borrow_mut(Option::take) {
        hook();
    }
}
pub(super) fn opened() {
    if let Some(hook) = OPENED.with_borrow_mut(Option::take) {
        hook();
    }
}
fn effects() -> Vec<LockStep> {
    EFFECTS.with_borrow_mut(std::mem::take)
}
fn before(hook: impl FnOnce() + 'static) {
    BEFORE.with_borrow_mut(|slot| *slot = Some(Box::new(hook)));
}
fn permissions(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().mode() & 0o7777
}
fn lock(path: &Path) -> LockNamespace {
    inspect_lock_namespace(path, LockDirectoryMode::Preserve).unwrap()
}
fn read_trace() -> Vec<(bool, std::ffi::OsString)> {
    super::tests::lock_trace()
}
fn no_content() {
    assert!(read_trace().iter().all(|(content, _)| !content));
}

#[test]
fn target_metadata_is_content_free_and_retains_absence() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("target");
    std::fs::write(&file, "untouched").unwrap();
    // A real content open would fail for this ordinary user.
    permissions(&file, 0o0);
    read_trace();
    let target = inspect_target_namespace(&file).unwrap();
    assert_eq!(
        target.metadata().unwrap().kind(),
        crate::files::OrdinaryKind::File
    );
    assert_eq!(target.metadata().unwrap().links(), 1);
    assert_eq!(
        target.metadata().unwrap().identity(),
        &crate::files::ObjectIdentity::Unix {
            device: std::fs::symlink_metadata(&file).unwrap().dev(),
            inode: std::fs::symlink_metadata(&file).unwrap().ino(),
        }
    );
    target.recheck().unwrap();
    let directory = inspect_target_namespace(dir.path()).unwrap();
    assert_eq!(
        directory.metadata().unwrap().kind(),
        crate::files::OrdinaryKind::Directory
    );
    directory.recheck().unwrap();
    let missing = dir.path().join("absent");
    let target = inspect_target_namespace(&missing).unwrap();
    let error = target.absence_error().unwrap();
    let original = error as *const io::Error;
    assert_eq!(
        error.raw_os_error(),
        Some(rustix::io::Errno::NOENT.raw_os_error())
    );
    assert!(target.metadata().is_none());
    target.recheck().unwrap();
    assert_eq!(
        target.absence_error().unwrap() as *const io::Error,
        original
    );
    assert_eq!(
        target.parent_identity().unwrap(),
        directory.metadata().unwrap().identity()
    );
    no_content();
    permissions(&file, 0o600);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "untouched");
    assert!(!missing.exists());
}

#[test]
fn missing_target_parent_is_io_not_absent_target() {
    let dir = tempfile::tempdir().unwrap();
    read_trace();
    let error = inspect_target_namespace(&dir.path().join("absent/unprobed/target")).unwrap_err();
    let NamespaceReadError::Io(error) = error else {
        panic!("missing parent must retain IO")
    };
    assert_eq!(
        error.raw_os_error(),
        Some(rustix::io::Errno::NOENT.raw_os_error())
    );
    let trace = read_trace();
    assert!(
        trace
            .iter()
            .all(|(content, name)| !content && name != "unprobed" && name != "target")
    );
}

#[test]
fn steps_are_incremental_and_dropping_never_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one/two/evidence.lock");
    effects();
    let ticket = lock(&path);
    assert_eq!(effects(), []);
    assert_eq!(ticket.next_step().unwrap(), LockStep::CreateDirectory);
    let ticket = ticket.create_next_directory().unwrap();
    assert_eq!(effects(), [LockStep::CreateDirectory]);
    assert!(dir.path().join("one").is_dir());
    assert!(!dir.path().join("one/two").exists());
    drop(ticket); // a revoked policy can stop here, without another effect
    assert!(dir.path().join("one").is_dir());
    let ticket = lock(&path).create_next_directory().unwrap();
    assert_eq!(effects(), [LockStep::CreateDirectory]);
    assert_eq!(ticket.next_step().unwrap(), LockStep::OpenFile);
    let opened = ticket.open_file().unwrap();
    assert_eq!(effects(), [LockStep::OpenFile]);
    opened.recheck().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    assert_eq!(mode(&path) & !0o600, 0); // requested0600 never defeats inherited umask
    assert_eq!(mode(&dir.path().join("one")), 0o700);
    drop(opened);
    assert!(path.is_file());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn private_mode_repair_is_a_distinct_effect_and_preserve_never_repairs() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("private");
    std::fs::create_dir(&parent).unwrap();
    permissions(&parent, 0o750);
    let path = parent.join("lock");
    effects();
    let ticket = inspect_lock_namespace(&path, LockDirectoryMode::Private).unwrap();
    assert_eq!(mode(&parent), 0o750);
    assert_eq!(ticket.next_step().unwrap(), LockStep::SetPermissions);
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Unsupported)
    ));
    assert_eq!(effects(), []);
    let ticket = inspect_lock_namespace(&path, LockDirectoryMode::Private)
        .unwrap()
        .set_next_permissions()
        .unwrap();
    assert_eq!(effects(), [LockStep::SetPermissions]);
    assert_eq!(mode(&parent), 0o700);
    assert!(!path.exists());
    drop(ticket);
    permissions(&parent, 0o750);
    let opened = lock(&path).open_file().unwrap();
    assert_eq!(effects(), [LockStep::OpenFile]);
    assert_eq!(mode(&parent), 0o750);
    drop(opened);
    let ticket = lock(&path);
    assert!(matches!(
        ticket.set_next_permissions(),
        Err(NamespaceReadError::Unsupported)
    ));
    assert_eq!(effects(), []);
}

#[test]
fn stable_redirects_aliases_and_specials_never_open_or_mutate() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, dir.path().join("above")).unwrap();
    symlink(dir.path().join("missing"), dir.path().join("dangling")).unwrap();
    for path in [
        dir.path().join("above/state/lock"),
        dir.path().join("dangling"),
    ] {
        effects();
        read_trace();
        assert!(matches!(
            inspect_lock_namespace(&path, LockDirectoryMode::Private),
            Err(NamespaceReadError::Redirected)
        ));
        assert!(matches!(
            inspect_target_namespace(&path),
            Err(NamespaceReadError::Redirected)
        ));
        assert_eq!(effects(), []);
        no_content();
    }
    assert!(!outside.join("state").exists());
    let file = dir.path().join("file");
    std::fs::write(&file, "sentinel").unwrap();
    std::fs::hard_link(&file, dir.path().join("alias")).unwrap();
    assert!(matches!(
        inspect_target_namespace(&file),
        Err(NamespaceReadError::Aliased)
    ));
    assert!(matches!(
        inspect_lock_namespace(&file, LockDirectoryMode::Preserve),
        Err(NamespaceReadError::Aliased)
    ));
    let fifo = dir.path().join("fifo");
    rustix::fs::mkfifoat(rustix::fs::CWD, &fifo, rustix::fs::Mode::RWXU).unwrap();
    let socket = dir.path().join("socket");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    for path in [&fifo, &socket] {
        assert!(matches!(
            inspect_target_namespace(path),
            Err(NamespaceReadError::WrongKind)
        ));
        assert!(matches!(
            inspect_lock_namespace(path, LockDirectoryMode::Preserve),
            Err(NamespaceReadError::WrongKind)
        ));
    }
    assert!(matches!(
        inspect_lock_namespace(&outside, LockDirectoryMode::Preserve),
        Err(NamespaceReadError::WrongKind)
    ));
    assert_eq!(effects(), []);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "sentinel");
}

#[test]
fn changed_parent_mode_leaf_and_target_refuse_without_next_effect() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("parent");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("lock");
    let ticket = lock(&path);
    std::fs::rename(&parent, dir.path().join("retained")).unwrap();
    std::fs::create_dir(&parent).unwrap();
    effects();
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(effects(), []);
    let ticket = lock(&path);
    permissions(&parent, 0o750);
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(effects(), []);
    let ticket = lock(&path);
    std::fs::write(&path, "appeared").unwrap();
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(effects(), []);
    let target = inspect_target_namespace(&path).unwrap();
    let ticket = lock(&path);
    std::fs::hard_link(&path, parent.join("alias")).unwrap();
    assert!(matches!(target.recheck(), Err(NamespaceReadError::Aliased)));
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Aliased)
    ));
    assert_eq!(effects(), []);
}

#[test]
fn exclusive_mkdir_and_open_do_not_adopt_raced_entries() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("missing");
    let path = parent.join("lock");
    let ticket = lock(&path);
    let raced = parent.clone();
    before(move || std::fs::create_dir(raced).unwrap());
    effects();
    assert!(matches!(
        ticket.create_next_directory(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(effects(), [LockStep::CreateDirectory]);
    assert!(parent.is_dir());
    let ticket = lock(&path);
    let raced = path.clone();
    before(move || std::fs::write(raced, "other writer").unwrap());
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(effects(), [LockStep::OpenFile]);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "other writer");
}

#[test]
fn opened_handle_association_is_rechecked_and_never_unlinked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock");
    std::fs::write(&path, "old").unwrap();
    let ticket = lock(&path);
    let retained = dir.path().join("retained");
    let moved = retained.clone();
    let replacement = path.clone();
    OPENED.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move || {
            std::fs::rename(&replacement, moved).unwrap();
            std::fs::write(replacement, "new").unwrap();
        }))
    });
    assert!(matches!(
        ticket.open_file(),
        Err(NamespaceReadError::Changed)
    ));
    assert_eq!(std::fs::read_to_string(&retained).unwrap(), "old");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    let opened = lock(&path).open_file().unwrap();
    std::fs::rename(&path, dir.path().join("retained2")).unwrap();
    std::fs::write(&path, "third").unwrap();
    assert!(matches!(opened.recheck(), Err(NamespaceReadError::Changed)));
    drop(opened);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "third");
}

#[test]
fn ordinary_permission_errors_keep_raw_os_and_partial_artifacts() {
    assert_ne!(
        rustix::process::geteuid().as_raw(),
        0,
        "requires ordinary user"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one/two/lock");
    let ticket = lock(&path).create_next_directory().unwrap();
    let one = dir.path().join("one");
    // Change permissions at the actual syscall seam, after metadata recheck.
    let denied = one.clone();
    before(move || permissions(&denied, 0o500));
    let error = ticket.create_next_directory().unwrap_err();
    permissions(&one, 0o700);
    let NamespaceReadError::Io(error) = error else {
        panic!("expected actual mkdir IO")
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.raw_os_error().is_some());
    assert!(one.is_dir());
    assert!(!one.join("two").exists());
    let path = one.join("lock");
    std::fs::write(&path, "preserve").unwrap();
    let ticket = lock(&path);
    permissions(&path, 0o0);
    let result = ticket.open_file();
    permissions(&path, 0o600);
    let Err(NamespaceReadError::Io(error)) = result else {
        panic!("expected actual open IO")
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.raw_os_error().is_some());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "preserve");
}

#[test]
fn invalid_spelling_and_wrong_steps_have_no_effect() {
    for path in ["relative", "/a/../b", "/a/./b", "//a", "/a//b", "/a/", "/"] {
        effects();
        read_trace();
        assert!(matches!(
            inspect_target_namespace(Path::new(path)),
            Err(NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            inspect_lock_namespace(Path::new(path), LockDirectoryMode::Private),
            Err(NamespaceReadError::Unsupported)
        ));
        assert_eq!(effects(), []);
        assert_eq!(read_trace(), []);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/lock");
    assert!(matches!(
        lock(&path).open_file(),
        Err(NamespaceReadError::Unsupported)
    ));
    assert!(matches!(
        lock(&path).set_next_permissions(),
        Err(NamespaceReadError::Unsupported)
    ));
    assert_eq!(effects(), []);
    assert!(!dir.path().join("missing").exists());
}
