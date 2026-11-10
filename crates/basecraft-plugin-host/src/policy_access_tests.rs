use super::*;
use basecraft_ops::access::{ServiceAction, TrustedLegacy};

fn denied_as(result: basecraft_ops::access::Result<PathBuf>, expected: Denial) {
    assert!(matches!(result, Err(AccessError::Denied(reason)) if reason == expected));
}

#[test]
fn lexical_denials_do_not_probe_any_path() {
    let project = tempfile::tempdir().unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    for (path, reason) in [
        (PathBuf::new(), Denial::InvalidPath),
        (PathBuf::from("../forbidden"), Denial::InvalidPath),
        (
            project.path().join("child/../../forbidden"),
            Denial::InvalidPath,
        ),
        (
            project.path().parent().unwrap().join("forbidden"),
            Denial::OutsideRoot,
        ),
    ] {
        denied_as(
            policy.resolve_resource_with(
                &path,
                ResourceAction::Probe,
                &mut |_| panic!("denied spelling must not probe metadata"),
                &mut |_| panic!("denied spelling must not open for identity"),
            ),
            reason,
        );
    }
}

#[test]
fn allowed_actions_are_checks_only_and_keep_public_binding_rules() {
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("input");
    std::fs::write(&file, "original").unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    assert_eq!(policy.boundary(), Some(project.path()));
    for action in [
        ResourceAction::Probe,
        ResourceAction::ReadFile,
        ResourceAction::ReplaceFile,
        ResourceAction::RenameSource,
        ResourceAction::Cleanup,
        ResourceAction::SyncFile,
    ] {
        assert_eq!(policy.resource(&file, action).unwrap(), file);
    }
    for action in [ResourceAction::ReadDirectory, ResourceAction::SyncDirectory] {
        assert_eq!(
            policy.resource(project.path(), action).unwrap(),
            project.path()
        );
    }
    for action in [
        ResourceAction::CreateFile,
        ResourceAction::CreateDirectory,
        ResourceAction::RenameDestination,
        ResourceAction::Probe,
    ] {
        assert!(policy.resource(Path::new("new"), action).is_ok());
    }
    assert!(!project.path().join("new").exists());
    assert_eq!(std::fs::read(&file).unwrap(), b"original");
    assert_eq!(policy.path("input", false).unwrap(), file);
    assert!(policy.path("new", true).is_ok());
    assert!(policy.path("input", true).is_err());
    assert!(policy.path(".", false).is_err());
    assert!(matches!(
        policy.resource(&file, ResourceAction::CreateFile),
        Err(AccessError::DestinationExists)
    ));
    // Only the final missing component is permitted; do not recursively create.
    assert!(
        matches!(policy.resource(Path::new("missing/child"), ResourceAction::CreateFile),
        Err(AccessError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
}

#[test]
fn mutation_requires_both_effects_and_rechecks_each_call() {
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("input");
    std::fs::write(&file, "original").unwrap();
    let mut policy = SessionPolicy::new(project.path(), true).unwrap();
    assert!(policy.resource(&file, ResourceAction::ReplaceFile).is_ok());
    for (document, artifact) in [(false, false), (false, true), (true, false)] {
        policy.document_write = document;
        policy.artifact_write = artifact;
        for action in [
            ResourceAction::CreateFile,
            ResourceAction::CreateDirectory,
            ResourceAction::ReplaceFile,
            ResourceAction::RenameSource,
            ResourceAction::RenameDestination,
            ResourceAction::Cleanup,
            ResourceAction::SyncFile,
            ResourceAction::SyncDirectory,
        ] {
            // Even a nonexistent/outside spelling yields the effect denial first.
            denied_as(
                policy.resource(Path::new("../forbidden"), action),
                Denial::ReadOnly,
            );
        }
        assert!(policy.resource(&file, ResourceAction::ReadFile).is_ok());
    }
    assert_eq!(std::fs::read(file).unwrap(), b"original");
}

#[test]
fn root_cannot_be_removed_or_renamed_and_ancestor_probe_stops() {
    let project = tempfile::tempdir().unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    for action in [
        ResourceAction::Cleanup,
        ResourceAction::RenameSource,
        ResourceAction::ReplaceFile,
    ] {
        denied_as(
            policy.resolve_resource_with(
                project.path(),
                action,
                &mut |_| panic!("root mutation refused before probe"),
                &mut |_| panic!("root mutation refused before open"),
            ),
            Denial::RootMutation,
        );
    }
    denied_as(
        policy.resolve_resource_with(
            project.path().parent().unwrap(),
            ResourceAction::Probe,
            &mut |_| panic!("no ancestor probes"),
            &mut |_| panic!("no ancestor opens"),
        ),
        Denial::OutsideRoot,
    );
}

#[test]
fn services_fail_closed_without_discovery_or_caller_paths() {
    let project = tempfile::tempdir().unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    for action in [
        ServiceAction::ReadPreferences,
        ServiceAction::ReadAnalysisRegistry,
        ServiceAction::LockAnalysisEvidence,
        ServiceAction::ReserveSaveTarget,
        ServiceAction::RegisterFinalStore,
    ] {
        assert!(matches!(
            policy.service(action),
            Err(AccessError::Denied(Denial::ServiceUnavailable))
        ));
    }
    assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
    // Legacy bypass must be explicitly selected and still grants no service here.
    assert_eq!(TrustedLegacy.boundary(), None);
    assert!(
        TrustedLegacy
            .resource(Path::new("../legacy"), ResourceAction::ReadFile)
            .is_ok()
    );
    assert!(
        TrustedLegacy
            .service(ServiceAction::ReadAnalysisRegistry)
            .is_err()
    );
}

#[test]
fn hardlinks_are_rejected_for_probes_reads_and_mutations() {
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("input");
    std::fs::write(&file, "original").unwrap();
    std::fs::hard_link(&file, project.path().join("alias")).unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    for action in [
        ResourceAction::Probe,
        ResourceAction::ReadFile,
        ResourceAction::ReplaceFile,
        ResourceAction::RenameSource,
        ResourceAction::Cleanup,
        ResourceAction::SyncFile,
    ] {
        denied_as(policy.resource(&file, action), Denial::Hardlink);
    }
    assert!(policy.path("input", false).is_err());
    assert_eq!(std::fs::read(file).unwrap(), b"original");
}

#[cfg(unix)]
#[test]
fn symbolic_ancestors_stop_before_child_probe_and_are_rechecked() {
    use std::os::unix::fs::symlink;
    let project = tempfile::tempdir().unwrap();
    let forbidden = tempfile::tempdir().unwrap();
    let parent = project.path().join("child");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(parent.join("input"), "permitted").unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    assert!(
        policy
            .resource(&parent.join("input"), ResourceAction::ReadFile)
            .is_ok()
    );
    std::fs::rename(&parent, project.path().join("previous")).unwrap();
    symlink(forbidden.path(), &parent).unwrap();
    let mut probed = Vec::new();
    denied_as(
        policy.resolve_resource_with(
            &parent.join("input"),
            ResourceAction::ReadFile,
            &mut |path| {
                probed.push(path.to_path_buf());
                path.symlink_metadata()
            },
            &mut |_| panic!("no identity open after symbolic denial"),
        ),
        Denial::SymbolicLink,
    );
    assert_eq!(probed, vec![project.path().to_path_buf(), parent]);
    assert!(policy.path("child/input", false).is_err());
    symlink(
        forbidden.path().join("absent"),
        project.path().join("dangling"),
    )
    .unwrap();
    denied_as(
        policy.resource(Path::new("dangling"), ResourceAction::CreateFile),
        Denial::SymbolicLink,
    );
}

#[cfg(unix)]
#[test]
fn substituted_root_is_rejected_before_descendant_probe() {
    use std::os::unix::fs::symlink;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let other = fixture.path().join("other");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&other).unwrap();
    let policy = SessionPolicy::new(&root, true).unwrap();
    std::fs::remove_dir(&root).unwrap();
    symlink(other, &root).unwrap();
    let mut probes = 0;
    denied_as(
        policy.resolve_resource_with(
            &root.join("input"),
            ResourceAction::Probe,
            &mut |path| {
                probes += 1;
                assert_eq!(path, root);
                path.symlink_metadata()
            },
            &mut |_| panic!("no open"),
        ),
        Denial::SymbolicLink,
    );
    assert_eq!(probes, 1);
}

#[test]
fn io_errors_remain_distinct_from_policy_denial() {
    let project = tempfile::tempdir().unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    let result = policy.resolve_resource_with(
        Path::new("input"),
        ResourceAction::ReadFile,
        &mut |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        &mut |_| panic!("no open after metadata failure"),
    );
    assert!(matches!(result, Err(AccessError::Io(error))
        if error.kind() == std::io::ErrorKind::PermissionDenied));
}
