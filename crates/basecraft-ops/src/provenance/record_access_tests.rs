use super::*;
use crate::access::{Access, AccessError, Denial, ResourceAction, TrustedLegacy};
use std::{cell::Cell, io, path::PathBuf};

struct Choice {
    result: std::result::Result<PathBuf, Denial>,
    calls: Cell<usize>,
}
impl Choice {
    fn allow(path: &Path) -> Self {
        Self {
            result: Ok(path.into()),
            calls: Cell::new(0),
        }
    }
}
impl Access for Choice {
    fn boundary(&self) -> Option<&Path> {
        None
    }
    fn resource(&self, _path: &Path, action: ResourceAction) -> crate::access::Result<PathBuf> {
        assert_eq!(action, ResourceAction::ReadFile);
        self.calls.set(self.calls.get() + 1);
        self.result.clone().map_err(AccessError::Denied)
    }
}

#[test]
fn denial_precedes_real_lower_reader_and_preserves_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readable");
    std::fs::write(&path, b"must not be read").unwrap();
    for reason in [Denial::OutsideRoot, Denial::ReadOnly] {
        let access = Choice {
            result: Err(reason),
            calls: Cell::new(0),
        };
        let lower_calls = Cell::new(0);
        let mut remaining = 99;
        let result = read_record_with_access_open(&path, &mut remaining, &access, |path| {
            lower_calls.set(lower_calls.get() + 1);
            let file = basecraft_platform::files::open_regular_read(path)?;
            let size = file.metadata()?.len();
            Ok((file, size))
        });
        assert!(
            matches!(result, Err(crate::Error::Access(AccessError::Denied(got))) if got == reason)
        );
        assert_eq!(lower_calls.get(), 0);
        assert_eq!(remaining, 99);
        assert_eq!(access.calls.get(), 1);
    }
}

#[test]
fn returned_path_is_used_without_original_path_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let selected = dir.path().join("selected");
    std::fs::write(&original, b"wrong").unwrap();
    std::fs::write(&selected, b"selected bytes").unwrap();
    let choice = Choice::allow(&selected);
    let mut budget = 100;
    assert_eq!(
        read_record_with_access(&original, &mut budget, &choice).unwrap(),
        b"selected bytes"
    );
    assert_eq!(budget, 86);
    std::fs::remove_file(&selected).unwrap();
    assert!(
        matches!(read_record_with_access(&original, &mut budget, &choice),
        Err(crate::Error::Io(error)) if error.kind() == io::ErrorKind::NotFound)
    );
    assert_eq!(budget, 86);
    assert_eq!(choice.calls.get(), 2);
}

fn parity(path: &Path, initial: u64) -> (Result<Vec<u8>>, u64) {
    let mut old_budget = initial;
    let mut new_budget = initial;
    let old = read_record_with_budget(path, &mut old_budget);
    let new = read_record_with_access(path, &mut new_budget, &Choice::allow(path));
    match (&old, &new) {
        (Ok(a), Ok(b)) => assert_eq!(a, b),
        (Err(crate::Error::Io(a)), Err(crate::Error::Io(b))) => {
            assert_eq!(a.kind(), b.kind());
            assert_eq!(a.raw_os_error(), b.raw_os_error());
        }
        (Err(crate::Error::Invalid(a)), Err(crate::Error::Invalid(b))) => assert_eq!(a, b),
        _ => panic!("reader parity differs: {old:?} / {new:?}"),
    }
    assert_eq!(old_budget, new_budget);
    (old, old_budget)
}

#[test]
fn wrappers_preserve_raw_bytes_errors_and_all_budget_limits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("record");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(read_record(&file).unwrap(), b"abc");
    let (bytes, left) = parity(&file, 3);
    assert_eq!(bytes.unwrap(), b"abc");
    assert_eq!(left, 0);
    for limit in [0, 2] {
        let (error, left) = parity(&file, limit);
        assert!(matches!(error, Err(crate::Error::Invalid(_))));
        assert_eq!(left, limit); // Metadata refusal consumes no bytes.
    }
    assert!(matches!(
        parity(&dir.path().join("absent"), 20).0,
        Err(crate::Error::Io(_))
    ));
    assert!(matches!(parity(dir.path(), 20).0, Err(crate::Error::Io(_))));
    let oversized = dir.path().join("oversized");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len((64 << 20) + 1)
        .unwrap();
    let (error, left) = parity(&oversized, 100 << 20);
    assert!(matches!(error, Err(crate::Error::Invalid(_))));
    assert_eq!(left, 100 << 20);
    std::fs::write(&file, b"").unwrap();
    assert_eq!(parity(&file, 0).0.unwrap(), [] as [u8; 0]);
}

#[test]
fn aggregate_budget_and_malformed_bytes_are_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-json");
    std::fs::write(&file, b"{").unwrap();
    for access in [&TrustedLegacy as &dyn Access, &Choice::allow(&file)] {
        let mut budget = 2;
        for expected in [1, 0] {
            let bytes = read_record_with_access(&file, &mut budget, access).unwrap();
            assert_eq!(bytes, b"{");
            assert_eq!(budget, expected);
            assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_err());
        }
        assert!(matches!(
            read_record_with_access(&file, &mut budget, access),
            Err(crate::Error::Invalid(_))
        ));
        assert_eq!(budget, 0);
    }
    let (bytes, left) = parity(&file, 4);
    assert_eq!(bytes.unwrap(), b"{");
    assert_eq!(left, 3);
}

#[cfg(unix)]
#[test]
fn trusted_wrappers_keep_regular_symlink_and_os_denial_behavior() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("record");
    let link = dir.path().join("link");
    std::fs::write(&file, b"bytes").unwrap();
    symlink(&file, &link).unwrap();
    assert_eq!(parity(&link, 10).0.unwrap(), b"bytes");
    assert_eq!(read_record(&link).unwrap(), b"bytes");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (result, remaining) = parity(&file, 10);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        matches!(result, Err(crate::Error::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied),
        "run this test as an ordinary user"
    );
    assert_eq!(remaining, 10);
}

struct PartialRead {
    sent: bool,
}
impl io::Read for PartialRead {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.sent {
            Err(io::Error::from_raw_os_error(5))
        } else {
            let n = 3.min(bytes.len());
            bytes[..n].copy_from_slice(&b"abc"[..n]);
            self.sent = true;
            Ok(n)
        }
    }
}

#[test]
fn partial_read_failure_accounts_consumed_bytes_in_shared_body() {
    let path = Path::new("synthetic-reader");
    let choice = Choice::allow(path);
    for access in [&TrustedLegacy as &dyn Access, &choice] {
        let mut budget = 10;
        let result = read_record_with_access_open(path, &mut budget, access, |_| {
            Ok((PartialRead { sent: false }, 3))
        });
        assert!(matches!(result, Err(crate::Error::Io(error)) if error.raw_os_error() == Some(5)));
        assert_eq!(budget, 7);
        // A growing file can exceed the metadata length. Count limit+1 bytes,
        // then fail with the same existing budget error.
        let mut budget = 2;
        let result = read_record_with_access_open(path, &mut budget, access, |_| {
            Ok((io::Cursor::new(b"abcd"), 1))
        });
        assert!(matches!(result, Err(crate::Error::Invalid(_))));
        assert_eq!(budget, 0);
    }
}

#[test]
fn access_carrier_keeps_typed_denial_io_source_and_raw_os_code() {
    #[derive(Debug, thiserror::Error)]
    #[error("permission_denied is only text in this IO source")]
    struct Cause;
    let denied: crate::Error = AccessError::Denied(Denial::ReadOnly).into();
    assert_eq!(denied.code(), "permission_denied");
    assert!(matches!(
        denied,
        crate::Error::Access(AccessError::Denied(Denial::ReadOnly))
    ));
    let conflict: crate::Error = AccessError::DestinationExists.into();
    assert_eq!(conflict.code(), "output_conflict");
    let io: crate::Error = AccessError::Io(io::Error::from_raw_os_error(13)).into();
    assert_eq!(io.code(), "io");
    assert!(
        matches!(io, crate::Error::Access(AccessError::Io(error)) if error.raw_os_error() == Some(13))
    );
    let error: crate::Error = AccessError::Io(io::Error::other(Cause)).into();
    assert_eq!(error.code(), "io");
    let crate::Error::Access(AccessError::Io(error)) = error else {
        panic!("lost IO carrier")
    };
    assert!(error.get_ref().unwrap().is::<Cause>());
}

#[test]
fn extracted_body_preserves_caps_growth_charge_and_labels() {
    use super::{RecordLimit, read_record_from_open};
    for (label, message) in [
        (RecordLimit::Registry, "known-store registry exceeds 1 MiB"),
        (
            RecordLimit::Provenance,
            "provenance record exceeds reading limit (at most 64 MiB)",
        ),
    ] {
        let mut remaining = 100;
        let result =
            read_record_from_open(std::io::Cursor::new(b"abcdef"), 0, &mut remaining, 4, label);
        assert_eq!(remaining, 95); // limit+1 actually read, not just the cap
        assert!(matches!(result, Err(crate::Error::Invalid(text)) if text == message));
    }
    let mut remaining = 100;
    let result = read_record_from_open(
        std::io::empty(),
        65 << 20,
        &mut remaining,
        u64::MAX,
        RecordLimit::Provenance,
    );
    assert!(result.is_err());
    assert_eq!(remaining, 100);
}
