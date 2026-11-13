//! Original-literal byte oracles, executed before writer substitution.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::ArtifactRun;
use serde_json::json;

#[test]
fn result_binding_writer_bytes_and_selective_reader() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("result.txt");
    let run = ArtifactRun::create(&output, "binding-probe").unwrap();
    for name in ["result.txt", "nested/ünicode result.txt"] {
        let result = root.path().join(name);
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(&result, b"bound result bytes").unwrap();
        run.bind(&result).unwrap();
        let key = basecraft_io::hash_bytes(name.as_bytes());
        let binding = root
            .path()
            .join(".basecraft/analyses-v1/bindings")
            .join(key.trim_start_matches("b3:"));
        let bytes = std::fs::read(&binding).unwrap();
        let original = json!({"schema_version":1,"run":run.id(),"result":name,"result_hash":basecraft_io::hash_file(&result).unwrap()});
        let expected = basecraft_protocol::records::encode(&original).unwrap();
        assert_eq!(bytes, expected);
        if let Some(capture) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
            std::fs::create_dir_all(&capture).unwrap();
            std::fs::write(
                std::path::Path::new(&capture).join(if name == "result.txt" {
                    "binding.json"
                } else {
                    "binding-nested.json"
                }),
                &bytes,
            )
            .unwrap();
        }
        assert_eq!(
            ArtifactRun::for_result_checked(&result).unwrap().as_deref(),
            Some(run.directory())
        );
        // The existing reader deliberately accepts unknown extras and ignores
        // the hash on discovery, while checked access verifies exact bytes.
        let mut extended = original.clone();
        extended["future"] = json!({"z":[null,{"a":true}]});
        std::fs::write(
            &binding,
            basecraft_protocol::records::encode(&extended).unwrap(),
        )
        .unwrap();
        assert!(ArtifactRun::for_result_checked(&result).unwrap().is_some());
        let mut no_hash = extended.clone();
        no_hash.as_object_mut().unwrap().remove("result_hash");
        std::fs::write(
            &binding,
            basecraft_protocol::records::encode(&no_hash).unwrap(),
        )
        .unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_some());
        assert!(ArtifactRun::for_result_checked(&result).is_err());
        let mut missing_run = original.clone();
        missing_run["run"] = json!("absent");
        std::fs::write(
            &binding,
            basecraft_protocol::records::encode(&missing_run).unwrap(),
        )
        .unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_none());
        assert!(ArtifactRun::for_result_checked(&result).is_err());
        std::fs::write(
            &binding,
            basecraft_protocol::records::encode(&extended).unwrap(),
        )
        .unwrap();
        std::fs::write(&result, b"changed").unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_some());
        assert!(ArtifactRun::for_result_checked(&result).is_err());
        for bad in [
            json!({}),
            json!({"schema_version":2,"result":name}),
            json!({"schema_version":1,"result":name,"run":"../escape"}),
        ] {
            std::fs::write(&binding, basecraft_protocol::records::encode(&bad).unwrap()).unwrap();
            assert!(ArtifactRun::for_result(&result).is_err());
        }
        std::fs::write(&binding, b"{broken").unwrap();
        assert!(ArtifactRun::for_result(&result).is_err());
        std::fs::remove_file(&binding).unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_none());
    }
}

#[test]
fn poison_presence_refuses_both_recovery_routes_without_parsing() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("recovered");
    for bytes in [b"".as_slice(), b"{broken", b"{\"schema_version\":999}"] {
        std::fs::write(root.path().join("recovery-integrity-error.json"), bytes).unwrap();
        for error in [
            basecraft_ops::records::recover(root.path(), &destination, None).unwrap_err(),
            basecraft_ops::records::recover_verified_prefix(root.path(), &destination, None)
                .unwrap_err(),
        ] {
            assert!(
                error
                    .to_string()
                    .contains("journal durability was uncertain")
            );
        }
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read(root.path().join("recovery-integrity-error.json")).unwrap(),
            bytes
        );
    }
}

#[test]
fn integrity_and_binding_examples_round_trip_with_unknown_fields() {
    use basecraft_protocol::records::{RecoveryIntegrityError, ResultBinding};
    fn check<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str, newline: bool) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/records-v1")
            .join(name);
        let bytes = std::fs::read(path).unwrap();
        let parsed: T = serde_json::from_slice(&bytes).unwrap();
        let mut actual = serde_json::to_vec_pretty(&serde_json::to_value(parsed).unwrap()).unwrap();
        if newline {
            actual.push(b'\n');
        }
        assert_eq!(actual, bytes, "{name}");
        let mut extended: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        extended["future"] = json!({"z":[null,{"b":2,"a":true}],"a":"kept"});
        let parsed: T = serde_json::from_value(extended.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), extended);
    }
    crate::isolated_profile::isolate!();
    for name in ["integrity-empty.json", "integrity-nonempty.json"] {
        check::<RecoveryIntegrityError>(name, false);
    }
    for name in ["binding.json", "binding-nested.json"] {
        check::<ResultBinding>(name, true);
    }
}
