//! Passive, invocation-scoped save boundary hooks. Legacy hooks perform no IO.
//! Successful calls are not durability witnesses; reconciliation is not authority.
use super::*;

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
    fn write<D: Document>(
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
    fn write<D: Document>(
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
                ))
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
                ))
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
                ))
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
            if case < 2 {
                Call::NotEntered
            } else if case == 2 {
                Call::Entered
            } else {
                Call::ReturnedOk
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
}
