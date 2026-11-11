//! Original RP11 writer captures. Keep this Value oracle independent of future types.
use super::*;
use basecraft_protocol::provenance::Completeness;

fn publication(id: &str, parents: &[&str]) -> Publication {
    Publication {
        schema_version: 1,
        id: id.into(),
        session: "synthetic-session".into(),
        parents: parents.iter().map(|s| (*s).into()).collect(),
        parent_hashes: BTreeMap::new(),
        created: "synthetic-time".into(),
        origin: "Synthetic café \"quoted\"\nancestry".into(),
        inputs: vec![],
        outputs: vec![],
        events: vec![],
        effective_edits: vec![],
        completeness: Completeness {
            ancestry_known: true,
            input_bytes_available: true,
            execution_specification_available: false,
            replay_verified: false,
        },
        report_template_version: 1,
    }
}

fn source(root: &Path, name: &str, publications: &[Publication]) -> PathBuf {
    let bundle = root.join(name);
    std::fs::create_dir_all(bundle.join("records")).unwrap();
    for p in publications {
        // Deliberately noncanonical whitespace, retained byte for byte by the writer.
        let mut bytes = b" \n".to_vec();
        bytes.extend(serde_json::to_vec_pretty(p).unwrap());
        bytes.extend(b"\n ");
        std::fs::write(bundle.join("records").join(format!("{}.json", p.id)), bytes).unwrap();
    }
    bundle
}

fn target(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(path.join("artifacts")).unwrap();
    path
}

// The original recursively sorted Value recipe, frozen before type substitution.
fn old_map(rows: &[(&str, String, String)]) -> Vec<u8> {
    let rows: Vec<_> = rows.iter().map(|(id, hash, export)| {
        serde_json::json!({"original_id":id,"original_hash":hash,"export_id":export})
    }).collect();
    serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"records":rows})).unwrap()
}

fn check_map(target: &Path, archives: &[ArtifactRevision], expected: &[u8]) -> Vec<u8> {
    let map = archives
        .iter()
        .find(|a| a.logical_name == "export-map.json")
        .unwrap();
    let bytes = std::fs::read(target.join(map.retained.as_ref().unwrap())).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(map.hash, basecraft_io::hash_bytes(expected));
    assert_eq!(map.size, expected.len() as u64);
    assert_eq!(
        map.retained.as_deref(),
        Some(format!("artifacts/{}", map.hash.trim_start_matches("b3:")).as_str())
    );
    assert!(map.id.ends_with(":export-map"));
    assert!(map.reference.is_none());
    assert_ne!(bytes.last(), Some(&b'\n'));
    bytes
}

fn capture(name: &str, bytes: &[u8], originals: &[Vec<u8>], rows: &[(&str, String, String)]) {
    // Explicit opt-in for the one original-writer capture; ordinary tests never update goldens.
    if let Some(directory) = std::env::var_os("BASECRAFT_EXPORT_RECORD_CAPTURE") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("provenance-export-map-{name}.json")),
            bytes,
        )
        .unwrap();
        for (i, original) in originals.iter().enumerate() {
            std::fs::write(
                directory.join(format!("{name}-original-{i}.json")),
                original,
            )
            .unwrap();
        }
        std::fs::write(
            directory.join(format!("{name}-oracle-inputs.json")),
            serde_json::to_vec_pretty(rows).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn original_export_maps() {
    let temp = tempfile::tempdir().unwrap();
    let empty = target(temp.path(), "empty");
    let (roots, archives) = export_history(&[], &empty).unwrap();
    assert!(roots.is_empty());
    let bytes = check_map(&empty, &archives, &old_map(&[]));
    capture("empty", &bytes, &[], &[]);

    let single = publication("Single", &[]);
    let bundle = source(temp.path(), "single-source", std::slice::from_ref(&single));
    let original = std::fs::read(bundle.join("records/Single.json")).unwrap();
    let dest = target(temp.path(), "single");
    let (roots, archives) = export_history(&[(bundle.clone(), single)], &dest).unwrap();
    assert_ne!(roots[0].id, "Single");
    let rows = [(
        "Single",
        basecraft_io::hash_bytes(&original),
        roots[0].id.clone(),
    )];
    assert_originals(&dest, &archives, std::slice::from_ref(&original));
    assert_eq!(
        std::fs::read(bundle.join("records/Single.json")).unwrap(),
        original
    );
    let bytes = check_map(&dest, &archives, &old_map(&rows));
    capture("single", &bytes, &[original], &rows);

    // Root stack is LIFO: other-owner Shared, then owner Shared/Right/Left.
    // Left and Right share one parent. Equal ids in different owners stay distinct.
    let left = publication("Left", &["Shared"]);
    let right = publication("Right", &["Shared"]);
    let shared = publication("Shared", &[]);
    let a = source(
        temp.path(),
        "a",
        &[left.clone(), right.clone(), shared.clone()],
    );
    let mut other = shared;
    other.origin = "Distinct canonical owner".into();
    let b = source(temp.path(), "b", std::slice::from_ref(&other));
    let originals: Vec<_> = [(&b, "Shared"), (&a, "Shared"), (&a, "Right"), (&a, "Left")]
        .iter()
        .map(|(bundle, id)| {
            std::fs::read(bundle.join("records").join(format!("{id}.json"))).unwrap()
        })
        .collect();
    let dest = target(temp.path(), "shared");
    let (roots, archives) = export_history(
        &[(a, left), (temp.path().join("a"), right), (b, other)],
        &dest,
    )
    .unwrap();
    assert_eq!(roots[0].parents, roots[1].parents);
    assert_ne!(roots[0].parents[0], roots[2].id);
    let ids = [
        roots[2].id.clone(),
        roots[0].parents[0].clone(),
        roots[1].id.clone(),
        roots[0].id.clone(),
    ];
    let old_ids = ["Shared", "Shared", "Right", "Left"];
    let rows: Vec<_> = (0..4)
        .map(|i| {
            (
                old_ids[i],
                basecraft_io::hash_bytes(&originals[i]),
                ids[i].clone(),
            )
        })
        .collect();
    assert_originals(&dest, &archives, &originals);
    for root in &roots {
        let stored: Publication = serde_json::from_slice(
            &std::fs::read(dest.join("records").join(format!("{}.json", root.id))).unwrap(),
        )
        .unwrap();
        assert_eq!(stored.id, root.id);
        for parent in &root.parents {
            assert_eq!(
                root.parent_hashes[parent],
                basecraft_io::hash_file(&dest.join("records").join(format!("{parent}.json")))
                    .unwrap()
            );
        }
    }
    let bytes = check_map(&dest, &archives, &old_map(&rows));
    capture("shared-ancestry", &bytes, &originals, &rows);
}

fn assert_originals(dest: &Path, archives: &[ArtifactRevision], originals: &[Vec<u8>]) {
    assert_eq!(archives.len(), originals.len() + 1);
    for (archive, original) in archives.iter().zip(originals) {
        assert_eq!(archive.hash, basecraft_io::hash_bytes(original));
        assert_eq!(archive.size, original.len() as u64);
        assert_eq!(
            std::fs::read(dest.join(archive.retained.as_ref().unwrap())).unwrap(),
            *original
        );
        assert!(archive.id.ends_with(":original-record"));
        assert!(
            dest.join("records")
                .join(format!(
                    "{}.json",
                    archive.id.trim_end_matches(":original-record")
                ))
                .is_file()
        );
    }
}

#[test]
fn original_export_failures() {
    for case in [
        "unsafe",
        "empty",
        "missing",
        "malformed",
        "id",
        "schema",
        "cycle",
        "unknown",
        "corrupt",
        "count",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut p = publication("Root", &[]);
        let bundle = source(temp.path(), "source", std::slice::from_ref(&p));
        let record = bundle.join("records/Root.json");
        match case {
            "unsafe" => p.id = "../Root".into(),
            "empty" => p.id.clear(),
            "missing" => std::fs::remove_file(&record).unwrap(),
            "malformed" => std::fs::write(&record, b"{").unwrap(),
            "id" => {
                let mut q = p.clone();
                q.id = "Different".into();
                std::fs::write(&record, serde_json::to_vec(&q).unwrap()).unwrap();
            }
            "schema" => {
                let mut q = p.clone();
                q.schema_version = 2;
                std::fs::write(&record, serde_json::to_vec(&q).unwrap()).unwrap();
            }
            "cycle" => {
                p.parents.push(p.id.clone());
                std::fs::write(&record, serde_json::to_vec(&p).unwrap()).unwrap();
            }
            "unknown" => {
                let mut value = serde_json::to_value(&p).unwrap();
                value["extension"] = serde_json::json!({"nested":["café",null]});
                std::fs::write(&record, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "corrupt" => {
                std::fs::write(bundle.join("evidence"), b"bad").unwrap();
                p.inputs.push(ArtifactRevision {
                    id: "input".into(),
                    logical_name: "input".into(),
                    hash: basecraft_io::hash_bytes(b"good"),
                    size: 4,
                    retained: Some("evidence".into()),
                    reference: None,
                });
                std::fs::write(&record, serde_json::to_vec(&p).unwrap()).unwrap();
            }
            "count" => {
                p.parents = vec!["Parent".into(); 10_001];
                std::fs::write(&record, serde_json::to_vec(&p).unwrap()).unwrap();
                std::fs::write(
                    bundle.join("records/Parent.json"),
                    serde_json::to_vec(&publication("Parent", &[])).unwrap(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = std::fs::read(&record).ok();
        let dest = target(temp.path(), "target");
        let error = export_history(&[(bundle, p)], &dest).unwrap_err();
        let (code, message) = match case {
            "unsafe" | "empty" => ("invalid_operation", "unsafe publication identity"),
            "id" | "schema" => ("invalid_operation", "inconsistent export history record"),
            "cycle" => ("invalid_operation", "export history contains a cycle"),
            "corrupt" => ("invalid_operation", "evidence changed during export"),
            "count" => (
                "invalid_operation",
                "export history exceeds bounded record limit",
            ),
            "missing" => ("io", "No such file or directory (os error 2)"),
            "malformed" => ("json", "EOF while parsing an object at line 1 column 1"),
            "unknown" => ("json", "unknown field `extension`"),
            _ => unreachable!(),
        };
        assert_eq!(error.code(), code, "{case}: {error}");
        if code == "invalid_operation" {
            assert_eq!(error.to_string(), format!("invalid operation: {message}"));
        } else if case == "unknown" {
            assert!(error.to_string().starts_with(message));
        } else if case == "missing" {
            assert!(matches!(
                &error,
                crate::Error::Io(source) if source.kind() == std::io::ErrorKind::NotFound
            ));
            #[cfg(target_os = "linux")]
            assert_eq!(error.to_string(), message);
        } else {
            assert_eq!(error.to_string(), message);
        }
        assert_eq!(std::fs::read(&record).ok(), before);
        let artifacts: Vec<_> = std::fs::read_dir(dest.join("artifacts")).unwrap().collect();
        if case == "corrupt" {
            let original = before.unwrap();
            assert_eq!(artifacts.len(), 2); // original archived; bad evidence copied; no relocated record/map.
            assert_eq!(
                std::fs::read(dest.join(format!(
                    "artifacts/{}",
                    basecraft_io::hash_bytes(&original).trim_start_matches("b3:")
                )))
                .unwrap(),
                original
            );
            assert_eq!(
                std::fs::read(dest.join(format!(
                    "artifacts/{}",
                    basecraft_io::hash_bytes(b"good").trim_start_matches("b3:")
                )))
                .unwrap(),
                b"bad"
            );
            assert_eq!(std::fs::read_dir(dest.join("records")).unwrap().count(), 0);
        } else {
            assert!(artifacts.is_empty());
            assert!(!dest.join("records").exists());
        }
        println!("{case}: {}: {error}", error.code());
    }
}

#[test]
fn reached_final_map_obstruction() {
    let temp = tempfile::tempdir().unwrap();
    let dest = target(temp.path(), "target");
    let bytes = old_map(&[]);
    let path = dest.join(format!(
        "artifacts/{}",
        basecraft_io::hash_bytes(&bytes).trim_start_matches("b3:")
    ));
    // A file obstructs a parent directory of the exact final map path. An existing
    // regular file at the destination is normally replaced by the atomic writer.
    std::fs::remove_dir(dest.join("artifacts")).unwrap();
    std::fs::write(dest.join("artifacts"), b"preserve obstruction").unwrap();
    assert!(!path.exists());
    let error = export_history(&[], &dest).unwrap_err();
    assert_eq!(error.code(), "format");
    assert!(dest.join("records").is_dir()); // traversal complete; final write reached.
    assert_eq!(
        std::fs::read(dest.join("artifacts")).unwrap(),
        b"preserve obstruction"
    );
    println!("final map obstruction: {}: {error}", error.code());
}

#[test]
fn typed_export_maps_round_trip_original_fixtures() {
    use basecraft_protocol::export_records::ProvenanceExportMap;

    for bytes in [
        include_bytes!("../../../../testdata/fixtures/records-v1/provenance-export-map-empty.json")
            .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/provenance-export-map-single.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/provenance-export-map-shared-ancestry.json"
        )
        .as_slice(),
    ] {
        let map: ProvenanceExportMap = serde_json::from_slice(bytes).unwrap();
        let encoded = serde_json::to_vec_pretty(&serde_json::to_value(map).unwrap()).unwrap();
        assert_eq!(encoded, bytes);
        assert_ne!(encoded.last(), Some(&b'\n'));
    }
}
