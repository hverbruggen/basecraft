//! Kind-specific behavior borrowed by the shared Session.
//!
//! Seven payloads share identity, journal, undo and save ownership. Each payload
//! supplies its own edits and serializer; scientific review history stays specialised.

use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::facts::{Alphabet, Fact, FactSource, SequenceAlphabet, StatedAlphabet};
use basecraft_protocol::{DocState, Op};

use crate::Result;
use crate::files::Format;
use crate::session::Inverse;

/// Serialized candidates live beside their destinations until the pending-save
/// record is durable. Keeping these files avoids a second serialization.
#[derive(Default)]
pub(crate) struct PreparedWrite {
    files: Vec<(PathBuf, String, tempfile::NamedTempFile)>,
    /// Companions the document no longer needs, removed after the files are replaced.
    stale: Vec<PathBuf>,
    intent: Option<(PathBuf, tempfile::NamedTempFile)>,
    pub run: Option<crate::records::analysis_store::ArtifactRun>,
    pub reverse: bool,
    /// The owner accepted an absent main file: never replace another writer's.
    pub create: bool,
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
    /// Remove `path` once the files are written, if it exists: a companion
    /// whose facts are gone must not bring them back on reopen.
    pub fn remove_stale(&mut self, path: &Path) {
        self.stale.push(path.into());
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
        // Only the main file is new to the owner; companions may be replaced.
        let main = self.files.first().map(|(path, _, _)| path.clone());
        if self.reverse {
            self.files.reverse();
        }
        let mut replaced = Vec::new();
        for (path, _, file) in self.files {
            let persisted = if self.create && main.as_ref() == Some(&path) {
                file.persist_noclobber(&path)
            } else {
                file.persist(&path)
            };
            persisted.map_err(|error| crate::Error::Damaged {
                path: path.clone(),
                detail: format!("partial save: replaced {}; file replacement failed: {}. Keep the open document and retry saving", replaced.join(", "), error.error),
            })?;
            replaced.push(path.display().to_string());
        }
        for path in self.stale {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(crate::Error::Damaged {
                        path,
                        detail: format!(
                            "saved, but an outdated companion could not be removed: {error}"
                        ),
                    });
                }
                _ => {}
            }
        }
        Ok(hashes)
    }
}

mod alignment;
mod delimitation;
mod metadata;
mod sanger;
mod sequence;
mod submission;
pub(crate) mod tree;

/// The user's alphabet, as the fact `doc.set_alphabet` records.
fn user_alphabet(value: StatedAlphabet) -> Fact<Alphabet> {
    Fact {
        value: value.into(),
        source: FactSource::User,
    }
}

impl crate::Session {
    /// The letters an alignment or sequence is written in: what the user
    /// said, kept in its companion, or what the file says, or else
    /// basecraft's guess (D12): the sequences' common value, "mixed" when
    /// they differ. A sequence file counts every record it holds.
    pub fn alphabet(&self, id: crate::DocId) -> Result<Fact<Alphabet>> {
        let content = self.content(id)?;
        if let Some(opened) = content.alignment() {
            return Ok(opened.doc.alphabet());
        }
        let opened = sequence(content)?;
        let open = opened.doc.alphabet();
        if open.source == FactSource::User || opened.others.alphabets.is_empty() {
            return Ok(open);
        }
        let others = opened.others.alphabets.iter().map(|(_, fact)| fact);
        let all: Vec<&Fact<Alphabet>> = std::iter::once(&open).chain(others).collect();
        let stated = all.iter().all(|fact| fact.source == FactSource::Stated);
        Ok(Fact {
            value: Alphabet::common(all.iter().map(|fact| fact.value)),
            source: if stated {
                FactSource::Stated
            } else {
                FactSource::Guessed
            },
        })
    }

    /// Each sequence's alphabet, guessed or stated by the file: the rows of
    /// an alignment, or every record of a sequence file.
    pub fn sequence_alphabets(&self, id: crate::DocId) -> Result<Vec<SequenceAlphabet>> {
        let content = self.content(id)?;
        let entry = |name: &str, alphabet| SequenceAlphabet {
            name: name.to_owned(),
            alphabet,
        };
        if let Some(opened) = content.alignment() {
            return Ok(opened
                .doc
                .rows()
                .map(|row| entry(&row.name, row.alphabet()))
                .collect());
        }
        let opened = sequence(content)?;
        let open = opened.doc.alphabet();
        let open = if open.source == FactSource::User {
            opened.doc.guessed_alphabet()
        } else {
            open.value
        };
        let others = opened
            .others
            .alphabets
            .iter()
            .map(|(name, fact)| entry(name, fact.value));
        Ok(std::iter::once(entry(&opened.doc.name, open))
            .chain(others)
            .collect())
    }
}

fn sequence(content: &crate::files::Loaded) -> Result<&crate::files::Opened> {
    content
        .sequence()
        .ok_or_else(|| crate::error::invalid("only alignments and sequences have an alphabet"))
}

/// Stage the facts companion of an alignment or sequence: written when it
/// states an alphabet, removed when it no longer does.
fn stage_companion(
    prepared: &mut PreparedWrite,
    target: &Path,
    alphabet: Option<&Fact<Alphabet>>,
    main: &str,
) -> Result<()> {
    let path = crate::files::tree_sidecar(target);
    match crate::files::facts_companion(alphabet, main) {
        Some(companion) => {
            prepared.push(&path, |w| companion.write(w))?;
        }
        None => prepared.remove_stale(&path),
    }
    Ok(())
}

/// The hash of the companion a save writes for a stated alphabet, if any.
fn facts_hash(alphabet: Option<&Fact<Alphabet>>, main: &str) -> Result<Option<String>> {
    let Some(companion) = crate::files::facts_companion(alphabet, main) else {
        return Ok(None);
    };
    let mut hw = crate::files::HashingWriter::new(std::io::sink());
    companion.write(&mut hw)?;
    Ok(Some(hw.hash()))
}

pub(crate) trait Document {
    fn kind(&self) -> crate::files::Kind;
    /// Why saving would leave related data inconsistent, if it would.
    /// `own_target` is true when the document's own file is replaced.
    fn save_refusal(&self, _own_target: bool) -> Option<String> {
        None
    }
    /// Reject operations for another payload before durable tracking starts.
    fn accepts(&self, op: &Op) -> bool;
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
        Err(crate::Error::Unsupported(
            "this document kind does not support proposals".into(),
        ))
    }
    fn apply_proposal(&mut self, _op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        Err(crate::Error::Unsupported(
            "this document kind does not support proposals".into(),
        ))
    }
    fn revision(&self, generation: u64) -> u64;
    fn fingerprint(&self) -> DocState;
    /// Apply transactionally: an error must leave the payload unchanged.
    /// Session owns actor, history, journal and external input verification.
    /// Check the record an edit names, and name the open record when the
    /// edit leaves it out in a file of several records.
    fn name_record(&self, _op: &mut Op) -> Result<()> {
        Ok(())
    }
    fn validate(&self, _op: &Op, _root: &Path, _live: bool) -> Result<()> {
        Ok(())
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
    ) -> Result<Option<crate::records::links::PreparedLinks>> {
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
        let mut prepared = prepared;
        prepared.create = matches!(publication, WritePublication::Create);
        let hashes = prepared.publish()?;
        self.finish_write(target, &hashes, run.as_ref(), publication)?;
        Ok(hashes)
    }
    fn finish_write(
        &self,
        _target: &Path,
        _hashes: &[(PathBuf, String)],
        _run: Option<&crate::records::analysis_store::ArtifactRun>,
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

    /// Companions accepted only after the main file matches the intended write,
    /// in the order of their hashes after the first in `saved_hashes`.
    fn saved_companions(&self, _target: &Path) -> Vec<PathBuf> {
        Vec::new()
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
    /// The owner accepted an absent destination; promotion must not replace another writer.
    Create,
    Recovery {
        stage: PathBuf,
        destination: PathBuf,
        stores: std::sync::Mutex<Vec<crate::records::analysis_store::ArtifactRun>>,
    },
}

impl WritePublication {
    pub(crate) fn logical(&self, path: &Path) -> Result<PathBuf> {
        match self {
            Self::Immediate | Self::Create => Ok(path.into()),
            Self::Recovery {
                stage, destination, ..
            } => Ok(destination.join(
                basecraft_platform::files::inside(path, stage)
                    .ok_or_else(|| crate::error::invalid("recovery write lies outside stage"))?,
            )),
        }
    }

    pub(crate) fn register(&self, run: &crate::records::analysis_store::ArtifactRun) -> Result<()> {
        match self {
            Self::Immediate | Self::Create => crate::records::input_evidence::register(run),
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
