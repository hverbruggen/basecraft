//! Kind-specific behavior borrowed by the shared Session.
//!
//! Migration is incremental: Submission, Metadata, Alignment, Tree, Sequence and Delimitation implement this interface today.
//! All six existing payloads participate; Sanger review integration remains separate.
//! Payloads are not constrained to a single sequence/record, so future result,
//! multi-record and Sanger documents can retain their own models here.

use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use crate::Result;
use crate::files::Format;
use crate::session::Inverse;

/// Serialized candidates live beside their destinations until the pending-save
/// record is durable. Keeping these files avoids a second serialization.
#[derive(Default)]
pub(crate) struct PreparedWrite {
    files: Vec<(PathBuf, String, tempfile::NamedTempFile)>,
    intent: Option<(PathBuf, tempfile::NamedTempFile)>,
    pub run: Option<crate::analysis_store::ArtifactRun>,
    pub reverse: bool,
}

impl PreparedWrite {
    pub fn pending(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(bytes)?;
        self.intent = Some((path.into(), file));
        Ok(())
    }
    pub fn push(
        &mut self,
        path: &Path,
        write: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
    ) -> Result<String> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        let mut writer = crate::files::HashingWriter::new(std::io::BufWriter::with_capacity(
            1 << 20,
            file.as_file_mut(),
        ));
        write(&mut writer)?;
        writer.flush()?;
        let hash = writer.hash();
        drop(writer);
        self.files.push((path.into(), hash.clone(), file));
        Ok(hash)
    }
    pub fn hashes(&self) -> Vec<(PathBuf, String)> {
        self.files
            .iter()
            .map(|(path, hash, _)| (path.clone(), hash.clone()))
            .collect()
    }
    pub(crate) fn publish(self) -> Result<Vec<(PathBuf, String)>> {
        self.publish_with(|| Ok(()))
    }
    pub(crate) fn publish_with(
        mut self,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<Vec<(PathBuf, String)>> {
        // Independent candidates may share a disk flush. Every file is durable
        // before any destination is replaced; errors leave originals untouched.
        std::thread::scope(|scope| -> Result<()> {
            let jobs: Vec<_> = self
                .files
                .iter()
                .map(|(_, _, file)| file)
                .chain(self.intent.iter().map(|(_, file)| file))
                .map(|file| {
                    scope.spawn(move || {
                        #[cfg(feature = "test-counters")]
                        basecraft_io::counters::record_sync_all();
                        file.as_file().sync_all()
                    })
                })
                .collect();
            let result = before_publish();
            for job in jobs {
                job.join().expect("file sync worker panicked")?;
            }
            result
        })?;
        // Publish the durable recovery intent before replacing any user file.
        if let Some((path, file)) = self.intent.take() {
            file.persist(path).map_err(|error| error.error)?;
        }
        let hashes = self.hashes();
        if self.reverse {
            self.files.reverse();
        }
        let mut replaced = Vec::new();
        for (path, _, file) in self.files {
            file.persist(&path).map_err(|error| crate::Error::Damaged {
                path: path.clone(),
                detail: format!("partial save: replaced {}; file replacement failed: {}. Keep the open document and retry saving", replaced.join(", "), error.error),
            })?;
            replaced.push(path.display().to_string());
        }
        Ok(hashes)
    }
}

mod alignment;
mod delimitation;
mod metadata;
mod sequence;
mod submission;
mod tree;

/// A payload staged before Session starts durable tracking. The live document
/// stays unchanged until the journal owner is ready to apply the edit.
pub(crate) enum PreparedEdit {
    Tree(Box<basecraft_docs::TreeDoc>),
}

pub(crate) trait Document {
    fn kind(&self) -> crate::files::Kind;
    /// Operations requiring preparation must reject the wrong payload before
    /// Session retains originals or starts a journal.
    fn check_preparation_kind(&self, op: &Op, live: bool) -> Result<()> {
        if matches!(
            op,
            Op::TreeMidpointRoot {} | Op::TreeMinimumVarianceRoot {} | Op::TreeStyle { .. }
        ) && self.kind() != crate::files::Kind::Tree
        {
            return Err(crate::error::invalid(format!("{} needs a tree", op.name())));
        }
        if live
            && let Op::SubmissionEdit { edit } = op
            && matches!(
                edit.as_ref(),
                basecraft_protocol::submission::SubmissionEdit::ApplyInclusion { .. }
            )
            && self.kind() != crate::files::Kind::Submission
        {
            return Err(crate::error::invalid("choose a submission document"));
        }
        Ok(())
    }
    /// Whether admission must retain the discovered companion hashes.
    fn retains_companions(&self) -> bool {
        false
    }
    fn opening_warning(&self) -> Option<&str> {
        None
    }
    fn rebase_replay_paths(&mut self, _from: &Path, _to: &Path) {}
    /// Detached proposal staging retains the same payload and writer metadata.
    /// Unsupported kinds cannot be mutated through the proposal route.
    fn proposal_candidate(&self) -> Result<crate::files::Loaded> {
        Err(crate::error::invalid(
            "this document kind does not support proposals",
        ))
    }
    fn apply_proposal(&mut self, _op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        Err(crate::error::invalid(
            "this document kind does not support proposals",
        ))
    }
    fn revision(&self, generation: u64) -> u64;
    fn fingerprint(&self) -> DocState;
    /// Apply transactionally: an error must leave the payload unchanged.
    /// Session owns actor, history, journal and external input verification.
    fn prepare_edit(&self, _op: &Op, _root: &Path, _live: bool) -> Result<Option<PreparedEdit>> {
        Ok(None)
    }
    fn apply_prepared(
        &mut self,
        op: &Op,
        prepared: Option<PreparedEdit>,
    ) -> Result<(Inverse, Option<FeatureId>)> {
        if prepared.is_some() {
            return Err(crate::error::invalid(
                "prepared edit has the wrong document kind",
            ));
        }
        self.apply(op)
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)>;
    fn revert(&mut self, inverse: &Inverse) -> Result<()>;
    /// Redo using the original inverse when an edit allocated stable identities.
    /// Session still owns stacks and the journal; payloads only restore the edit.
    fn reapply(&mut self, op: &Op, _inverse: &Inverse) -> Result<Inverse> {
        self.apply(op).map(|(inverse, _)| inverse)
    }

    /// Write using atomic replacement per file. Session owns save guards,
    /// partial-save recovery and document provenance publication. Kind-specific
    /// analysis records use the existing record owner from the writer. Paths remain absolute/as supplied;
    /// Session converts them to project-relative log keys. The recovery root is
    /// diagnostic context for partial multi-file saves, not owned by the payload.
    /// Prepare linked evidence when saving a document to another location.
    /// Session publishes it with the guarded document write and records outputs.
    fn prepare_links(
        &self,
        _original: &Path,
        _target: &Path,
        _format: Format,
    ) -> Result<Option<crate::links::PreparedLinks>> {
        Ok(None)
    }
    #[allow(dead_code)] // Recovery checkpoints in lane open use this immediate writer.
    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        publication: &WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        let prepared = self.prepare_write(target, format, recovery_root, publication)?;
        self.publish_write(prepared, target, publication)
    }
    fn prepare_write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        publication: &WritePublication,
    ) -> Result<PreparedWrite>;
    fn publish_write(
        &self,
        prepared: PreparedWrite,
        target: &Path,
        publication: &WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        let run = prepared.run.clone();
        let hashes = prepared.publish()?;
        self.finish_write(target, &hashes, run.as_ref(), publication)?;
        Ok(hashes)
    }
    fn finish_write(
        &self,
        _target: &Path,
        _hashes: &[(PathBuf, String)],
        _run: Option<&crate::analysis_store::ArtifactRun>,
        _publication: &WritePublication,
    ) -> Result<()> {
        Ok(())
    }
    /// Files replaced independently before the primary file. Recovery accepts
    /// each matching expected hash even when the primary replacement failed.
    /// Ordering matches `saved_hashes` (metadata includes its manifest here).
    fn independently_saved_files(&self, _target: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Companion accepted only after the main file matches the intended write.
    /// Its expected hash is the second element of `saved_hashes`.
    fn saved_companion(&self, _target: &Path) -> Option<PathBuf> {
        None
    }

    /// Whether saved hashes and their errors depend only on the complete payload.
    /// Opt-in requires stable transitive serializer inputs between Slot mutable
    /// accesses: no target, publication, filesystem, time or shared payload mutation.
    /// Current opted-in Session constructors use owned backing or immutable indexed
    /// FASTA backing. The public `Residues` trait and `Source::Mapped` alone do not
    /// guarantee this invariant; future constructors must preserve it or opt out.
    fn saved_hashes_are_content_only(&self) -> bool {
        false
    }

    /// Preserve the existing replay writer, including historical target handling.
    fn saved_hashes(&self, target: &Path, publication: &WritePublication) -> Result<Vec<String>>;
    /// Kind-specific operation composition. Session owns timing, actor/group
    /// boundaries, pending history and stitching the existing inverse steps.
    fn merge(&self, _previous: &Op, _next: &Op) -> Option<Op> {
        None
    }
}

// Discovery precedes payload loading, so it cannot borrow a Document yet.
pub(crate) use submission::companions as submission_companions;

// Shared by staged support import and drawing-edit replay on detached TreeDocs.
pub(crate) use tree::execute_tree;

/// Placement only: no consent, policy, source or overwrite authority enters a writer.
#[derive(Default)]
pub(crate) enum WritePublication {
    #[default]
    Immediate,
    Recovery {
        stage: PathBuf,
        destination: PathBuf,
        stores: std::sync::Mutex<Vec<crate::analysis_store::ArtifactRun>>,
    },
}

impl WritePublication {
    pub(crate) fn logical(&self, path: &Path) -> Result<PathBuf> {
        match self {
            Self::Immediate => Ok(path.into()),
            Self::Recovery {
                stage, destination, ..
            } => Ok(destination.join(
                path.strip_prefix(stage)
                    .map_err(|_| crate::error::invalid("recovery write lies outside stage"))?,
            )),
        }
    }

    pub(crate) fn register(&self, run: &crate::analysis_store::ArtifactRun) -> Result<()> {
        match self {
            Self::Immediate => crate::input_evidence::register(run),
            Self::Recovery { stores, .. } => {
                let mut stores = stores
                    .lock()
                    .map_err(|_| crate::error::invalid("recovery stores unavailable"))?;
                if !stores.iter().any(|old| old.directory() == run.directory()) {
                    stores.push(run.clone());
                }
                Ok(())
            }
        }
    }
}
