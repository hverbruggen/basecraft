//! Edits to a file of several records name the record they change, and a
//! file whose records share a name can't be edited.

use std::path::Path;

use basecraft_ops::{Error, Session, records};
use basecraft_protocol::{Feature, Location, Op, Strand};

fn record(name: &str) -> String {
    format!(
        "LOCUS       {name:<24} 20 bp    DNA     linear SYN 10-OCT-2026
FEATURES             Location/Qualifiers
     gene            2..9
                     /gene=\"g\"
ORIGIN
        1 acgtacgtac gtacgtacgt
//
"
    )
}

fn feature(start: u64) -> Feature {
    Feature {
        kind: "misc_feature".into(),
        location: Location::simple(start, start + 4, Strand::Forward),
        qualifiers: Vec::new(),
    }
}

fn replace(record: Option<&str>) -> Op {
    Op::SeqReplace {
        start: 0,
        end: 1,
        residues: "G".into(),
        record: record.map(str::to_owned),
    }
}

/// One of each operation that names a record; returns the logged names.
fn edit_every_way(root: &Path, file: &Path) -> Vec<Option<String>> {
    let mut s = Session::new(root);
    let d = s.open(file).unwrap();
    s.apply(d, replace(None)).unwrap();
    let added = s
        .apply(
            d,
            Op::FeatureAdd {
                feature: feature(3),
                record: Some("first".into()),
            },
        )
        .unwrap();
    let id = added.feature.unwrap().0;
    s.apply(d, Op::feature_update(id, feature(5))).unwrap();
    s.apply(d, Op::feature_remove(0)).unwrap();
    s.extract(d, (2, 9), &[], false, &root.join("part.gb"), Some("first"))
        .unwrap();
    basecraft_ops::log::read_all(root)
        .unwrap()
        .into_iter()
        .filter_map(|mut entry| entry.op.record().map(|r| r.clone()))
        .collect()
}

#[test]
fn edits_name_their_record_and_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("two.gb");
    std::fs::write(&file, record("first") + &record("second")).unwrap();
    let named = edit_every_way(root, &file);
    assert_eq!(named, vec![Some("first".to_owned()); 5]);
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.unchecked, 0);

    // The logged edits, applied to a copy whose first record is another
    // one, are refused instead of landing on the wrong record.
    let other = tempfile::tempdir().unwrap();
    let swapped = other.path().join("two.gb");
    std::fs::write(&swapped, record("second") + &record("first")).unwrap();
    let mut s = Session::new(other.path());
    let d = s.open(&swapped).unwrap();
    for entry in basecraft_ops::log::read_all(root).unwrap() {
        if let Op::SeqReplace { .. } = entry.op {
            let refused = s.apply(d, entry.op).unwrap_err();
            assert!(refused.to_string().contains("only second can be edited"));
        }
    }
}

#[test]
fn a_single_record_file_logs_no_record_name() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("one.gb");
    std::fs::write(&file, record("first")).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&file).unwrap();
    s.apply(d, replace(None)).unwrap();
    let logged = basecraft_ops::log::read_all(root).unwrap();
    let mut last = logged.last().unwrap().op.clone();
    assert_eq!(last.record(), Some(&mut None));
}

#[test]
fn records_that_share_a_name_refuse_every_edit_and_name_the_clash() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("same.gb");
    std::fs::write(&file, record("first") + &record("other") + &record("first")).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&file).unwrap();
    let refusals = [
        s.apply(d, replace(None)).err(),
        s.apply(d, Op::feature_add(feature(3))).err(),
        s.apply(d, Op::feature_update(0, feature(5))).err(),
        s.apply(d, Op::feature_remove(0)).err(),
        s.extract(d, (2, 9), &[], false, &root.join("part.gb"), None)
            .err(),
    ];
    for refusal in refusals {
        let code = refusal.as_ref().map(Error::code);
        assert_eq!(code, Some("record_names_repeat"));
        match refusal {
            Some(Error::RecordNamesRepeat(names)) => assert_eq!(names, ["first"]),
            other => panic!("expected a refusal naming the clash, got {other:?}"),
        }
    }
    assert!(!root.join("part.gb").exists());
}

#[test]
fn an_edit_naming_another_record_is_refused() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("two.gb");
    std::fs::write(&file, record("first") + &record("second")).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&file).unwrap();
    let refused = s.apply(d, replace(Some("second"))).unwrap_err();
    assert!(
        refused.to_string().contains("only first can be edited"),
        "{refused}"
    );
    assert!(s.apply(d, replace(Some("missing"))).is_err());
    assert!(s.apply(d, replace(Some("first"))).is_ok());
}
