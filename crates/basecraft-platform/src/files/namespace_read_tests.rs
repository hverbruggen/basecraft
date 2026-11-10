use super::*;
use std::{
    cell::RefCell,
    ffi::{OsStr, OsString},
    os::unix::fs::{PermissionsExt, symlink},
};
thread_local! { static TRACE: RefCell<Vec<(bool, OsString)>> = const { RefCell::new(Vec::new()) }; }
pub(super) fn record(content: bool, name: &OsStr) {
    TRACE.with_borrow_mut(|v| v.push((content, name.to_owned())));
}
fn trace() -> Vec<(bool, OsString)> {
    TRACE.with_borrow_mut(std::mem::take)
}
fn contents(input: CheckedRead) -> String {
    let CheckedRead::File(mut file) = input else {
        panic!("expected file")
    };
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}
#[test]
fn retained_read_and_metadata_only_drop() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("nested");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("data");
    std::fs::write(&path, "original").unwrap();
    trace();
    drop(inspect_read_namespace(&path).unwrap());
    assert!(trace().iter().all(|(content, _)| !content));
    let checked = inspect_read_namespace(&path).unwrap().open_read().unwrap();
    assert_eq!(trace().iter().filter(|(content, _)| *content).count(), 1);
    std::fs::rename(&parent, dir.path().join("old")).unwrap();
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, "replacement").unwrap();
    assert_eq!(contents(checked), "original");
}
#[test]
fn absence_preserves_error_and_never_probes_suffix() {
    let dir = tempfile::tempdir().unwrap();
    for suffix in ["missing", "missing/unprobed/leaf"] {
        let path = dir.path().join(suffix);
        trace();
        let ticket = inspect_read_namespace(&path).unwrap();
        let original = std::fs::symlink_metadata(dir.path().join("missing"))
            .unwrap_err()
            .to_string();
        let CheckedRead::Absent(absence) = ticket.open_read().unwrap() else {
            panic!()
        };
        assert_eq!(absence.location(), path);
        assert_eq!(absence.error().kind(), io::ErrorKind::NotFound);
        assert!(absence.error().raw_os_error().is_some());
        assert_eq!(absence.error().to_string(), original);
        let (location, error) = absence.into_parts();
        assert_eq!(location, path);
        assert_eq!(error.to_string(), original);
        assert!(
            trace()
                .iter()
                .all(|(content, name)| !content && name != "unprobed" && name != "leaf")
        );
        assert!(!dir.path().join("missing").exists());
    }
}
#[test]
fn substitutions_refuse_before_content_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::write(&path, "new").unwrap();
    trace();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Changed)
    ));
    assert!(trace().iter().all(|(c, _)| !c));
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::rename(&path, dir.path().join("old")).unwrap();
    std::fs::write(&path, "other").unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Changed)
    ));
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::hard_link(&path, dir.path().join("alias")).unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Aliased)
    ));
    assert!(matches!(
        inspect_read_namespace(&path),
        Err(NamespaceReadError::Aliased)
    ));
    let parent = dir.path().join("parent");
    std::fs::create_dir(&parent).unwrap();
    let ticket = inspect_read_namespace(&parent.join("missing/leaf")).unwrap();
    std::fs::rename(&parent, dir.path().join("retained")).unwrap();
    std::fs::create_dir(&parent).unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Changed)
    ));
}
#[test]
fn redirects_and_special_leaves_never_open_content() {
    use rustix::fs::{CWD, Mode, mkfifoat};
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    for (name, destination) in [
        ("above", target.clone()),
        ("dangling", dir.path().join("absent")),
    ] {
        let link = dir.path().join(name);
        symlink(destination, &link).unwrap();
        for path in [link.clone(), link.join("config/preferences.toml")] {
            trace();
            assert!(matches!(
                inspect_read_namespace(&path),
                Err(NamespaceReadError::Redirected)
            ));
            assert!(trace().iter().all(|(c, _)| !c));
        }
    }
    let fifo = dir.path().join("fifo");
    mkfifoat(CWD, &fifo, Mode::RUSR | Mode::WUSR).unwrap();
    let socket = dir.path().join("socket");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    for path in [&target, &fifo, &socket] {
        trace();
        assert!(matches!(
            inspect_read_namespace(path),
            Err(NamespaceReadError::WrongKind)
        ));
        assert!(trace().iter().all(|(c, _)| !c));
    }
    let missing = dir.path().join("later");
    let ticket = inspect_read_namespace(&missing).unwrap();
    symlink(&target, &missing).unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Redirected)
    ));
}
#[test]
fn lexical_refusal_and_raw_io() {
    for path in [
        "relative/file",
        "/../file",
        "/a/./file",
        "//file",
        "/a//b",
        "/a/",
        "/",
    ] {
        trace();
        assert!(matches!(
            inspect_read_namespace(Path::new(path)),
            Err(NamespaceReadError::Unsupported)
        ));
        assert_eq!(trace(), []);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("denied");
    std::fs::write(&path, "data").unwrap();
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
    let result = ticket.open_read();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let Err(NamespaceReadError::Io(error)) = result else {
        panic!("requires ordinary user")
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.raw_os_error().is_some());
    let custom = io::Error::other("symbolic link: misleading text");
    let error = NamespaceReadError::from(custom);
    assert!(matches!(error, NamespaceReadError::Io(_)));
}

#[test]
fn intermediate_appearance_and_ancestor_redirect_are_not_absence() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("parent");
    let path = parent.join("still-missing/leaf");
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::create_dir(&parent).unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Changed)
    ));
    let ticket = inspect_read_namespace(&path).unwrap();
    std::fs::rename(&parent, dir.path().join("retained")).unwrap();
    symlink(dir.path().join("retained"), &parent).unwrap();
    assert!(matches!(
        ticket.open_read(),
        Err(NamespaceReadError::Redirected)
    ));
}

#[test]
fn inaccessible_parent_preserves_original_io() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("denied");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o0)).unwrap();
    let result = inspect_read_namespace(&parent.join("leaf"));
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let Err(NamespaceReadError::Io(error)) = result else {
        panic!("requires ordinary user")
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.raw_os_error().is_some());
}
