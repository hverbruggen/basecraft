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
    };
    let second = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadPreferences,
    };
    let registry = ServiceReadRequest {
        context: &context,
        action: ServiceAction::ReadAnalysisRegistry,
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
