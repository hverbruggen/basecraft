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
    for alias in [Path::new("."), Path::new("./")] {
        denied_as(
            policy.resolve_resource_with(
                alias,
                ResourceAction::Cleanup,
                &mut |_| panic!("root alias refused before probe"),
                &mut |_| panic!("root alias refused before open"),
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

#[cfg(unix)]
#[test]
fn redirected_ancestor_above_root_refuses_output_bindings() {
    use std::os::unix::fs::symlink;
    let fixture = tempfile::tempdir().unwrap();
    let base = fixture.path().join("base");
    let retained = fixture.path().join("retained-base");
    let outside = fixture.path().join("outside");
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(outside.join("project")).unwrap();
    let policy = SessionPolicy::new(&root, true).unwrap();
    let output = Path::new("new-output");
    assert!(policy.path("new-output", true).is_ok());
    for action in [
        ResourceAction::CreateFile,
        ResourceAction::CreateDirectory,
        ResourceAction::RenameDestination,
    ] {
        assert!(policy.resource(output, action).is_ok());
    }

    // Stable redirection before the call, not a race. The root leaf itself is
    // still an ordinary directory, so descendant-only checks cannot catch it.
    std::fs::rename(&base, &retained).unwrap();
    symlink(&outside, &base).unwrap();
    assert!(root.symlink_metadata().unwrap().is_dir());
    assert!(!root.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(matches!(
        policy.path("new-output", true),
        Err(Error::Protocol(_))
    ));
    for action in [
        ResourceAction::CreateFile,
        ResourceAction::CreateDirectory,
        ResourceAction::RenameDestination,
    ] {
        denied_as(policy.resource(output, action), Denial::OutsideRoot);
    }
    // All calls only check eligibility; neither original nor redirected project
    // receives an output, even in the initially permitted control cases.
    assert_eq!(
        std::fs::read_dir(retained.join("project")).unwrap().count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(outside.join("project")).unwrap().count(),
        0
    );
}

fn strict_denied(
    result: std::result::Result<StrictResource, StrictResourceError>,
    expected: Denial,
) {
    assert!(
        matches!(result, Err(StrictResourceError::Access(AccessError::Denied(reason))) if reason == expected)
    );
}

#[test]
fn strict_pure_gates_precede_actual_inspector_boundary() {
    let project = tempfile::tempdir().unwrap();
    let mut policy = SessionPolicy::new(project.path(), true).unwrap();
    for (path, action, expected) in [
        (PathBuf::new(), ResourceAction::Probe, Denial::InvalidPath),
        (
            PathBuf::from("../outside"),
            ResourceAction::Probe,
            Denial::InvalidPath,
        ),
        (
            project.path().parent().unwrap().to_owned(),
            ResourceAction::ReadDirectory,
            Denial::OutsideRoot,
        ),
        (
            PathBuf::from("."),
            ResourceAction::Cleanup,
            Denial::RootMutation,
        ),
    ] {
        strict_denied(
            policy.resolve_resource_strict_with(&path, action, true, |_| {
                panic!("pure refusal must not reach platform inspection")
            }),
            expected,
        );
    }
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
            strict_denied(
                policy.resolve_resource_strict_with(Path::new("../outside"), action, true, |_| {
                    panic!("write refusal must precede bootstrap")
                }),
                Denial::ReadOnly,
            );
        }
    }
    policy.document_write = true;
    policy.artifact_write = true;
    strict_denied(
        policy.resolve_resource_strict_with(
            Path::new("input"),
            ResourceAction::Probe,
            false,
            |_| panic!("unsupported backend must not probe"),
        ),
        Denial::ServiceUnavailable,
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn strict_native_unsupported_does_not_affect_legacy_resolution() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("input");
    std::fs::write(&path, "bytes").unwrap();
    let policy = SessionPolicy::new(project.path(), true).unwrap();
    strict_denied(
        policy.resolve_resource_strict(&path, ResourceAction::ReadFile),
        Denial::ServiceUnavailable,
    );
    assert_eq!(
        policy.resource(&path, ResourceAction::ReadFile).unwrap(),
        path
    );
}

#[cfg(target_os = "linux")]
mod strict_linux {
    use super::*;
    use basecraft_platform::files::{NamespaceReadError, OrdinaryKind, inspect_target_namespace};
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    fn permissions(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn strict_observes_unreadable_file_without_content_open_or_legacy_reopen() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("input");
        std::fs::write(&path, "original").unwrap();
        let policy = SessionPolicy::new(project.path(), false).unwrap();
        permissions(&path, 0o0);
        let mut inspections = 0;
        let resolved = policy
            .resolve_resource_strict_with(&path, ResourceAction::ReadFile, true, |observed| {
                inspections += 1;
                assert_eq!(observed, path);
                inspect_target_namespace(observed)
            })
            .unwrap();
        assert_eq!(inspections, 1);
        assert_eq!(resolved.path, path);
        assert_eq!(
            resolved.namespace.metadata().unwrap().kind(),
            OrdinaryKind::File
        );
        assert_eq!(resolved.namespace.metadata().unwrap().links(), 1);
        let metadata = path.symlink_metadata().unwrap();
        assert_eq!(
            resolved.namespace.metadata().unwrap().identity(),
            &basecraft_platform::files::ObjectIdentity::Unix {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        );
        resolved.namespace.recheck().unwrap();
        // The explicit legacy path still invokes its content-capable link-count
        // helper and fails for this ordinary user. Strict success cannot use it.
        let legacy = policy.resource(&path, ResourceAction::ReadFile);
        permissions(&path, 0o600);
        assert!(
            matches!(legacy, Err(AccessError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied),
            "requires ordinary user"
        );
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn strict_and_legacy_share_ordinary_action_and_no_overwrite_decisions() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("input");
        std::fs::write(&path, "original").unwrap();
        let directory = project.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        let policy = SessionPolicy::new(project.path(), true).unwrap();
        for action in [
            ResourceAction::Probe,
            ResourceAction::ReadFile,
            ResourceAction::ReplaceFile,
            ResourceAction::RenameSource,
            ResourceAction::Cleanup,
            ResourceAction::SyncFile,
        ] {
            assert_eq!(
                policy.resolve_resource_strict(&path, action).unwrap().path,
                policy.resource(&path, action).unwrap()
            );
        }
        for action in [
            ResourceAction::Probe,
            ResourceAction::ReadDirectory,
            ResourceAction::SyncDirectory,
            ResourceAction::RenameSource,
            ResourceAction::Cleanup,
        ] {
            assert_eq!(
                policy
                    .resolve_resource_strict(&directory, action)
                    .unwrap()
                    .path,
                policy.resource(&directory, action).unwrap()
            );
        }
        for action in [
            ResourceAction::CreateFile,
            ResourceAction::CreateDirectory,
            ResourceAction::RenameDestination,
        ] {
            for existing in [&path, &directory] {
                assert!(matches!(
                    policy.resolve_resource_strict(existing, action),
                    Err(StrictResourceError::Access(AccessError::DestinationExists))
                ));
                assert!(matches!(
                    policy.resource(existing, action),
                    Err(AccessError::DestinationExists)
                ));
            }
        }
        for (target, action) in [
            (&path, ResourceAction::ReadDirectory),
            (&path, ResourceAction::SyncDirectory),
            (&directory, ResourceAction::ReadFile),
            (&directory, ResourceAction::ReplaceFile),
            (&directory, ResourceAction::SyncFile),
        ] {
            strict_denied(
                policy.resolve_resource_strict(target, action),
                Denial::ResourceType,
            );
            denied_as(policy.resource(target, action), Denial::ResourceType);
        }
        let new = Path::new("./directory/new");
        for action in [
            ResourceAction::Probe,
            ResourceAction::CreateFile,
            ResourceAction::CreateDirectory,
            ResourceAction::RenameDestination,
        ] {
            let strict = policy.resolve_resource_strict(new, action).unwrap();
            assert_eq!(strict.path, policy.resource(new, action).unwrap());
            assert!(strict.namespace.metadata().is_none());
            assert_eq!(
                strict.namespace.absence_error().unwrap().kind(),
                std::io::ErrorKind::NotFound
            );
            assert!(
                strict
                    .namespace
                    .absence_error()
                    .unwrap()
                    .raw_os_error()
                    .is_some()
            );
            strict.namespace.recheck().unwrap();
        }
        assert!(!directory.join("new").exists());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn strict_checked_missing_is_retained_as_original_io_not_reconstructed() {
        use std::error::Error as _;
        let project = tempfile::tempdir().unwrap();
        let policy = SessionPolicy::new(project.path(), true).unwrap();
        for action in [
            ResourceAction::ReadFile,
            ResourceAction::ReadDirectory,
            ResourceAction::ReplaceFile,
            ResourceAction::RenameSource,
            ResourceAction::Cleanup,
            ResourceAction::SyncFile,
            ResourceAction::SyncDirectory,
        ] {
            let failure = policy
                .resolve_resource_strict(Path::new("missing"), action)
                .unwrap_err();
            let StrictResourceError::Missing(namespace) = &failure else {
                panic!("expected retained checked absence")
            };
            let original = namespace.absence_error().unwrap();
            assert_eq!(original.kind(), std::io::ErrorKind::NotFound);
            assert!(original.raw_os_error().is_some());
            assert!(std::ptr::eq(
                failure
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap(),
                original
            ));
            assert_eq!(failure.to_string(), original.to_string());
            namespace.recheck().unwrap();
        }
        // An intermediate missing directory never becomes checked leaf absence.
        for action in [
            ResourceAction::Probe,
            ResourceAction::CreateFile,
            ResourceAction::ReadFile,
        ] {
            assert!(
                matches!(policy.resolve_resource_strict(Path::new("missing/leaf"), action),
                Err(StrictResourceError::Access(AccessError::Io(error)))
                if error.kind() == std::io::ErrorKind::NotFound && error.raw_os_error().is_some())
            );
        }
        assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
    }

    #[test]
    fn strict_above_root_redirect_fails_at_full_chain_inspection() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().join("base");
        let root = base.join("project");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(outside.join("project")).unwrap();
        std::fs::write(root.join("input"), "original").unwrap();
        std::fs::write(outside.join("project/input"), "outside").unwrap();
        let policy = SessionPolicy::new(&root, true).unwrap();
        assert!(
            policy
                .resolve_resource_strict(Path::new("input"), ResourceAction::ReadFile)
                .is_ok()
        );
        std::fs::rename(&base, fixture.path().join("retained")).unwrap();
        symlink(&outside, &base).unwrap();
        // Descendant traversal through the alias would encounter PermissionDenied.
        // The full no-follow chain instead rejects the alias before descending.
        permissions(&outside, 0o0);
        for (target, action) in [
            ("input", ResourceAction::ReadFile),
            ("new", ResourceAction::CreateFile),
            ("new", ResourceAction::RenameDestination),
        ] {
            let mut inspections = 0;
            let result =
                policy.resolve_resource_strict_with(Path::new(target), action, true, |path| {
                    inspections += 1;
                    inspect_target_namespace(path)
                });
            assert_eq!(inspections, 1);
            strict_denied(result, Denial::SymbolicLink);
        }
        permissions(&outside, 0o700);
        assert_eq!(
            std::fs::read(outside.join("project/input")).unwrap(),
            b"outside"
        );
        assert!(!outside.join("project/new").exists());
    }

    #[test]
    fn strict_namespace_refusals_and_retained_substitution_are_structural() {
        let project = tempfile::tempdir().unwrap();
        let file = project.path().join("input");
        std::fs::write(&file, "original").unwrap();
        let policy = SessionPolicy::new(project.path(), true).unwrap();
        let observed = policy
            .resolve_resource_strict(&file, ResourceAction::ReplaceFile)
            .unwrap();
        std::fs::hard_link(&file, project.path().join("alias")).unwrap();
        assert!(matches!(
            observed.namespace.recheck(),
            Err(NamespaceReadError::Aliased)
        ));
        strict_denied(
            policy.resolve_resource_strict(&file, ResourceAction::Probe),
            Denial::Hardlink,
        );
        let dangling = project.path().join("dangling");
        symlink(project.path().join("missing"), &dangling).unwrap();
        strict_denied(
            policy.resolve_resource_strict(&dangling, ResourceAction::CreateFile),
            Denial::SymbolicLink,
        );
        let socket = project.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        strict_denied(
            policy.resolve_resource_strict(&socket, ResourceAction::Probe),
            Denial::ResourceType,
        );
        let absent = policy
            .resolve_resource_strict(Path::new("new"), ResourceAction::CreateFile)
            .unwrap();
        std::fs::write(project.path().join("new"), "replacement").unwrap();
        assert!(matches!(
            absent.namespace.recheck(),
            Err(NamespaceReadError::Changed)
        ));
    }

    #[test]
    fn strict_root_kind_and_missing_root_keep_existing_rules() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let policy = SessionPolicy::new(&root, true).unwrap();
        let resolved = policy
            .resolve_resource_strict(Path::new("."), ResourceAction::ReadDirectory)
            .unwrap();
        assert_eq!(
            resolved.namespace.metadata().unwrap().kind(),
            OrdinaryKind::Directory
        );
        for action in [
            ResourceAction::ReplaceFile,
            ResourceAction::Cleanup,
            ResourceAction::RenameSource,
        ] {
            strict_denied(
                policy.resolve_resource_strict(&root, action),
                Denial::RootMutation,
            );
        }
        std::fs::remove_dir(&root).unwrap();
        assert!(matches!(
            policy.resolve_resource_strict(&root, ResourceAction::CreateDirectory),
            Err(StrictResourceError::Missing(_))
        ));
        std::fs::write(&root, "not a directory").unwrap();
        strict_denied(
            policy.resolve_resource_strict(&root, ResourceAction::ReadFile),
            Denial::ResourceType,
        );
    }

    #[test]
    fn strict_io_keeps_os_error_and_custom_source_without_text_classification() {
        #[derive(Debug)]
        struct Cause(std::sync::Arc<()>);
        impl std::fmt::Display for Cause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("symbolic link permission_denied")
            }
        }
        impl std::error::Error for Cause {}
        let project = tempfile::tempdir().unwrap();
        let parent = project.path().join("denied");
        std::fs::create_dir(&parent).unwrap();
        let policy = SessionPolicy::new(project.path(), true).unwrap();
        permissions(&parent, 0o0);
        let result = policy.resolve_resource_strict(&parent.join("input"), ResourceAction::Probe);
        permissions(&parent, 0o700);
        assert!(
            matches!(result, Err(StrictResourceError::Access(AccessError::Io(error)))
            if error.kind() == std::io::ErrorKind::PermissionDenied && error.raw_os_error().is_some()),
            "requires ordinary user"
        );
        let source = std::sync::Arc::new(());
        let original = source.clone();
        let result = policy.resolve_resource_strict_with(
            Path::new("input"),
            ResourceAction::Probe,
            true,
            |_| {
                Err(NamespaceReadError::Io(std::io::Error::other(Cause(
                    original,
                ))))
            },
        );
        let Err(StrictResourceError::Access(AccessError::Io(error))) = result else {
            panic!("expected original IO")
        };
        assert!(std::sync::Arc::ptr_eq(
            &source,
            &error.get_ref().unwrap().downcast_ref::<Cause>().unwrap().0
        ));
        for (error, reason) in [
            (NamespaceReadError::Unsupported, Denial::ServiceUnavailable),
            (NamespaceReadError::Changed, Denial::ServiceUnavailable),
            (NamespaceReadError::Redirected, Denial::SymbolicLink),
            (NamespaceReadError::Aliased, Denial::Hardlink),
            (NamespaceReadError::WrongKind, Denial::ResourceType),
        ] {
            strict_denied(
                policy.resolve_resource_strict_with(
                    Path::new("input"),
                    ResourceAction::Probe,
                    true,
                    |_| Err(error),
                ),
                reason,
            );
        }
    }
}
