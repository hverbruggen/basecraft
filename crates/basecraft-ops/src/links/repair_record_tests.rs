//! Original producer byte oracle. Inputs are independent of decoded output records.
use super::*;

fn original_recovery(
    previous_origin: &PreviousMetadata,
    previous_view: &PreviousMetadata,
    origin: &Option<StagedLink>,
    view: &Option<StagedLink>,
) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "note": "Save As association recovery: previous entries and intended metadata; snapshots are retained separately",
        "previous_origin": previous_origin,
        "previous_view": previous_view,
        "intended_origin": origin.as_ref().map(|link| &link.metadata),
        "intended_view": view.as_ref().map(|link| &link.metadata),
    })).unwrap()
}

fn original_guard(recovery: &Path) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "unavailable": "Save As alignment publication incomplete",
        "recovery": recovery.file_name(),
    })).unwrap()
}

#[test]
fn repair_original_producer_bytes() {
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
            metadata: include_bytes!("../../../../testdata/fixtures/records-v1/link-snapshot-extra.json").to_vec(),
        });
        let view = (mode > 1).then(|| StagedLink {
            snapshot: None,
            metadata: include_bytes!("../../../../testdata/fixtures/records-v1/link-referenced-extra.json").to_vec(),
        });
        let prepared = PreparedLinks {
            target: target.clone(), origin, view,
            previous_origin: PreviousMetadata::read(&paths[0]).unwrap(),
            previous_view: PreviousMetadata::read(&paths[1]).unwrap(),
        };
        let expected = original_recovery(&prepared.previous_origin, &prepared.previous_view, &prepared.origin, &prepared.view);
        let error = prepared.publish(|| -> Result<()> {
            // Both guards must precede any attempt at the main data write.
            assert_eq!(std::fs::read(&paths[0]).unwrap(), std::fs::read(&paths[1]).unwrap());
            Err(invalid("test save failure"))
        }).unwrap_err();
        assert!(error.to_string().contains("Save As incomplete"));
        assert_eq!(std::fs::read(&target).unwrap(), b"old tree");
        let recovery = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with(".alignment-recovery.json")).unwrap();
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
