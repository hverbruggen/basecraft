use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::Format;
use crate::metadata::OpenedMetadata;
use crate::session::Inverse;

impl Document for OpenedMetadata {
    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Metadata
    }

    fn revision(&self, generation: u64) -> u64 {
        generation
    }

    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Metadata {
            specimens: self.metadata.specimens.rows.len() as u64,
            identifications: self.metadata.identifications.len() as u64,
            links: self.metadata.links.len() as u64,
        })
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let Op::MetadataEdit { edit } = op else {
            return Err(invalid(format!(
                "{} can't be applied to the project metadata",
                op.name()
            )));
        };
        // Retain the existing snapshot inverse. Edit-sized metadata inverses
        // remain separate debt; this migration does not change snapshot policy.
        let before = Box::new(self.metadata.clone());
        basecraft_docs::metadata::apply(&mut self.metadata, edit).map_err(|e| invalid(e.0))?;
        Ok((Inverse::Metadata(before), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Metadata(before) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        self.metadata = (**before).clone();
        Ok(())
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        produce(self, target, format, &mut Unobserved).legacy(recovery_root)
    }

    #[cfg(test)]
    fn write_observed(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
        hooks: observation::Hooks,
    ) -> super::DocumentAttempt {
        observation::write_observed(self, target, format, hooks).into_document()
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        Ok(crate::metadata::encode(&self.metadata)?
            .iter()
            .map(|bytes| basecraft_io::hash_bytes(bytes))
            .collect())
    }
}

/// Prepared memory, not a file-backed recovery artifact or publication authority.
#[derive(Debug)]
pub(crate) struct ExpectedOutput {
    path: PathBuf,
    bytes: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MetadataStep {
    ValidateFormat,
    Encode,
    PrepareParent,
    WriteOutput { index: usize },
    ReturnHashes,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParentPreparation {
    NotAttempted,
    CallEnteredWithoutSuccess,
    CallReturnedOk,
}
#[derive(Debug)]
struct Attempt {
    expected: Option<Vec<ExpectedOutput>>,
    parent: ParentPreparation,
    step: MetadataStep,
    producer: Result<Vec<(PathBuf, String)>>,
}
impl Attempt {
    /// Historical message projection, based only on successfully returned calls.
    /// No observations, target reads or additional hashes participate here.
    fn legacy(self, recovery_root: &Path) -> Result<Vec<(PathBuf, String)>> {
        match (self.producer, self.step, self.expected) {
            (Err(error), MetadataStep::WriteOutput { index }, Some(expected))
                if index + 1 < expected.len() =>
            {
                let replaced: Vec<_> = expected[index + 1..]
                    .iter()
                    .rev()
                    .map(|output| output.path.display().to_string())
                    .collect();
                Err(invalid(format!(
                    "partial metadata save: replaced {}; failed at {}: {error}. Metadata generation is incomplete. Keep the open document and retry saving after fixing the destination, or save to a new folder. Temporary recovery, when enabled, is at {}",
                    replaced.join(", "),
                    expected[index].path.display(),
                    recovery_root.display()
                )))
            }
            (producer, _, _) => producer,
        }
    }
}

trait WriteHooks {
    fn prepared(&mut self, _: &[ExpectedOutput]) {}
    fn writing(&mut self, _: usize) {}
    fn staged(&mut self, _: usize, _: &std::fs::File, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn persisted(&mut self, _: usize) {}
    #[cfg(test)]
    fn returning(&mut self) -> Result<()> {
        Ok(())
    }
}
struct Unobserved;
impl WriteHooks for Unobserved {}

fn produce(
    document: &OpenedMetadata,
    target: &Path,
    format: Format,
    hooks: &mut impl WriteHooks,
) -> Attempt {
    let mut attempt = Attempt {
        expected: None,
        parent: ParentPreparation::NotAttempted,
        step: MetadataStep::ValidateFormat,
        producer: Ok(Vec::new()),
    };
    attempt.producer = (|| {
        if format != Format::Metadata {
            return Err(invalid(format!(
                "project metadata is saved as {}",
                crate::metadata::MANIFEST
            )));
        }
        attempt.step = MetadataStep::Encode;
        let encoded = crate::metadata::encode(&document.metadata)?;
        let paths = crate::metadata::files(target);
        let expected = attempt.expected.insert(
            paths
                .into_iter()
                .zip(encoded)
                .map(|(path, bytes)| ExpectedOutput { path, bytes })
                .collect(),
        );
        // The complete roster and buffers exist before the first filesystem effect.
        hooks.prepared(expected);
        attempt.step = MetadataStep::PrepareParent;
        if let Some(parent) = target.parent() {
            attempt.parent = ParentPreparation::CallEnteredWithoutSuccess;
            std::fs::create_dir_all(parent)?;
            attempt.parent = ParentPreparation::CallReturnedOk;
        }
        // Same tables-first/manifest-last producer for both projections. The
        // unobserved specialization is the atomic writer's existing no-op path.
        for (index, output) in expected.iter().enumerate().rev() {
            attempt.step = MetadataStep::WriteOutput { index };
            hooks.writing(index);
            let callbacks = std::cell::RefCell::new(&mut *hooks);
            basecraft_io::write_atomic_observed(
                &output.path,
                |w| w.write_all(&output.bytes),
                |file| callbacks.borrow_mut().staged(index, file, &output.bytes),
                || callbacks.borrow_mut().persisted(index),
            )?;
        }
        attempt.step = MetadataStep::ReturnHashes;
        #[cfg(test)]
        hooks.returning()?;
        Ok(expected
            .iter()
            .map(|output| (output.path.clone(), basecraft_io::hash_bytes(&output.bytes)))
            .collect())
    })();
    attempt
}

// Passive proof only. Session/recovery adoption and retention ownership require
// separate review; compiling this module cannot activate a production route.
#[cfg(test)]
pub(crate) mod observation {
    use super::*;
    use crate::analysis_store::publication_observation::{
        FilePublicationObservation, ObservationFailure, inspect_target,
    };

    pub(crate) type PreparedHook = Box<dyn FnOnce(&[ExpectedOutput])>;
    pub(crate) type StageHook = Box<dyn FnOnce(&std::fs::File) -> std::io::Result<()>>;
    pub(crate) type BeforeHook =
        Box<dyn FnOnce(&FilePublicationObservation) -> std::io::Result<()>>;
    pub(crate) type InspectHook = Box<dyn FnOnce(&Path) -> Result<()>>;
    #[derive(Default)]
    pub(crate) struct Hooks {
        pub returning: Option<Box<dyn FnOnce() -> Result<()>>>,
        pub prepared: Option<PreparedHook>,
        pub stage: Option<(usize, StageHook)>,
        pub before: Option<(usize, BeforeHook)>,
        pub inspect: Option<(usize, InspectHook)>,
    }
    #[derive(Debug)]
    pub(crate) struct OutputDiagnostic {
        index: usize,
        failure: ObservationFailure,
    }
    impl OutputDiagnostic {
        pub fn index(&self) -> usize {
            self.index
        }
        pub fn failure(&self) -> &ObservationFailure {
            &self.failure
        }
    }
    #[derive(Debug)]
    pub(crate) struct ObservedMetadataWrite {
        attempt: Attempt,
        files: Vec<FilePublicationObservation>,
        diagnostics: Vec<OutputDiagnostic>,
    }
    #[derive(Debug)]
    pub(crate) struct MetadataFacts {
        pub expected: Option<Vec<ExpectedOutput>>,
        pub parent: ParentPreparation,
        pub step: MetadataStep,
        pub files: Vec<FilePublicationObservation>,
        pub diagnostics: Vec<OutputDiagnostic>,
    }
    impl ObservedMetadataWrite {
        pub(crate) fn into_document(self) -> super::super::DocumentAttempt {
            let Attempt {
                expected,
                parent,
                step,
                producer,
            } = self.attempt;
            super::super::DocumentAttempt {
                result: producer,
                metadata: Some(MetadataFacts {
                    expected,
                    parent,
                    step,
                    files: self.files,
                    diagnostics: self.diagnostics,
                }),
            }
        }
        pub fn expected(&self) -> Option<&[ExpectedOutput]> {
            self.attempt.expected.as_deref()
        }
        pub fn files(&self) -> &[FilePublicationObservation] {
            &self.files
        }
        pub fn parent(&self) -> &ParentPreparation {
            &self.attempt.parent
        }
        pub fn step(&self) -> &MetadataStep {
            &self.attempt.step
        }
        pub fn producer(&self) -> &Result<Vec<(PathBuf, String)>> {
            &self.attempt.producer
        }
        pub fn diagnostics(&self) -> &[OutputDiagnostic] {
            &self.diagnostics
        }
    }
    struct Collector {
        files: Vec<FilePublicationObservation>,
        attempted: Vec<bool>,
        hooks: Hooks,
    }
    impl WriteHooks for Collector {
        fn returning(&mut self) -> Result<()> {
            self.hooks.returning.take().map_or(Ok(()), |hook| hook())
        }
        fn prepared(&mut self, expected: &[ExpectedOutput]) {
            self.files = expected
                .iter()
                .map(|o| FilePublicationObservation::new(&o.path))
                .collect();
            self.attempted = vec![false; expected.len()];
            if let Some(hook) = self.hooks.prepared.take() {
                hook(expected);
            }
        }
        fn writing(&mut self, index: usize) {
            self.attempted[index] = true;
        }
        fn staged(
            &mut self,
            index: usize,
            file: &std::fs::File,
            bytes: &[u8],
        ) -> std::io::Result<()> {
            if self.hooks.stage.as_ref().is_some_and(|(i, _)| *i == index)
                && let Some((_, hook)) = self.hooks.stage.take()
            {
                hook(file)?;
            }
            let fact = &mut self.files[index];
            fact.staged(file, basecraft_io::hash_bytes(bytes), bytes.len() as u64)?;
            if self.hooks.before.as_ref().is_some_and(|(i, _)| *i == index)
                && let Some((_, hook)) = self.hooks.before.take()
            {
                hook(fact)?;
            }
            // All fallible callback work finished; persist is the next fallible step.
            fact.attempting();
            Ok(())
        }
        fn persisted(&mut self, index: usize) {
            self.files[index].persisted();
        }
    }
    pub(crate) fn write_observed(
        document: &OpenedMetadata,
        target: &Path,
        format: Format,
        hooks: Hooks,
    ) -> ObservedMetadataWrite {
        let mut collector = Collector {
            files: Vec::new(),
            attempted: Vec::new(),
            hooks,
        };
        let attempt = produce(document, target, format, &mut collector);
        let mut diagnostics = Vec::new();
        for (index, fact) in collector.files.iter_mut().enumerate() {
            if !collector.attempted[index] {
                continue;
            }
            let inspected = (|| {
                if collector
                    .hooks
                    .inspect
                    .as_ref()
                    .is_some_and(|(i, _)| *i == index)
                    && let Some((_, hook)) = collector.hooks.inspect.take()
                {
                    hook(fact.destination())?;
                }
                inspect_target(fact.destination())
            })();
            if let Some(failure) = fact.inspected(inspected) {
                diagnostics.push(OutputDiagnostic { index, failure });
            }
        }
        ObservedMetadataWrite {
            attempt,
            files: collector.files,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod publication_tests;
