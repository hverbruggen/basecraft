//! Known producer shapes only; no strict production guard/repair decoder is added.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{
    records,
    records::{AlignmentLink, AlignmentLinkExt},
};
use basecraft_protocol::link_records::{AlignmentLinkRecovery, AlignmentLinkUnavailable};
use serde::Serialize;
use serde_json::{Value, json};

fn sorted_bytes(record: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::to_value(record).unwrap()).unwrap()
}

#[test]
fn original_repair_and_guard_goldens_roundtrip() {
    crate::isolated_profile::isolate!();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for label in ["absent", "file", "symlink"] {
        let bytes = std::fs::read(root.join(format!("link-repair-{label}.json"))).unwrap();
        let record: AlignmentLinkRecovery = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(sorted_bytes(&record), bytes);
        let bytes = std::fs::read(root.join(format!("link-guard-{label}.json"))).unwrap();
        // The committed guard captures contain Serde's Unix-native OsString.
        // Foreign-platform fixtures remain inspectable JSON, not a promise that
        // OsString deserializes a different platform's native representation.
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec_pretty(&value).unwrap(), bytes);
        #[cfg(unix)]
        {
            let record: AlignmentLinkUnavailable = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(sorted_bytes(&record), bytes);
        }
    }
    // Synthetic Option shape, not an actual retained-filename capture.
    let guard = AlignmentLinkUnavailable {
        schema: 1,
        unavailable: "reason".into(),
        recovery: None,
    };
    assert_eq!(
        serde_json::to_value(guard).unwrap(),
        json!({"schema":1,"unavailable":"reason","recovery":null})
    );
}

#[test]
fn selected_guard_fields_keep_existing_acceptance_and_view_refusal() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let tree = dir.path().join("tree.nwk");
    std::fs::write(&tree, "(a,b);").unwrap();
    let origin = records::sidecar(&tree);
    let view = records::view_sidecar(&tree);
    let valid = include_bytes!("../../../testdata/fixtures/records-v1/link-snapshot-inferred.json");
    let link: Value = serde_json::from_slice(valid).unwrap();
    std::fs::write(&origin, basecraft_protocol::records::encode(&link).unwrap()).unwrap();
    for reason in ["", "unavailable test"] {
        for recovery in [
            None,
            Some(Value::Null),
            Some(json!(false)),
            Some(json!(42)),
            Some(json!({})),
            Some(json!({"Unix":[114,101,112,97,105,114]})),
            Some(json!("repair.json")),
        ] {
            for schema in [None, Some(json!(999)), Some(json!("invalid"))] {
                let mut value = json!({"unavailable":reason,"future":{"nested":[null,true]}});
                if let Some(recovery) = recovery.clone() {
                    value["recovery"] = recovery;
                }
                if let Some(schema) = schema {
                    value["schema"] = schema;
                }
                let before = basecraft_protocol::records::encode(&value).unwrap();
                std::fs::write(&view, &before).unwrap();
                let error = AlignmentLink::read_view(&tree).unwrap_err();
                let expected = if value["recovery"].is_string() {
                    format!(
                        "{reason}; recover association metadata from {}",
                        dir.path().join("repair.json").display()
                    )
                } else {
                    reason.to_owned()
                };
                assert_eq!(error.to_string(), format!("invalid operation: {expected}"));
                assert_eq!(std::fs::read(&view).unwrap(), before);
                // The valid scientific origin must not replace the unusable explicit view.
                assert!(AlignmentLink::read(&tree).is_ok());
            }
        }
    }
    for unavailable in [Value::Null, json!(false), json!(42), json!({})] {
        let mut value: Value = serde_json::from_slice(valid).unwrap();
        value["unavailable"] = unavailable;
        std::fs::write(&view, basecraft_protocol::records::encode(&value).unwrap()).unwrap();
        assert!(
            AlignmentLink::read_view(&tree).is_ok(),
            "nonstring guard falls through"
        );
    }
    std::fs::write(&view, b"malformed JSON").unwrap();
    assert!(matches!(
        AlignmentLink::read_view(&tree),
        Err(basecraft_ops::Error::Json(_))
    ));
}
