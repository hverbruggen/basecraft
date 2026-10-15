use std::process::Command;

#[test]
fn preferences_reader_is_shared_validated_and_read_only() {
    let profile = tempfile::tempdir().unwrap();
    let read = || {
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .arg("preferences")
            .env("BASECRAFT_PROFILE_DIR", profile.path())
            .output()
            .unwrap()
    };
    let defaults = read();
    assert!(defaults.status.success());
    let value: serde_json::Value = serde_json::from_slice(&defaults.stdout).unwrap();
    assert_eq!(value["compute"]["local_cpu_threads"], 4);
    assert!(!profile.path().join("config").exists());
    let config = profile.path().join("config");
    std::fs::create_dir(&config).unwrap();
    let preferences = config.join("preferences.toml");
    std::fs::write(
        &preferences,
        "schema_version=1\n[compute]\nlocal_cpu_threads=3\n",
    )
    .unwrap();
    let explicit = read();
    assert!(explicit.status.success());
    let value: serde_json::Value = serde_json::from_slice(&explicit.stdout).unwrap();
    assert_eq!(value["compute"]["local_cpu_threads"], 3);
    let invalid = "schema_version=99\n";
    std::fs::write(&preferences, invalid).unwrap();
    let rejected = read();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unsupported schema version"));
    assert_eq!(std::fs::read_to_string(&preferences).unwrap(), invalid);
    assert!(!config.join("preferences.toml.lock").exists());
}
