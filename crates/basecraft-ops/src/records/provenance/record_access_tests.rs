use super::*;

#[test]
fn reads_keep_bytes_and_enforce_shared_budget() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("record.json");
    std::fs::write(&path, b"not JSON").unwrap();
    let mut remaining = 16;
    for _ in 0..2 {
        assert_eq!(
            read_record_with_budget(&path, &mut remaining).unwrap(),
            b"not JSON"
        );
    }
    assert_eq!(remaining, 0);
    assert!(matches!(
        read_record_with_budget(&path, &mut remaining),
        Err(crate::Error::Limit { limit: 0, .. })
    ));
    assert!(
        matches!(read_record(&root.path().join("missing")), Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
}

#[test]
fn growth_past_cap_charges_only_read_bytes() {
    let label = RecordLimit::Provenance;
    let mut remaining = 100;
    let error = read_record_from_open(std::io::Cursor::new(b"abcdef"), 0, &mut remaining, 4, label)
        .unwrap_err();
    assert_eq!(remaining, 95);
    assert!(matches!(error, crate::Error::Limit { limit: 4, .. }));
    let error = read_record_from_open(std::io::empty(), 65 << 20, &mut remaining, u64::MAX, label)
        .unwrap_err();
    assert!(matches!(error, crate::Error::Limit { .. }));
    assert_eq!(remaining, 95);
}
