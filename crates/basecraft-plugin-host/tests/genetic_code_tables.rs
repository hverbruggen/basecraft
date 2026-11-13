//! Plugins read genetic codes from the Python SDK (`basecraft.genetic_code`);
//! basecraft reads its own (`basecraft_docs::genetic_code`). They must hold
//! the same tables.
use basecraft_plugin_host::process::python_interpreter;
use std::path::Path;

#[test]
fn the_sdk_holds_basecraft_s_genetic_codes() {
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src");
    let script = "import json, sys; sys.path.insert(0, sys.argv[1]); \
        from basecraft.genetic_code import CODES, CODONS; \
        print(json.dumps({'codons': CODONS, 'codes': {k: list(v) for k, v in CODES.items()}}))";
    let output = std::process::Command::new(python_interpreter())
        .args(["-c", script, sdk.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sdk: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let codons: Vec<String> = serde_json::from_value(sdk["codons"].clone()).unwrap();
    let codes = sdk["codes"].as_object().unwrap();
    assert_eq!(codes.len(), basecraft_docs::genetic_code::CODES.len());
    for code in basecraft_docs::genetic_code::CODES {
        let [name, amino_acids, starts] = &codes[&code.id.to_string()].as_array().unwrap()[..]
        else {
            panic!("code {} has no table in the SDK", code.id)
        };
        assert_eq!(name, code.name);
        for (i, codon) in codons.iter().enumerate() {
            let codon = codon.as_bytes();
            assert_eq!(
                amino_acids.as_str().unwrap().as_bytes()[i],
                code.translate_codon(codon),
                "code {} codon {}",
                code.id,
                String::from_utf8_lossy(codon)
            );
            assert_eq!(
                starts.as_str().unwrap().as_bytes()[i] == b'M',
                code.is_start(codon),
                "code {} start {}",
                code.id,
                String::from_utf8_lossy(codon)
            );
        }
    }
}
