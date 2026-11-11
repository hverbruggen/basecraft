use super::*;
use basecraft_platform::{paths::AppPaths, settings::ProvenancePolicy};
use std::{cell::RefCell, fmt::Write, fs, io::ErrorKind};

thread_local! {
    static CALLS: RefCell<Vec<ResolverStage>> = const { RefCell::new(Vec::new()) };
}
pub(super) fn called(stage: ResolverStage) {
    CALLS.with_borrow_mut(|calls| calls.push(stage));
}
fn take_calls() -> Vec<ResolverStage> {
    CALLS.with_borrow_mut(std::mem::take)
}
fn stage(step: &ResolverStep) -> ResolverStage {
    match step {
        ResolverStep::Binding(_) => ResolverStage::Binding,
        ResolverStep::Metadata(stage, _, _) => *stage,
        ResolverStep::Canonicalize(_) => ResolverStage::Canonicalize,
        ResolverStep::ProjectRelative(_, _) => ResolverStage::ProjectRelative,
        ResolverStep::Discover(_) => ResolverStage::Discover,
        ResolverStep::Settings(_) => ResolverStage::Settings,
    }
}

// Inventory follows no links and includes absent roots, bytes, link spelling,
// permissions, mtime and (on Unix) stable object identity. Read atime excluded.
fn inventory(root: &Path) -> Vec<(PathBuf, String)> {
    fn visit(path: &Path, rows: &mut Vec<(PathBuf, String)>) {
        let Ok(m) = fs::symlink_metadata(path) else {
            rows.push((path.to_path_buf(), "absent".into()));
            return;
        };
        let mut record = format!(
            "{:?}:{:?}:{:?}",
            m.file_type(),
            m.permissions(),
            m.modified()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            write!(record, ":{}:{}", m.dev(), m.ino()).unwrap();
        }
        if m.file_type().is_symlink() {
            write!(record, ":{:?}", fs::read_link(path).unwrap()).unwrap();
        } else if m.is_file() {
            write!(record, ":{:?}", fs::read(path).map_err(|e| e.kind())).unwrap();
        }
        rows.push((path.to_path_buf(), record));
        if m.is_dir() {
            match fs::read_dir(path) {
                Ok(entries) => {
                    let mut paths: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
                    paths.sort();
                    for child in paths {
                        visit(&child, rows);
                    }
                }
                Err(error) => {
                    rows.push((path.to_path_buf(), format!("read-dir:{:?}", error.kind())));
                }
            }
        }
    }
    let mut rows = Vec::new();
    visit(root, &mut rows);
    for name in [
        "profile",
        "profile/state",
        "profile/cache",
        "scratch",
        "support",
    ] {
        visit(&root.join(name), &mut rows);
    }
    rows
}

fn check(
    root: &Path,
    source: &Path,
    expected: &Path,
    selection: BundleSelection,
) -> BundleObservation {
    let before = inventory(root);
    take_calls();
    let observation = observe_bundle_for_result(source);
    let calls = take_calls();
    assert_eq!(
        observation.selected.as_os_str().as_encoded_bytes(),
        expected.as_os_str().as_encoded_bytes()
    );
    assert_eq!(observation.selected_by, selection);
    assert_eq!(
        calls,
        observation.steps.iter().map(stage).collect::<Vec<_>>()
    );
    assert_eq!(
        calls
            .iter()
            .filter(|&&s| s == ResolverStage::Binding)
            .count(),
        1
    );
    for single in [
        ResolverStage::HiddenManifest,
        ResolverStage::CompanionManifest,
        ResolverStage::Canonicalize,
        ResolverStage::ProjectRelative,
        ResolverStage::Discover,
        ResolverStage::Settings,
        ResolverStage::LocalManifest,
    ] {
        assert!(calls.iter().filter(|&&s| s == single).count() <= 1);
    }
    assert_eq!(
        bundle_for_result(source).as_os_str().as_encoded_bytes(),
        expected.as_os_str().as_encoded_bytes()
    );
    assert_eq!(
        take_calls(),
        calls,
        "legacy calls exactly the same boundaries"
    );
    assert_eq!(inventory(root), before);
    // A second observed call is independent, not cached; dropping observations
    // (including error carriers) has no filesystem effect.
    let second = observe_bundle_for_result(source);
    assert_eq!(take_calls(), calls);
    drop(second);
    assert_eq!(inventory(root), before);
    observation
}
fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn metadata(
    observation: &BundleObservation,
    stage: ResolverStage,
) -> &std::io::Result<fs::Metadata> {
    observation
        .steps
        .iter()
        .find_map(|step| match step {
            ResolverStep::Metadata(s, _, value) if *s == stage => Some(value),
            _ => None,
        })
        .unwrap()
}
fn policy(
    observation: &BundleObservation,
) -> &std::result::Result<ProvenancePolicy, basecraft_platform::settings::Error> {
    observation
        .steps
        .iter()
        .find_map(|step| match step {
            ResolverStep::Settings(value) => Some(value),
            _ => None,
        })
        .unwrap()
}
fn stages(observation: &BundleObservation) -> Vec<ResolverStage> {
    observation.steps.iter().map(stage).collect()
}

#[test]
fn binding_and_companion() {
    let Some(root) = crate::isolated_profile::enter(
        "provenance::resolver_observation_tests::binding_and_companion",
    ) else {
        return;
    };
    let source = root.join("result.txt");
    write(&source, b"result");
    let run = crate::analysis_store::ArtifactRun::create(&source, "observation-test").unwrap();
    run.bind(&source).unwrap();
    let hidden = run.directory().to_path_buf();
    let companion = companion(&source);
    write(&companion.join("manifest.json"), b"not parsed");
    write(&hidden.join("manifest.json"), b"not parsed either");
    // Unchecked discovery continues to select the binding after result changes.
    write(&source, b"changed result");
    let observed = check(&root, &source, &hidden, BundleSelection::Binding);
    assert_eq!(
        stages(&observed),
        [ResolverStage::Binding, ResolverStage::HiddenManifest]
    );
    assert!(matches!(&observed.steps[0], ResolverStep::Binding(Ok(Some(p))) if p == &hidden));
    fs::remove_file(hidden.join("manifest.json")).unwrap();
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert_eq!(
        metadata(&observed, ResolverStage::HiddenManifest)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    fs::create_dir(hidden.join("manifest.json")).unwrap();
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert!(
        !metadata(&observed, ResolverStage::HiddenManifest)
            .as_ref()
            .unwrap()
            .is_file()
    );
    let binding = root
        .join(".basecraft/analyses-v1/bindings")
        .join(basecraft_io::hash_bytes(b"result.txt").trim_start_matches("b3:"));
    let bytes = fs::read(&binding).unwrap();
    write(&binding, b"{broken");
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert!(matches!(
        &observed.steps[0],
        ResolverStep::Binding(Err(crate::Error::Json(_)))
    ));
    assert_eq!(
        stages(&observed),
        [ResolverStage::Binding, ResolverStage::CompanionManifest]
    );
    assert!(
        inspect(&source).is_err(),
        "separate fatal binding precheck remains"
    );
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["run"] = serde_json::json!("missing-run");
    write(&binding, &serde_json::to_vec(&value).unwrap());
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert!(matches!(
        &observed.steps[0],
        ResolverStep::Binding(Ok(None))
    ));
    fs::remove_file(&binding).unwrap();
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert!(matches!(
        &observed.steps[0],
        ResolverStep::Binding(Ok(None))
    ));
}

#[test]
fn project_fallback_and_settings() {
    let Some(root) = crate::isolated_profile::enter(
        "provenance::resolver_observation_tests::project_fallback_and_settings",
    ) else {
        return;
    };
    let source = root.join("outer/inner/result.txt");
    write(&source, b"result");
    let companion = companion(&source);
    let paths = AppPaths::discover().unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert_eq!(
        *policy(&observed).as_ref().unwrap(),
        ProvenancePolicy::Portable
    );
    assert!(
        !paths.preferences().exists(),
        "successful policy has unknown origin"
    );
    assert!(!stages(&observed).contains(&ResolverStage::LocalManifest));
    for (text, expected) in [
        ("portable", ProvenancePolicy::Portable),
        ("none", ProvenancePolicy::None),
        ("local-only", ProvenancePolicy::LocalOnly),
    ] {
        write(
            &paths.preferences(),
            format!("[files]\nprovenance = '{text}'\n").as_bytes(),
        );
        let observed = check(
            &root,
            &source,
            &companion,
            BundleSelection::CompanionFallback,
        );
        assert_eq!(*policy(&observed).as_ref().unwrap(), expected);
        assert_eq!(
            stages(&observed).contains(&ResolverStage::LocalManifest),
            expected == ProvenancePolicy::LocalOnly
        );
    }
    let local = paths.state.join("provenance").join(
        basecraft_io::hash_bytes(
            source
                .canonicalize()
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        )
        .trim_start_matches("b3:"),
    );
    write(&local.join("manifest.json"), b"not parsed");
    check(&root, &source, &local, BundleSelection::Local);
    fs::remove_file(local.join("manifest.json")).unwrap();
    fs::create_dir(local.join("manifest.json")).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert!(
        !metadata(&observed, ResolverStage::LocalManifest)
            .as_ref()
            .unwrap()
            .is_file()
    );
    for bytes in [b"[malformed".to_vec(), vec![b'x'; (1 << 20) + 1]] {
        write(&paths.preferences(), &bytes);
        let observed = check(
            &root,
            &source,
            &companion,
            BundleSelection::CompanionFallback,
        );
        assert!(matches!(
            policy(&observed),
            Err(basecraft_platform::settings::Error::Invalid(_))
        ));
        assert!(!stages(&observed).contains(&ResolverStage::LocalManifest));
    }
    for directory in [root.join("outer"), root.join("outer/inner")] {
        write(
            &directory.join(".basecraft/project.toml"),
            b"malformed marker content is irrelevant",
        );
    }
    let expected = root
        .join("outer/inner/.basecraft/provenance")
        .join(basecraft_io::hash_bytes(b"result.txt").trim_start_matches("b3:"));
    let observed = check(&root, &source, &expected, BundleSelection::Project);
    assert_eq!(
        stages(&observed),
        [
            ResolverStage::Binding,
            ResolverStage::CompanionManifest,
            ResolverStage::Canonicalize,
            ResolverStage::ProjectMarker,
            ResolverStage::ProjectRelative
        ]
    );
    assert!(!expected.exists());
    assert!(matches!(&observed.steps[2], ResolverStep::Canonicalize(Ok(p)) if p == &source));
    assert!(
        matches!(&observed.steps[4], ResolverStep::ProjectRelative(p, Ok(r)) if p == &root.join("outer/inner") && r == Path::new("result.txt"))
    );
    let marker = root.join("outer/inner/.basecraft/project.toml");
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    let expected = root
        .join("outer/.basecraft/provenance")
        .join(basecraft_io::hash_bytes(b"inner/result.txt").trim_start_matches("b3:"));
    let observed = check(&root, &source, &expected, BundleSelection::Project);
    assert_eq!(
        stages(&observed)
            .iter()
            .filter(|&&s| s == ResolverStage::ProjectMarker)
            .count(),
        2
    );
    // Relative missing spelling is retained after exactly one failed canonicalize.
    let relative = Path::new("missing-observation-parent/../result.txt");
    let observed = check(
        &root,
        relative,
        &companion_for(relative),
        BundleSelection::CompanionFallback,
    );
    assert!(observed.steps.iter().any(|s| matches!(s, ResolverStep::Canonicalize(Err(e)) if e.kind() == ErrorKind::NotFound && e.raw_os_error().is_some())));
}
fn companion_for(path: &Path) -> PathBuf {
    super::companion(path)
}

#[test]
fn discovery_failure() {
    let name = "provenance::resolver_observation_tests::discovery_failure";
    let Some(root) = crate::isolated_profile::enter(name) else {
        return;
    };
    if std::env::var_os("RESOLVER_BAD_DISCOVERY").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("RESOLVER_BAD_DISCOVERY", "1")
            .env("BASECRAFT_PROFILE_DIR", "relative-profile")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let source = root.join("absent");
    let observed = check(
        &root,
        &source,
        &companion(&source),
        BundleSelection::CompanionFallback,
    );
    assert!(matches!(
        observed.steps.last(),
        Some(ResolverStep::Discover(Err(
            basecraft_platform::paths::PathError(_)
        )))
    ));
    assert!(!stages(&observed).contains(&ResolverStage::Settings));
}

#[cfg(unix)]
#[test]
fn native_paths_links_and_denials() {
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    };
    let Some(root) = crate::isolated_profile::enter(
        "provenance::resolver_observation_tests::native_paths_links_and_denials",
    ) else {
        return;
    };
    let source = root.join(std::ffi::OsString::from_vec(b"result-\xff.txt".to_vec()));
    write(&source, b"result");
    let companion = companion(&source);
    let manifest = companion.join("manifest.json");
    let target = root.join("support/target");
    write(&target, b"unparsed");
    fs::create_dir_all(&companion).unwrap();
    symlink(&target, &manifest).unwrap();
    let observed = check(&root, &source, &companion, BundleSelection::Companion);
    assert!(
        metadata(&observed, ResolverStage::CompanionManifest)
            .as_ref()
            .unwrap()
            .is_file()
    );
    fs::remove_file(&target).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert_eq!(
        metadata(&observed, ResolverStage::CompanionManifest)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    fs::remove_file(&manifest).unwrap();
    fs::create_dir(&manifest).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert!(
        !metadata(&observed, ResolverStage::CompanionManifest)
            .as_ref()
            .unwrap()
            .is_file()
    );
    fs::remove_dir(&manifest).unwrap();
    let denied = root.join("denied");
    write(&denied.join("file"), b"blocked by parent");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    assert_eq!(
        fs::metadata(denied.join("file")).unwrap_err().kind(),
        ErrorKind::PermissionDenied,
        "must run as ordinary uid, not skip denial checks"
    );
    symlink(denied.join("file"), &manifest).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    let error = metadata(&observed, ResolverStage::CompanionManifest)
        .as_ref()
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert!(error.raw_os_error().is_some());
    let paths = AppPaths::discover().unwrap();
    write(
        &paths.preferences(),
        b"[files]\nprovenance = 'local-only'\n",
    );
    let local = paths.state.join("provenance").join(
        basecraft_io::hash_bytes(source.as_os_str().as_encoded_bytes()).trim_start_matches("b3:"),
    );
    let local_manifest = local.join("manifest.json");
    fs::create_dir_all(&local).unwrap();
    symlink(denied.join("file"), &local_manifest).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert_eq!(
        metadata(&observed, ResolverStage::LocalManifest)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::PermissionDenied
    );
    fs::remove_file(&local_manifest).unwrap();
    symlink(&target, &local_manifest).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert_eq!(
        metadata(&observed, ResolverStage::LocalManifest)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    write(&target, b"still unparsed");
    check(&root, &source, &local, BundleSelection::Local);
    fs::set_permissions(paths.preferences(), fs::Permissions::from_mode(0o0)).unwrap();
    let observed = check(
        &root,
        &source,
        &companion,
        BundleSelection::CompanionFallback,
    );
    assert!(
        matches!(policy(&observed), Err(basecraft_platform::settings::Error::Io(e)) if e.kind() == ErrorKind::PermissionDenied && e.raw_os_error().is_some())
    );
    fs::set_permissions(paths.preferences(), fs::Permissions::from_mode(0o600)).unwrap();
    let marker = root.join(".basecraft/project.toml");
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    symlink(denied.join("file"), &marker).unwrap();
    let observed = check(&root, &source, &local, BundleSelection::Local);
    assert_eq!(
        metadata(&observed, ResolverStage::ProjectMarker)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::PermissionDenied
    );
    fs::remove_file(&marker).unwrap();
    symlink(&target, &marker).unwrap();
    let key = basecraft_io::hash_bytes(source.file_name().unwrap().to_string_lossy().as_bytes());
    let expected = root
        .join(".basecraft/provenance")
        .join(key.trim_start_matches("b3:"));
    assert_ne!(
        key,
        basecraft_io::hash_bytes(source.file_name().unwrap().as_encoded_bytes())
    );
    check(&root, &source, &expected, BundleSelection::Project);
    // Canonicalization follows the source link, while companion uses its exact
    // original spelling. Project selection hashes the target-relative name.
    let link = root.join("alias");
    symlink(&source, &link).unwrap();
    check(&root, &link, &expected, BundleSelection::Project);
    fs::remove_file(&marker).unwrap();
    // Real binding with manifest pointing into a denied directory falls through.
    let bound = root.join("bound.txt");
    write(&bound, b"bound");
    let run = crate::analysis_store::ArtifactRun::create(&bound, "denied-manifest").unwrap();
    run.bind(&bound).unwrap();
    symlink(denied.join("file"), run.directory().join("manifest.json")).unwrap();
    let observed = check(
        &root,
        &bound,
        &companion_for(&bound),
        BundleSelection::CompanionFallback,
    );
    assert_eq!(
        metadata(&observed, ResolverStage::HiddenManifest)
            .as_ref()
            .unwrap_err()
            .kind(),
        ErrorKind::PermissionDenied
    );
    // Content unreadable does not make following metadata fail.
    fs::remove_file(&manifest).unwrap();
    write(&manifest, b"unreadable but regular");
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o0)).unwrap();
    check(&root, &source, &companion, BundleSelection::Companion);
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn species_owner_and_relative_companion() {
    let name = "provenance::resolver_observation_tests::species_owner_and_relative_companion";
    let Some(root) = crate::isolated_profile::enter(name) else {
        return;
    };
    if std::env::var_os("RESOLVER_RELATIVE_CASE").is_some() {
        let source = Path::new("./nested/../relative.txt");
        fs::create_dir_all(root.join("nested")).unwrap();
        write(&root.join("relative.txt"), b"source");
        write(
            &root.join("relative.txt.provenance/manifest.json"),
            b"unparsed",
        );
        let observed = check(
            &root,
            source,
            Path::new("./nested/../relative.txt.provenance"),
            BundleSelection::Companion,
        );
        assert_eq!(
            stages(&observed),
            [ResolverStage::Binding, ResolverStage::CompanionManifest]
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .current_dir(&root)
        .env("RESOLVER_RELATIVE_CASE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let run = crate::analysis_store::ArtifactRun::create(
        &root.join("species.txt"),
        "species-observation",
    )
    .unwrap();
    let source = run.directory().join("species.txt");
    write(&source, b"species");
    write(&run.directory().join("manifest.json"), b"unparsed");
    let mut input = crate::input_evidence::SubmittedInput {
        schema_version: 1,
        source: None,
        submitted_hash: basecraft_io::hash_bytes(b"species"),
        submitted_size: 7,
        transformation: "identity-v1".into(),
        state: "generated".into(),
        extra: std::collections::BTreeMap::new(),
    };
    input
        .extra
        .insert("species_result".into(), serde_json::json!("species.txt"));
    crate::input_evidence::write_json(&run.directory().join("input.json"), &input).unwrap();
    check(&root, &source, run.directory(), BundleSelection::Binding);
    input
        .extra
        .insert("species_result".into(), serde_json::json!("different.txt"));
    crate::input_evidence::write_json(&run.directory().join("input.json"), &input).unwrap();
    let observed = check(
        &root,
        &source,
        &companion(&source),
        BundleSelection::CompanionFallback,
    );
    assert!(matches!(
        &observed.steps[0],
        ResolverStep::Binding(Ok(None))
    ));
    write(&run.directory().join("input.json"), b"malformed");
    let observed = check(
        &root,
        &source,
        &companion(&source),
        BundleSelection::CompanionFallback,
    );
    assert!(matches!(
        &observed.steps[0],
        ResolverStep::Binding(Err(crate::Error::Json(_)))
    ));
}
