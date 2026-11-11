//! Passive, invocation-scoped save boundary hooks. Legacy hooks perform no IO.
//! Successful calls are not durability witnesses; reconciliation is not authority.
use super::Session;
#[cfg(test)]
use super::{DocId, SaveDecision, SaveOutcome};
use crate::{Error, Result, documents::Document, files};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Site {
    Consent,
    Admission,
    CleanHashes,
    TargetGuard,
    Evidence,
    OuterTrack,
    OuterFlush,
    ExpectedHashes,
    PendingEncode,
    PendingParent,
    PendingWrite,
    Recheck,
    InnerFlush,
    InnerTrack,
    SourceCheck,
    Format,
    #[cfg(test)]
    Document,
    SaveRecord,
    Provenance,
    SavedHashes,
    DiskStamp,
    PendingRemove,
    Observation,
    OtherKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Milestone {
    AcceptedLog,
    SavedHashes,
    DiskStamp,
    PendingRemoved,
    Reconciled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    #[cfg(test)]
    Data,
    Clean,
    History,
}

pub(crate) trait Hooks {
    fn at(&mut self, _: Site) {}
    fn returned(&mut self, _: Site) {}
    fn executed(&mut self, _: Milestone) {}
    fn disposition(&mut self, _: Disposition) {}
    fn boundary(&mut self, site: Site, _session: &mut Session) -> Result<()> {
        self.at(site);
        Ok(())
    }
    fn record(&mut self, _: bool) {}
    fn project(&mut self, error: Error, legacy: impl FnOnce(&Error) -> Error) -> Error {
        legacy(&error)
    }
    fn write<D: Document + ?Sized>(
        &mut self,
        doc: &D,
        target: &Path,
        format: files::Format,
        root: &Path,
        publication: &crate::documents::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        doc.write(target, format, root, publication)
    }
}
pub(crate) struct Legacy;
impl Hooks for Legacy {}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Call {
    #[default]
    NotEntered,
    Entered,
    ReturnedOk,
}
#[cfg(test)]
#[derive(Debug)]
// Independent executed milestones, not an alternate save state machine.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Facts {
    pub failure: Option<Site>,
    pub disposition: Disposition,
    pub document: Call,
    pub metadata: Option<crate::documents::MetadataFacts>,
    pub record: Call,
    pub journal_present: Option<bool>,
    pub provenance: Call,
    pub observation: Call,
    pub accepted_log: bool,
    pub saved_hashes: bool,
    pub disk_stamp: bool,
    pub pending_removed: bool,
    pub reconciled: bool,
}
#[cfg(test)]
impl Default for Facts {
    fn default() -> Self {
        Self {
            failure: None,
            disposition: Disposition::Data,
            document: Call::NotEntered,
            metadata: None,
            record: Call::NotEntered,
            journal_present: None,
            provenance: Call::NotEntered,
            observation: Call::NotEntered,
            accepted_log: false,
            saved_hashes: false,
            disk_stamp: false,
            pending_removed: false,
            reconciled: false,
        }
    }
}
#[cfg(test)]
pub(crate) type BoundaryHook = Box<dyn FnOnce(&mut Session) -> Result<()>>;
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Observer {
    pub facts: Facts,
    pub document_hooks: crate::documents::DocumentHooks,
    pub fault: Option<(Site, BoundaryHook)>,
    active: Option<Site>,
}
#[cfg(test)]
impl Hooks for Observer {
    fn at(&mut self, site: Site) {
        self.active = Some(site);
        match site {
            Site::SaveRecord => self.facts.record = Call::Entered,
            Site::Provenance => self.facts.provenance = Call::Entered,
            Site::Observation => self.facts.observation = Call::Entered,
            _ => {}
        }
    }
    fn returned(&mut self, site: Site) {
        match site {
            Site::SaveRecord => self.facts.record = Call::ReturnedOk,
            Site::Provenance => self.facts.provenance = Call::ReturnedOk,
            Site::Observation => self.facts.observation = Call::ReturnedOk,
            _ => {}
        }
    }
    fn executed(&mut self, milestone: Milestone) {
        match milestone {
            Milestone::AcceptedLog => self.facts.accepted_log = true,
            Milestone::SavedHashes => self.facts.saved_hashes = true,
            Milestone::DiskStamp => self.facts.disk_stamp = true,
            Milestone::PendingRemoved => self.facts.pending_removed = true,
            Milestone::Reconciled => self.facts.reconciled = true,
        }
    }
    fn disposition(&mut self, disposition: Disposition) {
        self.facts.disposition = disposition;
    }
    fn boundary(&mut self, site: Site, session: &mut Session) -> Result<()> {
        self.at(site);
        if self.fault.as_ref().is_some_and(|(at, _)| *at == site)
            && let Some((_, hook)) = self.fault.take()
        {
            hook(session)?;
        }
        Ok(())
    }
    fn record(&mut self, journal: bool) {
        self.facts.journal_present = Some(journal);
    }
    fn project(&mut self, error: Error, _: impl FnOnce(&Error) -> Error) -> Error {
        error
    }
    fn write<D: Document + ?Sized>(
        &mut self,
        doc: &D,
        target: &Path,
        format: files::Format,
        root: &Path,
        publication: &crate::documents::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        self.at(Site::Document);
        self.facts.document = Call::Entered;
        let attempt = doc.write_observed(
            target,
            format,
            root,
            publication,
            std::mem::take(&mut self.document_hooks),
        );
        self.facts.metadata = attempt.metadata;
        if attempt.result.is_ok() {
            self.facts.document = Call::ReturnedOk;
        }
        attempt.result
    }
}
#[cfg(test)]
pub(crate) struct ObservedSave {
    pub result: Result<SaveOutcome>,
    pub facts: Facts,
}
#[cfg(test)]
impl Session {
    pub(crate) fn save_recovery_observed(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: Option<SaveDecision>,
        observer: Observer,
    ) -> ObservedSave {
        let target = self.resolve(&self.docs[id.0].key);
        let policy = decision
            .as_ref()
            .map_or(crate::input_evidence::SavePolicy::Cancel, |d| d.known);
        self.save_observed(
            id,
            path.or(Some(&target)),
            policy,
            true,
            false,
            decision,
            observer,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn save_observed(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        policy: crate::input_evidence::SavePolicy,
        overwrite: bool,
        history: bool,
        decision: Option<SaveDecision>,
        mut observer: Observer,
    ) -> ObservedSave {
        let result = self.save_attempt_with(
            id,
            path,
            policy,
            overwrite,
            history,
            decision,
            &mut observer,
        );
        if result.is_err() {
            observer.facts.failure = observer.active;
        }
        ObservedSave {
            result,
            facts: observer.facts,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::analysis_store::publication_observation::PersistFact;
    use basecraft_protocol::Op;
    use std::{collections::BTreeMap, fs};

    pub(crate) fn inventory(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                let key = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    out.insert(key, None);
                    walk(root, &path, out);
                } else {
                    out.insert(key, Some(fs::read(path).unwrap()));
                }
            }
        }
        let mut result = BTreeMap::new();
        walk(root, root, &mut result);
        result
    }
    pub(crate) fn fixture(root: &Path) -> (Session, DocId, PathBuf) {
        let source = root.join("source");
        let target = crate::metadata::create(&source).unwrap();
        let mut session = Session::recoverable(&source).unwrap();
        let id = session.open(&target).unwrap();
        let mut settings = session.metadata(id).unwrap().import.clone();
        settings
            .header_patterns
            .push(basecraft_protocol::metadata::HeaderPattern {
                name: "boundary".into(),
                separator: "|".into(),
                fields: vec![basecraft_protocol::metadata::FieldTarget::Specimen],
            });
        session
            .apply(
                id,
                Op::MetadataEdit {
                    edit: Box::new(
                        basecraft_protocol::metadata::MetadataEdit::SetImportSettings { settings },
                    ),
                },
            )
            .unwrap();
        session.flush().unwrap();
        (session, id, target)
    }
    pub(crate) fn injected() -> Error {
        std::io::Error::other("injected boundary failure").into()
    }
    pub(crate) fn observer(case: usize) -> Observer {
        let mut observer = Observer::default();
        match case {
            0 => {
                observer.document_hooks.before = Some((
                    3,
                    Box::new(|fact| {
                        fs::remove_file(fact.destination())?;
                        fs::create_dir(fact.destination())
                    }),
                ));
            }
            1 => observer.document_hooks.returning = Some(Box::new(|| Err(injected()))),
            2 => {
                observer.fault = Some((
                    Site::SaveRecord,
                    Box::new(|session| {
                        let log = session.log.as_mut().expect("actual recovery journal");
                        log.faults.after_bytes = Some(7);
                        log.faults.rollback_sync = true;
                        Ok(())
                    }),
                ));
            }
            3 => {
                observer.fault = Some((
                    Site::Provenance,
                    Box::new(|session| {
                        let target = session.resolve(&session.docs[0].key);
                        let bundle = crate::provenance::companion(&target);
                        fs::write(bundle, b"blocked provenance bundle")?;
                        Ok(())
                    }),
                ));
            }
            4 => observer.fault = Some((Site::SavedHashes, Box::new(|_| Err(injected())))),
            5 => observer.fault = Some((Site::DiskStamp, Box::new(|_| Err(injected())))),
            6 => observer.fault = Some((Site::PendingRemove, Box::new(|_| Err(injected())))),
            7 => observer.fault = Some((Site::Observation, Box::new(|_| Err(injected())))),
            _ => {}
        }
        observer
    }
    pub(crate) fn assert_writer(facts: &Facts, case: usize) {
        let metadata = facts.metadata.as_ref().unwrap();
        assert_eq!(metadata.expected.as_ref().unwrap().len(), 5);
        assert_eq!(metadata.files.len(), 5);
        if case == 0 {
            assert_eq!(metadata.diagnostics.len(), 1);
            assert_eq!(metadata.diagnostics[0].index(), 3);
        } else {
            assert!(metadata.diagnostics.is_empty());
        }
        for (index, file) in metadata.files.iter().enumerate() {
            if case == 0 && index == 3 {
                assert!(matches!(
                    file.persist(),
                    PersistFact::NoCommitWitness { .. }
                ));
            } else if case == 0 && index < 3 {
                assert!(matches!(file.persist(), PersistFact::NotAttempted { .. }));
            } else {
                assert!(matches!(file.persist(), PersistFact::Committed { .. }));
            }
        }
        assert_eq!(
            facts.document,
            if case < 2 {
                Call::Entered
            } else {
                Call::ReturnedOk
            }
        );
        assert_eq!(
            facts.record,
            match case {
                0 | 1 => Call::NotEntered,
                2 => Call::Entered,
                _ => Call::ReturnedOk,
            }
        );
    }
    #[test]
    fn actual_session_boundaries() {
        let Some(root) = crate::isolated_profile::enter(
            "session::save_attempt_observation::tests::actual_session_boundaries",
        ) else {
            return;
        };
        for case in 0..7 {
            let root = root.join(case.to_string());
            fs::create_dir_all(&root).unwrap();
            let (mut session, id, _) = fixture(&root);
            let attempt = session.save_observed(
                id,
                None,
                crate::input_evidence::SavePolicy::Cancel,
                true,
                false,
                None,
                observer(case),
            );
            assert!(attempt.result.is_err(), "case {case}");
            match (case, attempt.result.as_ref().unwrap_err()) {
                (0, Error::Format(basecraft_io::Error::Io(error))) => {
                    assert!(error.raw_os_error().is_some());
                }
                (2, Error::Damaged { .. }) => {} // Journal owner already formatted its IO cause.
                (3, Error::Io(error)) => assert!(error.raw_os_error().is_some()),
                (1 | 4..=6, Error::Io(error)) => {
                    assert_eq!(error.to_string(), "injected boundary failure");
                }
                (_, error) => panic!("unexpected primary for case {case}: {error:?}"),
            }
            assert_eq!(
                attempt.facts.failure,
                Some(match case {
                    0 | 1 => Site::Document,
                    2 => Site::SaveRecord,
                    3 => Site::Provenance,
                    4 => Site::SavedHashes,
                    5 => Site::DiskStamp,
                    6 => Site::PendingRemove,
                    _ => unreachable!(),
                })
            );
            assert_eq!(attempt.facts.accepted_log, case >= 3);
            assert_writer(&attempt.facts, case);
            assert_eq!(attempt.facts.reconciled, case < 4);
            assert_eq!(attempt.facts.saved_hashes, case >= 5);
            assert_eq!(attempt.facts.disk_stamp, case >= 6);
            assert!(session.storage_root.join("pending-save.json").exists());
            let before = inventory(&root);
            drop(attempt);
            assert_eq!(inventory(&root), before);
            // The Session owner stays alive while passive facts are dropped.
            assert!(session.is_open(id));
        }
    }
    #[test]
    fn early_clean_history_and_opaque() {
        let Some(root) = crate::isolated_profile::enter(
            "session::save_attempt_observation::tests::early_clean_history_and_opaque",
        ) else {
            return;
        };
        let (mut session, id, _) = fixture(&root);
        let early = session.save_observed(
            DocId(usize::MAX),
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            false,
            None,
            Observer::default(),
        );
        assert_eq!(early.facts.failure, Some(Site::Admission));
        assert_eq!(early.facts.document, Call::NotEntered);
        session.save(id, None).unwrap();
        let clean = session.save_observed(
            id,
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            false,
            None,
            Observer::default(),
        );
        assert!(clean.result.is_ok());
        assert_eq!(clean.facts.disposition, Disposition::Clean);
        assert_eq!(clean.facts.document, Call::NotEntered);
        let history = session.save_observed(
            id,
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            true,
            None,
            Observer::default(),
        );
        assert!(history.result.is_ok());
        assert_eq!(history.facts.disposition, Disposition::History);
        assert_eq!(history.facts.document, Call::NotEntered);
        assert_eq!(history.facts.record, Call::ReturnedOk);
        let path = root.join("opaque.fasta");
        fs::write(&path, ">a\nACGT\n").unwrap();
        let other = session.open(&path).unwrap();
        session
            .apply(
                other,
                Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                },
            )
            .unwrap();
        let opaque = session.save_observed(
            other,
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            false,
            None,
            Observer::default(),
        );
        assert!(opaque.result.is_ok());
        assert_eq!(opaque.facts.document, Call::ReturnedOk);
        assert!(opaque.facts.metadata.is_none());
    }
    #[test]
    fn primary_and_inspection_errors_remain_distinct() {
        let Some(root) = crate::isolated_profile::enter(
            "session::save_attempt_observation::tests::primary_and_inspection_errors_remain_distinct",
        ) else {
            return;
        };
        let (mut session, id, _) = fixture(&root);
        let mut hooks = observer(0);
        hooks.document_hooks.inspect = Some((
            4,
            Box::new(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected inspection",
                )
                .into())
            }),
        ));
        let observed = session.save_observed(
            id,
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            false,
            None,
            hooks,
        );
        assert!(
            matches!(&observed.result, Err(Error::Format(basecraft_io::Error::Io(e))) if e.raw_os_error().is_some())
        );
        let facts = observed.facts.metadata.as_ref().unwrap();
        assert_eq!(facts.diagnostics.len(), 2);
        let inspection = facts.diagnostics.iter().find(|d| d.index() == 4).unwrap();
        assert!(
            matches!(inspection.failure().cause(), Error::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied)
        );
        // Editing the live owner cannot mutate the moved invocation facts.
        let description = format!("{:?}", observed.facts);
        session.undo(id).unwrap();
        assert_eq!(format!("{:?}", observed.facts), description);
        let before = inventory(&root);
        drop(observed);
        assert_eq!(inventory(&root), before);
    }
    #[test]
    fn prewriter_refusals_have_no_document_facts() {
        let Some(root) = crate::isolated_profile::enter(
            "session::save_attempt_observation::tests::prewriter_refusals_have_no_document_facts",
        ) else {
            return;
        };
        for case in 0..4 {
            let root = root.join(case.to_string());
            fs::create_dir_all(&root).unwrap();
            let (mut session, id, target) = fixture(&root);
            let decision = if case == 0 {
                let decision = session
                    .check_save(id, None)
                    .unwrap()
                    .decide(crate::input_evidence::SavePolicy::Cancel, false);
                session.undo(id).unwrap();
                Some(decision)
            } else {
                None
            };
            if case == 1 {
                fs::create_dir(session.storage_root.join("pending-save.json")).unwrap();
            }
            if case == 2 {
                fs::write(&target, b"changed externally").unwrap();
            }
            let _guard = if case == 3 {
                Some(crate::save_guard::lock(&target).unwrap())
            } else {
                None
            };
            let before = inventory(&root.join("source"));
            let observed = session.save_observed(
                id,
                None,
                crate::input_evidence::SavePolicy::Cancel,
                true,
                false,
                decision,
                Observer::default(),
            );
            assert!(observed.result.is_err());
            assert_eq!(observed.facts.document, Call::NotEntered);
            assert_eq!(observed.facts.record, Call::NotEntered);
            assert!(observed.facts.metadata.is_none());
            assert_eq!(
                observed.facts.failure,
                Some(match case {
                    0 => Site::Evidence,
                    1 => Site::PendingWrite,
                    2 => Site::SourceCheck,
                    _ => Site::TargetGuard,
                })
            );
            assert_eq!(inventory(&root.join("source")), before);
        }
    }
    pub(crate) fn replacing_observer() -> Observer {
        let mut observer = Observer::default();
        observer.document_hooks.inspect = Some((
            4,
            Box::new(|path| {
                let bytes = fs::read(path)?;
                fs::hard_link(path, path.with_extension("held"))?;
                basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
                Ok(())
            }),
        ));
        observer
    }
    pub(crate) fn assert_distinct_identity(facts: &Facts) {
        use crate::analysis_store::publication_observation::TargetObservation;
        let file = &facts.metadata.as_ref().unwrap().files[4];
        let PersistFact::Committed { witness } = file.persist() else {
            panic!("missing persist witness")
        };
        let TargetObservation::Present {
            identity,
            hash,
            size,
        } = file.target()
        else {
            panic!("missing current target")
        };
        assert_ne!(witness.identity(), identity);
        assert_eq!(witness.hash(), hash);
        assert_eq!(witness.size(), *size);
    }
    #[test]
    fn same_bytes_new_identity_is_passive() {
        let Some(root) = crate::isolated_profile::enter(
            "session::save_attempt_observation::tests::same_bytes_new_identity_is_passive",
        ) else {
            return;
        };
        let (mut session, id, _) = fixture(&root);
        let observed = session.save_observed(
            id,
            None,
            crate::input_evidence::SavePolicy::Cancel,
            true,
            false,
            None,
            replacing_observer(),
        );
        assert!(observed.result.is_ok());
        assert_distinct_identity(&observed.facts);
        let before = inventory(&root);
        drop(observed);
        assert_eq!(inventory(&root), before);
        assert!(session.is_open(id));
    }
}
