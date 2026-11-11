//! Original producer byte oracle. Inputs are independent of decoded output records.
use super::*;
use serde::Serialize;

// Frozen original declaration: the oracle must not share the new wire serializer.
#[derive(Serialize)]
#[serde(tag = "entry", content = "value", rename_all = "snake_case")]
enum OriginalPreviousMetadata {
    Absent,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn original_previous(value: &PreviousMetadata) -> OriginalPreviousMetadata {
    match value {
        PreviousMetadata::Absent => OriginalPreviousMetadata::Absent,
        PreviousMetadata::File(bytes) => OriginalPreviousMetadata::File(bytes.clone()),
        PreviousMetadata::Symlink(path) => OriginalPreviousMetadata::Symlink(path.clone()),
    }
}

fn original_recovery(
    previous_origin: &PreviousMetadata,
    previous_view: &PreviousMetadata,
    origin: Option<&StagedLink>,
    view: Option<&StagedLink>,
) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "note": "Save As association recovery: previous entries and intended metadata; snapshots are retained separately",
        "previous_origin": original_previous(previous_origin),
        "previous_view": original_previous(previous_view),
        "intended_origin": origin.map(|link| &link.metadata),
        "intended_view": view.map(|link| &link.metadata),
    })).unwrap()
}

fn original_guard(recovery: &Path) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "unavailable": "Save As alignment publication incomplete",
        "recovery": recovery.file_name(),
    }))
    .unwrap()
}

#[test]
fn repair_original_producer_bytes() {
    crate::isolated_profile::isolate!();
    for mode in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ü target.nwk");
        std::fs::write(&target, b"old tree").unwrap();
        let paths = [sidecar(&target), view_sidecar(&target)];
        match mode {
            0 => {}
            1 => {
                std::fs::write(&paths[0], [0, 255, b'\n']).unwrap();
                std::fs::write(&paths[1], []).unwrap();
            }
            2 => {
                #[cfg(unix)]
                for path in &paths {
                    std::os::unix::fs::symlink("missing ü.json", path).unwrap();
                }
                #[cfg(not(unix))]
                continue;
            }
            _ => unreachable!(),
        }
        // These are real DS03 writer captures used as opaque staged bytes.
        // No new snapshots or analysis runs are needed to exercise publish.
        let origin = (mode > 0).then(|| StagedLink {
            snapshot: None,
            metadata: include_bytes!(
                "../../../../../testdata/fixtures/records-v1/link-snapshot-extra.json"
            )
            .to_vec(),
        });
        let view = (mode > 1).then(|| StagedLink {
            snapshot: None,
            metadata: include_bytes!(
                "../../../../../testdata/fixtures/records-v1/link-referenced-extra.json"
            )
            .to_vec(),
        });
        let prepared = PreparedLinks {
            target: target.clone(),
            origin,
            view,
            previous_origin: previous_metadata(&paths[0]).unwrap(),
            previous_view: previous_metadata(&paths[1]).unwrap(),
        };
        let expected = original_recovery(
            &prepared.previous_origin,
            &prepared.previous_view,
            prepared.origin.as_ref(),
            prepared.view.as_ref(),
        );
        let error = prepared
            .publish(|| -> Result<()> {
                // Both guards must precede any attempt at the main data write.
                assert_eq!(
                    std::fs::read(&paths[0]).unwrap(),
                    std::fs::read(&paths[1]).unwrap()
                );
                Err(invalid("test save failure"))
            })
            .unwrap_err();
        assert!(error.to_string().contains("Save As incomplete"));
        assert_eq!(std::fs::read(&target).unwrap(), b"old tree");
        let recovery = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with(".alignment-recovery.json"))
            .unwrap();
        let actual = std::fs::read(&recovery).unwrap();
        assert_eq!(actual, expected);
        assert!(!actual.ends_with(b"\n"));
        let guard = std::fs::read(&paths[0]).unwrap();
        assert_eq!(guard, original_guard(&recovery));
        assert!(!guard.ends_with(b"\n"));
        if let Some(capture) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
            let capture = PathBuf::from(capture);
            std::fs::create_dir_all(&capture).unwrap();
            let label = ["absent", "file", "symlink"][mode];
            std::fs::write(capture.join(format!("link-repair-{label}.json")), actual).unwrap();
            std::fs::write(capture.join(format!("link-guard-{label}.json")), guard).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn invalid_utf8_prior_symlink_returns_error_before_publication() {
    crate::isolated_profile::isolate!();
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    for bad_view in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("tree.nwk");
        std::fs::write(&target, b"old tree").unwrap();
        let origin = sidecar(&target);
        let view = view_sidecar(&target);
        let (bad, ordinary) = if bad_view {
            (&view, &origin)
        } else {
            (&origin, &view)
        };
        let invalid_path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 255]));
        symlink(&invalid_path, bad).unwrap();
        std::fs::write(ordinary, b"old metadata").unwrap();
        let snapshot = dir.path().join("retained.alignment.afa");
        std::fs::write(&snapshot, b"retained snapshot").unwrap();
        let prepared = PreparedLinks {
            target: target.clone(),
            origin: Some(StagedLink {
                snapshot: Some(snapshot.clone()),
                metadata: b"intended".to_vec(),
            }),
            view: None,
            previous_origin: previous_metadata(&origin).unwrap(),
            previous_view: previous_metadata(&view).unwrap(),
        };
        let entries = || {
            let mut names = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>();
            names.sort();
            names
        };
        let before = entries();
        let error = prepared
            .publish(|| -> Result<()> { panic!("data callback must not run") })
            .unwrap_err();
        assert!(matches!(error, crate::Error::Json(_)), "{error}");
        assert_eq!(error.code(), "invalid_arguments");
        assert_eq!(entries(), before, "no DS04 file may be retained");
        assert_eq!(std::fs::read(&target).unwrap(), b"old tree");
        assert_eq!(std::fs::read(ordinary).unwrap(), b"old metadata");
        assert_eq!(std::fs::read_link(bad).unwrap(), invalid_path);
        assert_eq!(std::fs::read(&snapshot).unwrap(), b"retained snapshot");
    }
}

#[test]
fn view_guard_failure_precedes_data_and_origin_mutation() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("tree.nwk");
    std::fs::write(&target, b"old tree").unwrap();
    let origin = sidecar(&target);
    let view = view_sidecar(&target);
    std::fs::write(&origin, b"old origin").unwrap();
    let prepared = PreparedLinks {
        target: target.clone(),
        origin: None,
        view: None,
        previous_origin: previous_metadata(&origin).unwrap(),
        previous_view: previous_metadata(&view).unwrap(),
    };
    // Publication is obstructed after successful preparation.
    std::fs::create_dir(&view).unwrap();
    let error = prepared
        .publish(|| -> Result<()> { panic!("data callback must not run") })
        .unwrap_err();
    assert!(!error.to_string().contains("Save As incomplete"));
    assert_eq!(std::fs::read(&target).unwrap(), b"old tree");
    assert_eq!(std::fs::read(&origin).unwrap(), b"old origin");
    assert!(view.is_dir());
    assert!(std::fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .path()
            .to_string_lossy()
            .ends_with(".alignment-recovery.json")
    }));
}
