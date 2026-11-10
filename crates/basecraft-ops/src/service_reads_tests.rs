use super::*;
use crate::access::{ResourceAction, TrustedLegacy};
#[cfg(target_os = "linux")]
use std::cell::RefCell;

pub(crate) fn context(root: &Path) -> ServiceReadContext {
    let paths = AppPaths::from_environment(|key| match key {
        "BASECRAFT_PROFILE_DIR" | "BASECRAFT_SCRATCH_DIR" => Some(root.into()),
        _ => None,
    })
    .unwrap();
    ServiceReadContext::from_paths(&paths).unwrap()
}

pub(crate) struct Bound<'a>(pub &'a ServiceReadContext);
impl Access for Bound<'_> {
    fn boundary(&self) -> Option<&Path> {
        None
    }
    fn resource(&self, _: &Path, _: ResourceAction) -> crate::access::Result<PathBuf> {
        panic!("service must not request resource authority")
    }
    fn service_read(
        &self,
        request: &ServiceReadRequest<'_>,
        phase: ServiceReadPhase<'_>,
    ) -> crate::access::Result<()> {
        if request.permits(self.0, &phase) {
            Ok(())
        } else {
            Err(AccessError::Denied(Denial::ServiceUnavailable))
        }
    }
}

#[test]
fn eligibility_refusal_never_bootstraps() {
    let temp = tempfile::tempdir().unwrap();
    let context = context(temp.path());
    let other = self::context(temp.path());
    let request = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadPreferences,
        target: None,
    };
    for access in [&TrustedLegacy as &dyn Access, &Bound(&other)] {
        assert!(
            request
                .read_with(access, |_| panic!("metadata"), |_| panic!("content"))
                .is_err()
        );
    }
    for action in [
        ServiceAction::LockAnalysisEvidence,
        ServiceAction::ReserveSaveTarget,
        ServiceAction::RegisterFinalStore,
    ] {
        let request = ServiceReadRequest {
            context: &context,
            action,
            target: None,
        };
        assert!(
            request
                .read_with(
                    &Bound(&context),
                    |_| panic!("metadata"),
                    |_| panic!("content")
                )
                .is_err()
        );
    }
    assert!(context.preferences(&TrustedLegacy).is_err());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn real_phase_order_and_validated_revocation() {
    struct Policy<'a> {
        context: &'a ServiceReadContext,
        trace: &'a RefCell<Vec<&'static str>>,
        revoke: bool,
    }
    impl Access for Policy<'_> {
        fn boundary(&self) -> Option<&Path> {
            None
        }
        fn resource(&self, _: &Path, _: ResourceAction) -> crate::access::Result<PathBuf> {
            panic!("resource")
        }
        fn service_read(
            &self,
            request: &ServiceReadRequest<'_>,
            phase: ServiceReadPhase<'_>,
        ) -> crate::access::Result<()> {
            let validated = matches!(&phase, ServiceReadPhase::Validated(_));
            self.trace.borrow_mut().push(if validated {
                "validated"
            } else {
                "eligibility"
            });
            assert!(request.permits(self.context, &phase));
            if validated && self.revoke {
                Err(AccessError::Denied(Denial::ReadOnly))
            } else {
                Ok(())
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let context = context(temp.path());
    let request = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadPreferences,
        target: None,
    };
    let trace = RefCell::new(Vec::new());
    for present in [false, true] {
        if present {
            std::fs::create_dir(context.preferences.parent().unwrap()).unwrap();
            std::fs::write(&context.preferences, b"fixture").unwrap();
        }
        for revoke in [true, false] {
            trace.borrow_mut().clear();
            let result = request.read_with(
                &Policy {
                    context: &context,
                    trace: &trace,
                    revoke,
                },
                |path| {
                    trace.borrow_mut().push("metadata");
                    inspect_read_namespace(path)
                },
                |ticket| {
                    trace.borrow_mut().push("consume");
                    ticket.open_read()
                },
            );
            if revoke {
                assert!(result.is_err());
                assert_eq!(*trace.borrow(), ["eligibility", "metadata", "validated"]);
            } else {
                match result.unwrap() {
                    CheckedRead::Absent(_) => {
                        assert!(!present);
                        assert_eq!(
                            *trace.borrow(),
                            ["eligibility", "metadata", "validated", "consume"]
                        );
                    }
                    CheckedRead::File(mut file) => {
                        use std::io::Read as _;
                        assert!(present);
                        trace.borrow_mut().push("content");
                        let mut bytes = Vec::new();
                        file.read_to_end(&mut bytes).unwrap();
                        assert_eq!(bytes, b"fixture");
                        assert_eq!(
                            *trace.borrow(),
                            ["eligibility", "metadata", "validated", "consume", "content"]
                        );
                    }
                }
            }
        }
        assert_eq!(
            std::fs::read_dir(temp.path()).unwrap().count(),
            usize::from(present)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn ticket_requires_exact_live_request_not_equal_context_action() {
    let temp = tempfile::tempdir().unwrap();
    let context = context(temp.path());
    let first = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadPreferences,
        target: None,
    };
    let second = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadPreferences,
        target: None,
    };
    let registry = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadAnalysisRegistry,
        target: None,
    };
    let ticket = CheckedServiceNamespace {
        request: &first,
        namespace: inspect_read_namespace(first.location().unwrap()).unwrap(),
    };
    let policy = Bound(&context);
    assert!(
        policy
            .service_read(&first, ServiceReadPhase::Validated(&ticket))
            .is_ok()
    );
    assert!(
        policy
            .service_read(&second, ServiceReadPhase::Validated(&ticket))
            .is_err()
    );
    assert!(
        policy
            .service_read(&registry, ServiceReadPhase::Validated(&ticket))
            .is_err()
    );
    // The consuming API moves the non-Clone platform ticket, closing the only
    // path to a second read; it remains borrowed from first until this move.
    assert!(matches!(
        ticket.namespace.open_read().unwrap(),
        CheckedRead::Absent(_)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn settings_reuses_checked_parser_and_never_writes() {
    let temp = tempfile::tempdir().unwrap();
    let context = context(temp.path());
    let policy = Bound(&context);
    assert_eq!(
        context.preferences(&policy).unwrap(),
        Preferences::default()
    );
    assert!(!context.preferences.parent().unwrap().exists());
    std::fs::create_dir(context.preferences.parent().unwrap()).unwrap();
    // Same S1a fixtures, exercised now through both contextual phases.
    for bytes in [
        b"schema_version = 0\n[ui]\nshow_gc = false\n[future]\nvalue = 42\n".to_vec(),
        b"broken = [".to_vec(),
        vec![0xff],
        vec![b' '; (1 << 20) + 1],
        b"schema_version = 999\n".to_vec(),
    ] {
        std::fs::write(&context.preferences, &bytes).unwrap();
        let checked = context.preferences(&policy);
        let legacy = SettingsStore::new(context.preferences.clone()).load();
        match (checked, legacy) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Err(crate::Error::Io(a)), Err(basecraft_platform::settings::Error::Io(b))) => {
                assert_eq!(a.kind(), b.kind());
            }
            (
                Err(crate::Error::Invalid(a)),
                Err(basecraft_platform::settings::Error::Invalid(b)),
            ) => assert_eq!(a, b),
            _ => panic!("parser categories differ"),
        }
        assert_eq!(std::fs::read(&context.preferences).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(context.preferences.parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }
}

#[test]
fn namespace_mapping_preserves_raw_io() {
    for (error, denial) in [
        (NamespaceReadError::Unsupported, Denial::ServiceUnavailable),
        (NamespaceReadError::Changed, Denial::ServiceUnavailable),
        (NamespaceReadError::Redirected, Denial::SymbolicLink),
        (NamespaceReadError::Aliased, Denial::Hardlink),
        (NamespaceReadError::WrongKind, Denial::ResourceType),
    ] {
        assert!(matches!(namespace_error(error), AccessError::Denied(actual) if actual == denial));
    }
    let AccessError::Io(error) = namespace_error(NamespaceReadError::Io(
        std::io::Error::from_raw_os_error(13),
    )) else {
        panic!()
    };
    assert_eq!(error.raw_os_error(), Some(13));
}

#[cfg(target_os = "linux")]
mod locks {
    use super::*;
    use basecraft_platform::files::inspect_target_namespace;
    use std::{
        cell::{Cell, RefCell},
        os::unix::fs::PermissionsExt,
    };

    fn fixture(root: &Path) -> Arc<ServiceContext> {
        let mut context = super::context(root);
        context.reservations = root.join("reservations");
        Arc::new(context)
    }
    struct Policy<'a> {
        context: &'a ServiceContext,
        revoke: Cell<Option<ServiceEffect>>,
        revoke_held: Cell<bool>,
        trace: RefCell<Vec<(ServiceAction, &'static str)>>,
        current: Cell<*const ()>,
    }
    impl<'a> Policy<'a> {
        fn new(context: &'a ServiceContext) -> Self {
            Self {
                context,
                revoke: Cell::new(None),
                revoke_held: Cell::new(false),
                trace: RefCell::new(Vec::new()),
                current: Cell::new(std::ptr::null()),
            }
        }
    }
    impl Access for Policy<'_> {
        fn boundary(&self) -> Option<&Path> {
            None
        }
        fn resource(&self, _: &Path, _: ResourceAction) -> crate::access::Result<PathBuf> {
            panic!("never use legacy resource before strict inspection")
        }
        fn service_context(
            &self,
            request: &ServiceRequest<'_>,
            phase: ServicePhase<'_>,
        ) -> crate::access::Result<()> {
            if !request.permits_lock(self.context, &phase) {
                return Err(unavailable());
            }
            let ptr = std::ptr::from_ref(request).cast::<()>();
            let label = match phase {
                ServicePhase::Eligibility => {
                    self.current.set(ptr);
                    "eligibility"
                }
                ServicePhase::LockValidated(_) => "validated",
                ServicePhase::BeforeEffect(_, effect) => {
                    assert_eq!(self.current.get(), ptr);
                    if self.revoke.get() == Some(effect) {
                        return Err(AccessError::Denied(Denial::ReadOnly));
                    }
                    match effect {
                        ServiceEffect::CreateDirectory => "mkdir",
                        ServiceEffect::SetPermissions => "chmod",
                        ServiceEffect::OpenFile => "open",
                        ServiceEffect::AcquireLock => "acquire",
                    }
                }
                ServicePhase::Held(_) => {
                    if self.revoke_held.get() {
                        return Err(AccessError::Denied(Denial::ReadOnly));
                    }
                    "held"
                }
                ServicePhase::Validated(_) => panic!("read phase used for lock"),
            };
            self.trace.borrow_mut().push((request.action, label));
            Ok(())
        }
        fn service_target<'a>(
            &self,
            request: &'a ServiceRequest<'a>,
        ) -> crate::access::Result<CheckedServiceTarget<'a>> {
            assert_eq!(self.current.get(), std::ptr::from_ref(request).cast::<()>());
            let (path, _) = request.target().unwrap();
            // Test policy supplies the real lower observation. Shared production
            // action/root policy is separately exercised in plugin-host tests.
            request.checked_target(
                path.to_owned(),
                inspect_target_namespace(path).map_err(namespace_error)?,
            )
        }
    }
    #[test]
    fn lock_defaults_refuse_and_read_dispatch_never_grants_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let context = fixture(dir.path());
        let request = ServiceRequest {
            context: &context,
            action: ServiceAction::LockAnalysisEvidence,
            target: None,
        };
        assert!(
            TrustedLegacy
                .service_context(&request, ServicePhase::Eligibility)
                .is_err()
        );
        assert!(
            Bound(&context)
                .service_read(&request, ServicePhase::Eligibility)
                .is_err()
        );
        assert!(context.lock_evidence(&TrustedLegacy).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn lock_ticket_rejects_equal_foreign_request_and_target_purpose() {
        let dir = tempfile::tempdir().unwrap();
        let context = fixture(dir.path());
        let path = dir.path().join("target");
        let first = ServiceRequest {
            context: &context,
            action: ServiceAction::ReserveSaveTarget,
            target: Some((&path, SaveTargetPurpose::NewFile)),
        };
        let second = ServiceRequest {
            context: &context,
            action: first.action,
            target: first.target,
        };
        let target = first
            .checked_target(path.clone(), inspect_target_namespace(&path).unwrap())
            .unwrap();
        let ns = inspect_lock_namespace(
            &context.reservations.join("lock"),
            LockDirectoryMode::Private,
        )
        .unwrap();
        let ticket = CheckedLockNamespace {
            request: &first,
            target: Some(&target.observation),
            lock: LockObservation::Pending(&ns),
        };
        assert!(first.permits_lock(&context, &ServicePhase::LockValidated(&ticket)));
        assert!(!second.permits_lock(&context, &ServicePhase::LockValidated(&ticket)));
        let other = fixture(dir.path());
        assert!(!first.permits_lock(&other, &ServicePhase::LockValidated(&ticket)));
        let wrong = ServiceRequest {
            context: &context,
            action: first.action,
            target: Some((&path, SaveTargetPurpose::FinalDirectory)),
        };
        assert!(!wrong.permits_lock(&context, &ServicePhase::LockValidated(&ticket)));
        assert!(!first.permits(&context, &ServicePhase::LockValidated(&ticket)));
        assert!(!first.permits_lock(
            &context,
            &ServicePhase::BeforeEffect(&ticket, ServiceEffect::AcquireLock)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn revocation_before_each_effect_preserves_only_prior_artifacts() {
        use fs4::fs_std::FileExt;
        for effect in [
            ServiceEffect::CreateDirectory,
            ServiceEffect::SetPermissions,
            ServiceEffect::OpenFile,
            ServiceEffect::AcquireLock,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let context = fixture(dir.path());
            let target = dir.path().join("target");
            if effect != ServiceEffect::CreateDirectory {
                std::fs::create_dir(&context.reservations).unwrap();
                std::fs::set_permissions(
                    &context.reservations,
                    std::fs::Permissions::from_mode(if effect == ServiceEffect::SetPermissions {
                        0o750
                    } else {
                        0o700
                    }),
                )
                .unwrap();
            }
            let policy = Policy::new(&context);
            policy.revoke.set(Some(effect));
            assert!(matches!(
                context.reserve_target(&policy, &target, SaveTargetPurpose::NewFile),
                Err(crate::Error::Access(AccessError::Denied(Denial::ReadOnly)))
            ));
            let leaf = crate::save_guard::lock_path(&context.reservations, &target);
            assert!(!target.exists());
            assert_eq!(leaf.exists(), effect == ServiceEffect::AcquireLock);
            if effect == ServiceEffect::CreateDirectory {
                assert!(!context.reservations.exists());
            }
            if effect == ServiceEffect::SetPermissions {
                assert_eq!(
                    std::fs::metadata(&context.reservations)
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o750
                );
            }
            if effect == ServiceEffect::AcquireLock {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&leaf)
                    .unwrap();
                assert!(file.try_lock_exclusive().unwrap());
            }
        }
    }
    #[test]
    fn retained_guards_reauthorize_without_reacquisition_in_target_stage_evidence_order() {
        let dir = tempfile::tempdir().unwrap();
        let context = fixture(dir.path());
        let policy = Policy::new(&context);
        let final_target = context
            .reserve_target(
                &policy,
                &dir.path().join("final"),
                SaveTargetPurpose::FinalDirectory,
            )
            .unwrap();
        let stage_target = context
            .reserve_target(
                &policy,
                &dir.path().join("stage"),
                SaveTargetPurpose::StageFile,
            )
            .unwrap();
        let evidence = context.lock_evidence(&policy).unwrap();
        let acquired: Vec<_> = policy
            .trace
            .borrow()
            .iter()
            .filter(|(_, e)| *e == "acquire")
            .map(|(a, _)| *a)
            .collect();
        assert_eq!(
            acquired,
            [
                ServiceAction::ReserveSaveTarget,
                ServiceAction::ReserveSaveTarget,
                ServiceAction::LockAnalysisEvidence
            ]
        );
        policy.trace.borrow_mut().clear();
        final_target.reauthorize(&policy).unwrap();
        stage_target.reauthorize(&policy).unwrap();
        evidence.reauthorize(&policy).unwrap();
        assert_eq!(
            policy
                .trace
                .borrow()
                .iter()
                .map(|(_, e)| *e)
                .collect::<Vec<_>>(),
            ["held", "held", "held"]
        );
        policy.revoke_held.set(true);
        assert!(final_target.reauthorize(&policy).is_err());
        assert!(evidence.reauthorize(&policy).is_err());
        drop((evidence, stage_target, final_target));
        assert!(context.evidence.exists());
        assert!(!dir.path().join("final").exists());
    }
}
