//! Passive, invocation-scoped save boundary hooks. Legacy hooks perform no IO.
//! Successful calls are not durability witnesses; reconciliation is not authority.
use super::Session;
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
