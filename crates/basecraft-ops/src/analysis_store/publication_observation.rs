//! Passive facts for one secondary output, not a receipt or a completion owner.
//! The source artifact is not pinned by these facts. Nothing here performs IO
//! on drop, retries publication, or decides cleanup/recovery policy.
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use super::ArtifactRun;
use crate::{Error, Result};
use basecraft_platform::files::{
    ObjectIdentity, OrdinaryKind, handle_object_identity, object_identity, open_regular_read,
};

#[derive(Debug)]
pub(crate) struct StagedWitness {
    identity: ObjectIdentity,
    hash: String,
    size: u64,
}
impl StagedWitness {
    pub(crate) fn identity(&self) -> &ObjectIdentity {
        &self.identity
    }
    pub(crate) fn hash(&self) -> &str {
        &self.hash
    }
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug)]
pub(crate) enum PersistFact {
    NotAttempted {
        staged: Option<StagedWitness>,
    },
    /// No successful-return witness; NOT proof of rollback or absence.
    NoCommitWitness {
        staged: StagedWitness,
    },
    Committed {
        witness: StagedWitness,
    },
}
impl PersistFact {
    pub(crate) fn witness(&self) -> Option<&StagedWitness> {
        match self {
            Self::NotAttempted { staged } => staged.as_ref(),
            Self::NoCommitWitness { staged } => Some(staged),
            Self::Committed { witness } => Some(witness),
        }
    }
}

#[derive(Debug)]
pub(crate) enum TargetObservation {
    NotRequested,
    Absent,
    Present {
        identity: ObjectIdentity,
        hash: String,
        size: u64,
    },
    Unavailable,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParentSync {
    NotAttempted,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProducerPhase {
    Validation,
    CopyAndSync,
    StagedIdentity,
    BeforePersist,
    Persist,
    ParentSync,
    Complete,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ObservationPhase {
    PostWriteInspection,
}
#[derive(Debug)]
pub(crate) struct ObservationFailure {
    phase: ObservationPhase,
    cause: Error,
}
impl ObservationFailure {
    pub(crate) fn phase(&self) -> &ObservationPhase {
        &self.phase
    }
    pub(crate) fn cause(&self) -> &Error {
        &self.cause
    }
}
#[derive(Debug)]
pub(crate) struct FilePublicationObservation {
    // Caller spelling only; this is never a validated path or access grant.
    destination: PathBuf,
    persist: PersistFact,
    parent_sync: ParentSync,
    target: TargetObservation,
}
impl FilePublicationObservation {
    pub(crate) fn new(destination: &Path) -> Self {
        Self {
            destination: destination.into(),
            persist: PersistFact::NotAttempted { staged: None },
            parent_sync: ParentSync::NotAttempted,
            target: TargetObservation::NotRequested,
        }
    }
    pub(crate) fn staged(&mut self, file: &File, hash: String, size: u64) -> io::Result<()> {
        let identity = handle_object_identity(file, OrdinaryKind::File)?;
        self.persist = PersistFact::NotAttempted {
            staged: Some(StagedWitness {
                identity,
                hash,
                size,
            }),
        };
        Ok(())
    }
    pub(crate) fn attempting(&mut self) {
        let prior = std::mem::replace(
            &mut self.persist,
            PersistFact::NotAttempted { staged: None },
        );
        self.persist = match prior {
            PersistFact::NotAttempted {
                staged: Some(staged),
            } => PersistFact::NoCommitWitness { staged },
            other => other,
        };
    }
    pub(crate) fn persisted(&mut self) {
        let prior = std::mem::replace(
            &mut self.persist,
            PersistFact::NotAttempted { staged: None },
        );
        self.persist = match prior {
            PersistFact::NoCommitWitness { staged } => PersistFact::Committed { witness: staged },
            other => other,
        };
    }
    pub(crate) fn inspected(
        &mut self,
        result: Result<TargetObservation>,
    ) -> Option<ObservationFailure> {
        match result {
            Ok(target) => {
                self.target = target;
                None
            }
            Err(cause) => {
                self.target = TargetObservation::Unavailable;
                Some(ObservationFailure {
                    phase: ObservationPhase::PostWriteInspection,
                    cause,
                })
            }
        }
    }
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }
    pub(crate) fn persist(&self) -> &PersistFact {
        &self.persist
    }
    pub(crate) fn parent_sync(&self) -> &ParentSync {
        &self.parent_sync
    }
    pub(crate) fn target(&self) -> &TargetObservation {
        &self.target
    }
}
#[derive(Debug)]
pub(crate) struct ObservedSecondaryPublication {
    observation: FilePublicationObservation,
    producer: Result<()>,
    producer_phase: ProducerPhase,
    diagnostics: Vec<ObservationFailure>,
}
impl ObservedSecondaryPublication {
    pub(crate) fn observation(&self) -> &FilePublicationObservation {
        &self.observation
    }
    pub(crate) fn producer(&self) -> &Result<()> {
        &self.producer
    }
    pub(crate) fn producer_phase(&self) -> ProducerPhase {
        self.producer_phase
    }
    pub(crate) fn diagnostics(&self) -> &[ObservationFailure] {
        &self.diagnostics
    }
}

// The legacy specialization has no optional queries, reads or diagnostic work.
// The real copy/sync/no-clobber algorithm lives only in ArtifactRun.
pub(super) trait SecondaryHooks {
    fn usable_target(&mut self, _: &Path) {}
    fn phase(&mut self, _: ProducerPhase) {}
    fn staged(&mut self, _: &File, _: String, _: u64) -> Result<()> {
        Ok(())
    }
    fn before_persist(&mut self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn attempting(&mut self) {}
    fn persist(&mut self, candidate: tempfile::NamedTempFile, destination: &Path) -> Result<()> {
        candidate
            .persist_noclobber(destination)
            .map_err(|e| e.error)?;
        Ok(())
    }
    fn persisted(&mut self) {}
    fn sync_parent(&mut self, parent: &Path) -> Result<()> {
        basecraft_platform::files::sync_directory(parent)?;
        Ok(())
    }
    fn synced(&mut self) {}
}
pub(super) struct Unobserved;
impl SecondaryHooks for Unobserved {}

struct Collector {
    observation: FilePublicationObservation,
    phase: ProducerPhase,
    inspect: bool,
    #[cfg(test)]
    hooks: TestHooks,
}
impl Collector {
    fn new(destination: &Path) -> Self {
        Self {
            observation: FilePublicationObservation::new(destination),
            phase: ProducerPhase::Validation,
            inspect: false,
            #[cfg(test)]
            hooks: TestHooks::default(),
        }
    }
    fn finish(mut self, producer: Result<()>) -> ObservedSecondaryPublication {
        let mut diagnostics = Vec::new();
        if self.inspect {
            let inspected = self.inspect_target();
            if let Some(failure) = self.observation.inspected(inspected) {
                diagnostics.push(failure);
            }
        }
        ObservedSecondaryPublication {
            observation: self.observation,
            producer,
            producer_phase: self.phase,
            diagnostics,
        }
    }
    fn inspect_target(&mut self) -> Result<TargetObservation> {
        #[cfg(test)]
        if let Some(hook) = self.hooks.inspect.take() {
            hook(&self.observation.destination)?;
        }
        inspect_target(&self.observation.destination)
    }
}
impl SecondaryHooks for Collector {
    fn usable_target(&mut self, path: &Path) {
        // Reached only after source validation and creation of a temporary in
        // the supplied parent. Do not inspect invalid leaf spellings on error.
        self.inspect = path.file_name().is_some()
            && matches!(
                path.components().next_back(),
                Some(std::path::Component::Normal(_))
            );
    }
    fn phase(&mut self, phase: ProducerPhase) {
        self.phase = phase;
    }
    fn staged(&mut self, file: &File, hash: String, size: u64) -> Result<()> {
        #[cfg(test)]
        if let Some(hook) = self.hooks.staged.take() {
            hook(file)?;
        }
        self.observation.staged(file, hash, size)?;
        Ok(())
    }

    #[cfg(test)]
    fn before_persist(&mut self, destination: &Path) -> Result<()> {
        if let Some(hook) = self.hooks.before_persist.take() {
            hook(destination, self.observation.persist.witness())?;
        }
        Ok(())
    }
    fn attempting(&mut self) {
        self.observation.attempting();
    }
    #[cfg(test)]
    fn persist(&mut self, candidate: tempfile::NamedTempFile, destination: &Path) -> Result<()> {
        self.hooks.persist_calls += 1;
        candidate
            .persist_noclobber(destination)
            .map_err(|e| e.error)?;
        // Real publication, but no successful-return latch for an ambiguous Err.
        if let Some(error) = self.hooks.persist_return_error.take() {
            return Err(error);
        }
        Ok(())
    }
    fn persisted(&mut self) {
        self.observation.persisted();
        #[cfg(test)]
        {
            self.hooks.latch_calls += 1;
        }
    }
    fn sync_parent(&mut self, parent: &Path) -> Result<()> {
        self.observation.parent_sync = ParentSync::Failed;
        #[cfg(test)]
        if let Some(hook) = self.hooks.sync.take() {
            hook(parent)?;
        }
        basecraft_platform::files::sync_directory(parent)?;
        Ok(())
    }
    fn synced(&mut self) {
        self.observation.parent_sync = ParentSync::Complete;
    }
}

pub(super) fn observe(
    run: &ArtifactRun,
    name: &str,
    destination: &Path,
) -> ObservedSecondaryPublication {
    let mut collector = Collector::new(destination);
    let producer = run.publish_secondary_with(name, destination, &mut collector);
    collector.finish(producer)
}

/// Strict path identities bracket same-handle hashing and full identity. This
/// rejects stable leaf links, but is not an atomic snapshot or hostile-mutator
/// confinement: ancestors and content may change, and paths can rebind later.
pub(crate) fn inspect_target(path: &Path) -> Result<TargetObservation> {
    let before = match object_identity(path, OrdinaryKind::File) {
        Ok(identity) => identity,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TargetObservation::Absent);
        }
        Err(error) => return Err(error.into()),
    };
    let mut file = open_regular_read(path)?;
    let identity = handle_object_identity(&file, OrdinaryKind::File)?;
    let mut hashing = crate::files::HashingWriter::new(io::sink());
    let size = io::copy(&mut file, &mut hashing)?;
    let after = object_identity(path, OrdinaryKind::File)?;
    if before != identity || identity != after {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "secondary output identity changed during observation",
        )
        .into());
    }
    Ok(TargetObservation::Present {
        identity,
        hash: hashing.hash(),
        size,
    })
}

#[cfg(test)]
type PathHook = Box<dyn FnOnce(&Path) -> Result<()>>;
#[cfg(test)]
type StagedHook = Box<dyn FnOnce(&File) -> Result<()>>;
#[cfg(test)]
type BeforePersistHook = Box<dyn FnOnce(&Path, Option<&StagedWitness>) -> Result<()>>;
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    staged: Option<StagedHook>,
    before_persist: Option<BeforePersistHook>,
    persist_return_error: Option<Error>,
    sync: Option<PathHook>,
    inspect: Option<PathHook>,
    persist_calls: usize,
    latch_calls: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs};
    const BYTES: &[u8] = b"retained secondary bytes\n";
    #[cfg(unix)]
    const DENIED: i32 = 13;
    #[cfg(windows)]
    const DENIED: i32 = 5;
    #[cfg(unix)]
    const MISSING: i32 = 2;
    #[cfg(windows)]
    const MISSING: i32 = 2;

    fn failure(code: i32) -> Error {
        io::Error::from_raw_os_error(code).into()
    }
    fn raw(error: &Error, code: i32) {
        let Error::Io(error) = error else {
            panic!("expected original IO: {error:?}")
        };
        assert_eq!(error.raw_os_error(), Some(code));
        assert_eq!(error.kind(), io::Error::from_raw_os_error(code).kind());
    }
    fn inventory(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(root: &Path, at: &Path, found: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(at).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    found.insert(path.strip_prefix(root).unwrap().into(), None);
                    walk(root, &path, found);
                } else {
                    found.insert(
                        path.strip_prefix(root).unwrap().into(),
                        Some(fs::read(&path).unwrap()),
                    );
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(root, root, &mut found);
        found
    }
    struct Fixture {
        root: tempfile::TempDir,
        run: ArtifactRun,
        target: PathBuf,
        baseline: BTreeMap<PathBuf, Option<Vec<u8>>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("work")).unwrap();
            fs::create_dir(root.path().join("export")).unwrap();
            let run = ArtifactRun::create(
                &root.path().join("work/result"),
                "publication-observation-test",
            )
            .unwrap();
            basecraft_io::write_atomic(&run.artifact("secondary.dat").unwrap(), |w| {
                w.write_all(BYTES)
            })
            .unwrap();
            fs::write(root.path().join("export/unrelated"), b"untouched").unwrap();
            let target = root.path().join("export/result.dat");
            let baseline = inventory(root.path());
            Self {
                root,
                run,
                target,
                baseline,
            }
        }
        fn execute(&self, hooks: TestHooks) -> (ObservedSecondaryPublication, usize, usize) {
            let mut collector = Collector::new(&self.target);
            collector.hooks = hooks;
            let producer =
                self.run
                    .publish_secondary_with("secondary.dat", &self.target, &mut collector);
            let counts = (collector.hooks.persist_calls, collector.hooks.latch_calls);
            (collector.finish(producer), counts.0, counts.1)
        }
        fn residue(&self, additions: &[&str]) {
            let mut expected = self.baseline.clone();
            for name in additions {
                expected.insert(PathBuf::from(name), Some(BYTES.to_vec()));
            }
            assert_eq!(inventory(self.root.path()), expected);
        }
        fn drop_unchanged(&self, result: ObservedSecondaryPublication) {
            let before = inventory(self.root.path());
            drop(result);
            assert_eq!(inventory(self.root.path()), before);
        }
    }
    fn committed(result: &ObservedSecondaryPublication) -> &StagedWitness {
        let PersistFact::Committed { witness } = result.observation().persist() else {
            panic!("{result:?}")
        };
        assert_eq!(witness.hash(), basecraft_io::hash_bytes(BYTES));
        assert_eq!(witness.size(), BYTES.len() as u64);
        witness
    }
    fn present(result: &ObservedSecondaryPublication) -> &ObjectIdentity {
        let TargetObservation::Present {
            identity,
            hash,
            size,
        } = result.observation().target()
        else {
            panic!("{result:?}")
        };
        assert_eq!(hash, &basecraft_io::hash_bytes(BYTES));
        assert_eq!(*size, BYTES.len() as u64);
        identity
    }

    #[test]
    fn ordinary_real_publication() {
        let f = Fixture::new();
        let result = f.run.publish_secondary_observed("secondary.dat", &f.target);
        assert!(result.producer().is_ok());
        assert_eq!(result.producer_phase(), ProducerPhase::Complete);
        assert_eq!(result.observation().destination(), f.target);
        assert_eq!(committed(&result).identity(), present(&result));
        assert_eq!(
            present(&result),
            &object_identity(&f.target, OrdinaryKind::File).unwrap()
        );
        assert_eq!(result.observation().parent_sync(), &ParentSync::Complete);
        assert!(result.diagnostics().is_empty());
        f.residue(&["export/result.dat"]);
        f.drop_unchanged(result);
    }

    #[test]
    fn equal_bytes_competitor_is_not_our_commit() {
        let f = Fixture::new();
        // The handle remains alive until after the call and all assertions.
        let held = std::rc::Rc::new(std::cell::RefCell::new(None));
        let hold = held.clone();
        let (result, persists, latches) = f.execute(TestHooks {
            before_persist: Some(Box::new(move |path, witness| {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                std::io::Write::write_all(&mut file, BYTES)?;
                assert_ne!(
                    &handle_object_identity(&file, OrdinaryKind::File)?,
                    witness.unwrap().identity()
                );
                *hold.borrow_mut() = Some(file);
                Ok(())
            })),
            ..TestHooks::default()
        });
        assert!(
            matches!(result.producer(), Err(Error::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists && e.raw_os_error().is_some())
        );
        assert_eq!((persists, latches), (1, 0));
        assert_eq!(result.producer_phase(), ProducerPhase::Persist);
        assert!(matches!(
            result.observation().persist(),
            PersistFact::NoCommitWitness { .. }
        ));
        assert_ne!(
            result.observation().persist().witness().unwrap().identity(),
            present(&result)
        );
        assert_eq!(
            present(&result),
            &handle_object_identity(held.borrow().as_ref().unwrap(), OrdinaryKind::File).unwrap()
        );
        assert_eq!(
            result.observation().parent_sync(),
            &ParentSync::NotAttempted
        );
        f.residue(&["export/result.dat"]);
        f.drop_unchanged(result);
    }

    #[test]
    fn prestage_and_prepersist_failures_preserve_original_causes() {
        for stage in [true, false] {
            let f = Fixture::new();
            let mut hooks = TestHooks::default();
            if stage {
                hooks.staged = Some(Box::new(|_| Err(failure(DENIED))));
            } else {
                hooks.before_persist = Some(Box::new(|_, _| Err(failure(DENIED))));
            }
            let (result, persists, latches) = f.execute(hooks);
            raw(result.producer().as_ref().unwrap_err(), DENIED);
            assert_eq!((persists, latches), (0, 0));
            assert_eq!(
                result.producer_phase(),
                if stage {
                    ProducerPhase::StagedIdentity
                } else {
                    ProducerPhase::BeforePersist
                }
            );
            assert!(matches!(
                result.observation().persist(),
                PersistFact::NotAttempted { .. }
            ));
            assert_eq!(result.observation().persist().witness().is_some(), !stage);
            assert!(matches!(
                result.observation().target(),
                TargetObservation::Absent
            ));
            assert_eq!(
                result.observation().parent_sync(),
                &ParentSync::NotAttempted
            );
            f.residue(&[]);
            f.drop_unchanged(result);
        }
    }

    #[test]
    fn commit_then_sync_error_keeps_witness() {
        let f = Fixture::new();
        let (result, persists, latches) = f.execute(TestHooks {
            sync: Some(Box::new(|_| Err(failure(DENIED)))),
            ..TestHooks::default()
        });
        raw(result.producer().as_ref().unwrap_err(), DENIED);
        assert_eq!((persists, latches), (1, 1));
        assert_eq!(result.producer_phase(), ProducerPhase::ParentSync);
        assert_eq!(result.observation().parent_sync(), &ParentSync::Failed);
        assert_eq!(committed(&result).identity(), present(&result));
        assert!(result.diagnostics().is_empty());
        f.residue(&["export/result.dat"]);
        f.drop_unchanged(result);
    }

    #[test]
    fn inspection_failure_is_supplemental_even_after_primary_sync_failure() {
        for fail_sync in [false, true] {
            let f = Fixture::new();
            let mut hooks = TestHooks {
                inspect: Some(Box::new(|_| Err(failure(MISSING)))),
                ..TestHooks::default()
            };
            if fail_sync {
                hooks.sync = Some(Box::new(|_| Err(failure(DENIED))));
            }
            let (result, persists, latches) = f.execute(hooks);
            assert_eq!((persists, latches), (1, 1));
            if fail_sync {
                raw(result.producer().as_ref().unwrap_err(), DENIED);
            } else {
                assert!(result.producer().is_ok());
            }
            assert_eq!(
                result.observation().parent_sync(),
                if fail_sync {
                    &ParentSync::Failed
                } else {
                    &ParentSync::Complete
                }
            );
            assert_eq!(
                committed(&result).identity(),
                &object_identity(&f.target, OrdinaryKind::File).unwrap()
            );
            assert!(matches!(
                result.observation().target(),
                TargetObservation::Unavailable
            ));
            assert_eq!(result.diagnostics().len(), 1);
            assert_eq!(
                result.diagnostics()[0].phase(),
                &ObservationPhase::PostWriteInspection
            );
            raw(result.diagnostics()[0].cause(), MISSING);
            f.residue(&["export/result.dat"]);
            f.drop_unchanged(result);
        }
    }

    #[test]
    fn same_bytes_replacement_does_not_replace_staged_witness() {
        let f = Fixture::new();
        let (result, persists, latches) = f.execute(TestHooks {
            inspect: Some(Box::new(|path| {
                // Retain both an open handle and a hardlink while replacing the name.
                let file = File::open(path)?;
                fs::hard_link(path, path.with_file_name("held-original"))?;
                basecraft_io::write_atomic(path, |w| w.write_all(BYTES))?;
                assert_ne!(
                    handle_object_identity(&file, OrdinaryKind::File)?,
                    object_identity(path, OrdinaryKind::File)?
                );
                Ok(())
            })),
            ..TestHooks::default()
        });
        assert!(result.producer().is_ok());
        assert_eq!((persists, latches), (1, 1));
        assert_ne!(committed(&result).identity(), present(&result));
        assert_eq!(
            committed(&result).identity(),
            &object_identity(
                &f.target.with_file_name("held-original"),
                OrdinaryKind::File
            )
            .unwrap()
        );
        f.residue(&["export/result.dat", "export/held-original"]);
        f.drop_unchanged(result);
    }

    #[test]
    fn real_persist_with_ambiguous_error_has_no_successful_return_witness() {
        let f = Fixture::new();
        let (result, persists, latches) = f.execute(TestHooks {
            persist_return_error: Some(failure(DENIED)),
            ..TestHooks::default()
        });
        raw(result.producer().as_ref().unwrap_err(), DENIED);
        assert_eq!((persists, latches), (1, 0));
        assert_eq!(result.producer_phase(), ProducerPhase::Persist);
        assert!(matches!(
            result.observation().persist(),
            PersistFact::NoCommitWitness { .. }
        ));
        assert_eq!(
            result.observation().persist().witness().unwrap().identity(),
            present(&result)
        );
        assert_eq!(
            result.observation().parent_sync(),
            &ParentSync::NotAttempted
        );
        f.residue(&["export/result.dat"]);
        f.drop_unchanged(result);
    }

    #[test]
    fn full_width_carrier_and_nested_causes_are_preserved() {
        let first = ObjectIdentity::Windows {
            volume_serial: u64::MAX,
            file_id: [0; 16],
        };
        let mut upper = [0; 16];
        upper[15] = 128;
        let second = ObjectIdentity::Windows {
            volume_serial: u64::MAX,
            file_id: upper,
        };
        for identity in [
            first.clone(),
            second.clone(),
            ObjectIdentity::Unix {
                device: u64::MAX,
                inode: u64::MAX - 1,
            },
        ] {
            let mut collector = Collector::new(Path::new("diagnostic-only"));
            collector.observation.persist = PersistFact::NotAttempted {
                staged: Some(StagedWitness {
                    identity: identity.clone(),
                    hash: "b3:carrier".into(),
                    size: u64::MAX,
                }),
            };
            collector.attempting();
            collector.persisted();
            let result = collector.finish(Err(crate::access::AccessError::Denied(
                crate::access::Denial::ReadOnly,
            )
            .into()));
            assert_eq!(
                result.observation().persist().witness().unwrap().identity(),
                &identity
            );
            assert_eq!(
                result.observation().persist().witness().unwrap().size(),
                u64::MAX
            );
            assert!(matches!(
                result.producer(),
                Err(Error::Access(crate::access::AccessError::Denied(
                    crate::access::Denial::ReadOnly
                )))
            ));
            let nested = ObservationFailure {
                phase: ObservationPhase::PostWriteInspection,
                cause: Error::Format(basecraft_io::Error::Io(io::Error::from_raw_os_error(
                    DENIED,
                ))),
            };
            assert!(
                matches!(nested.cause(), Error::Format(basecraft_io::Error::Io(e)) if e.raw_os_error() == Some(DENIED))
            );
        }
        assert_ne!(first, second);
    }

    #[test]
    fn unsupported_identity_and_invalid_inputs_do_not_probe_targets() {
        let f = Fixture::new();
        let (result, persists, latches) = f.execute(TestHooks {
            staged: Some(Box::new(|_| {
                Err(Error::Io(io::ErrorKind::Unsupported.into()))
            })),
            ..TestHooks::default()
        });
        assert!(
            matches!(result.producer(), Err(Error::Io(e)) if e.kind() == io::ErrorKind::Unsupported)
        );
        assert_eq!((persists, latches), (0, 0));
        assert!(result.observation().persist().witness().is_none());
        for (name, target) in [
            ("../invalid", f.target.clone()),
            ("secondary.dat", f.root.path().join("missing/result")),
            ("secondary.dat", PathBuf::new()),
        ] {
            let mut collector = Collector::new(&target);
            collector.hooks.inspect = Some(Box::new(|_| panic!("invalid path inspected")));
            let producer = f.run.publish_secondary_with(name, &target, &mut collector);
            let result = collector.finish(producer);
            assert!(result.producer().is_err());
            assert!(matches!(
                result.observation().target(),
                TargetObservation::NotRequested
            ));
            assert!(result.diagnostics().is_empty());
        }
        fs::create_dir(f.run.directory().join("directory-source")).unwrap();
        let result = f
            .run
            .publish_secondary_observed("directory-source", &f.target);
        assert!(matches!(result.producer(), Err(Error::Invalid(_))));
        assert!(matches!(
            result.observation().target(),
            TargetObservation::NotRequested
        ));
    }

    #[cfg(unix)]
    #[test]
    fn strict_observation_rejects_stable_leaf_symlink() {
        let f = Fixture::new();
        let (result, _, _) = f.execute(TestHooks {
            inspect: Some(Box::new(|path| {
                fs::rename(path, path.with_file_name("held-original"))?;
                std::os::unix::fs::symlink("held-original", path)?;
                Ok(())
            })),
            ..TestHooks::default()
        });
        assert!(result.producer().is_ok());
        committed(&result);
        assert!(matches!(
            result.observation().target(),
            TargetObservation::Unavailable
        ));
        assert!(
            matches!(result.diagnostics()[0].cause(), Error::Io(e) if e.kind() == io::ErrorKind::InvalidData)
        );
    }

    #[test]
    fn legacy_no_clobber_validation_and_budget_parity() {
        let f = Fixture::new();
        #[cfg(feature = "test-counters")]
        basecraft_io::counters::reset();
        f.run.publish_secondary("secondary.dat", &f.target).unwrap();
        #[cfg(feature = "test-counters")]
        {
            let counts = basecraft_io::counters::snapshot();
            assert_eq!(counts.reads.len(), 1);
            assert_eq!(
                counts.reads[&f
                    .run
                    .artifact("secondary.dat")
                    .unwrap()
                    .canonicalize()
                    .unwrap()],
                basecraft_io::counters::Reads {
                    full_reads: 1,
                    bytes: BYTES.len() as u64
                }
            );
            assert_eq!(
                (counts.sync_all, counts.sync_data, counts.unresolved_reads),
                (0, 0, 0)
            );
        }
        let identity = object_identity(&f.target, OrdinaryKind::File).unwrap();
        assert!(
            matches!(f.run.publish_secondary("secondary.dat", &f.target), Err(Error::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists)
        );
        assert!(matches!(
            f.run.publish_secondary("../invalid", &f.target),
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            identity,
            object_identity(&f.target, OrdinaryKind::File).unwrap()
        );
        f.residue(&["export/result.dat"]);
    }
}
