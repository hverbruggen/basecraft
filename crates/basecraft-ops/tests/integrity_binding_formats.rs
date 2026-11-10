//! Original-literal byte oracles, executed before writer substitution.
use basecraft_ops::analysis_store::ArtifactRun;
use serde_json::json;

#[test]
fn result_binding_writer_bytes_and_selective_reader() {
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
        let mut expected = serde_json::to_vec_pretty(&original).unwrap();
        expected.push(b'\n');
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
        std::fs::write(&binding, serde_json::to_vec(&extended).unwrap()).unwrap();
        assert!(ArtifactRun::for_result_checked(&result).unwrap().is_some());
        std::fs::write(&result, b"changed").unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_some());
        assert!(ArtifactRun::for_result_checked(&result).is_err());
        for bad in [
            json!({}),
            json!({"schema_version":2,"result":name}),
            json!({"schema_version":1,"result":name,"run":"../escape"}),
        ] {
            std::fs::write(&binding, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(ArtifactRun::for_result(&result).is_err());
        }
        std::fs::write(&binding, b"{broken").unwrap();
        assert!(ArtifactRun::for_result(&result).is_err());
        std::fs::remove_file(&binding).unwrap();
        assert!(ArtifactRun::for_result(&result).unwrap().is_none());
    }
}
