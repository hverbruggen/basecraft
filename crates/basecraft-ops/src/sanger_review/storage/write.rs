//! Staging a saved review and publishing its exact bytes. The caller owns
//! target/evidence locks, save policy, accepted revisions and recovery acceptance.
//! No review state or undo history is mutated while preparing the write.
use super::{
    ArtifactRun, ReviewEvidence, SangerReviewSession, carry_authored, inputs, invalid, publication,
    retained, unavailable_sources,
};
use basecraft_protocol::provenance::ArtifactRevision;
use std::path::{Path, PathBuf};

pub(super) struct ReviewWrite {
    target: PathBuf,
    pub(super) run: ArtifactRun,
    pub(super) evidence: ReviewEvidence,
}

pub(super) struct PreparedReviewWrite {
    target: PathBuf,
    run: ArtifactRun,
    bytes: Vec<u8>,
}

impl ReviewWrite {
    pub(super) fn allocate(review: &SangerReviewSession, target: &Path) -> crate::Result<Self> {
        let run = ArtifactRun::create(target, &ulid::Ulid::generate().to_string())?;
        let mut evidence = review.evidence.clone();
        evidence.runs = vec![run.directory().into()];
        Ok(Self {
            target: target.into(),
            run,
            evidence,
        })
    }

    /// Stage the immutable snapshot, draft, previous file, visible bytes and
    /// support record before the owner attempts a visible replacement. The
    /// evidence lock must already be held, matching link_generated_input_locked.
    pub(super) fn prepare(
        &self,
        review: &SangerReviewSession,
    ) -> crate::Result<PreparedReviewWrite> {
        let run = &self.run;
        let path = &self.target;
        let mut dependencies = review.evidence.runs.clone();
        for set in &review.retained {
            dependencies.extend(set.evidence.runs.clone());
            dependencies.extend(
                set.evidence
                    .sources
                    .iter()
                    .filter_map(|source| source.ledger.clone()),
            );
        }
        dependencies.extend(
            review
                .evidence
                .sources
                .iter()
                .filter_map(|s| s.ledger.clone()),
        );
        dependencies.sort();
        dependencies.dedup();
        inputs::link_generated_input_locked(
            run,
            &dependencies,
            review.draft_json.as_bytes(),
            "identity-v1",
            "Sanger review/support",
        )?;
        carry_authored(&review.evidence.runs, run)?;
        let snapshot = run.artifact("review.sanger-review.json")?;
        // The recovery copy is complete before attempting a visible replacement.
        let mut snapshot_bytes = review.encoded_at(&snapshot, &self.evidence)?;
        snapshot_bytes.push(b'\n');
        basecraft_io::write_atomic(&snapshot, |w| w.write_all(&snapshot_bytes))?;
        let draft = run.artifact("draft.json")?;
        basecraft_io::write_atomic(&draft, |w| w.write_all(review.draft_json.as_bytes()))?;
        let mut artifacts = vec![retained(run, &snapshot, 0)?, retained(run, &draft, 1)?];
        if path.is_file() {
            let previous = run.artifact("previous-review")?;
            basecraft_io::write_atomic(&previous, |w| {
                std::io::copy(&mut std::fs::File::open(path)?, w)?;
                Ok(())
            })?;
            artifacts.push(retained(run, &previous, 2)?);
        }
        let bytes = review.encoded_at(path, &self.evidence)?;
        let staged = run.artifact("visible-review.json")?;
        basecraft_io::write_atomic(&staged, |w| w.write_all(&bytes))?;
        artifacts.push(retained(run, &staged, artifacts.len())?);
        let mut record = publication(
            run,
            "Explicit saved Sanger review; decisions and support retained",
            artifacts,
        );
        unavailable_sources(&self.evidence, &mut record);
        record.outputs.push(ArtifactRevision {
            id: format!("{}:review", run.id()),
            logical_name: path
                .file_name()
                .ok_or_else(|| invalid("Review has no filename"))?
                .to_string_lossy()
                .into_owned(),
            hash: basecraft_io::hash_bytes(&bytes),
            size: bytes.len() as u64,
            retained: None,
            reference: None,
        });
        crate::provenance::publish(run.directory(), &record)?;
        Ok(PreparedReviewWrite {
            target: self.target.clone(),
            run: run.clone(),
            bytes,
        })
    }
}

impl PreparedReviewWrite {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Revision checks and the target lock belong to the caller. A new target
    /// always uses no-clobber promotion, including a target created after staging.
    pub(super) fn publish(&self, replacing: bool) -> crate::Result<()> {
        let run = &self.run;
        let path = &self.target;
        let bytes = &self.bytes;
        let written = if replacing {
            basecraft_io::write_atomic(path, |w| w.write_all(bytes)).map_err(crate::Error::from)
        } else {
            run.publish_secondary("visible-review.json", path)
        };
        if let Err(error) = written {
            // No-clobber publication can fail syncing the directory after
            // visible bytes are present. Do not describe that as unsaved.
            if basecraft_io::hash_file(path).ok().as_ref() == Some(&basecraft_io::hash_bytes(bytes))
            {
                return Err(invalid(format!(
                    "intended review bytes are visible; publication incomplete: {error}; reopen before retrying; repair evidence: {}",
                    run.directory().display()
                )));
            }
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanger_review::{ReviewChoice, ReviewOp};

    fn review() -> SangerReviewSession {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/sanger-legacy");
        let mut review = SangerReviewSession::new(&fixtures.join("draft-a.json")).unwrap();
        review
            .retain_call_set(SangerReviewSession::new(&fixtures.join("draft-b.json")).unwrap())
            .unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('N'),
            })
            .unwrap();
        review.apply(ReviewOp::Undo).unwrap();
        review
    }

    fn state(review: &SangerReviewSession) -> serde_json::Value {
        serde_json::json!({
            "report": review.document_report(false).unwrap().raw,
            "evidence": review.evidence,
            "undo": review.undo,
            "redo": review.redo,
            "anchor": review.anchor,
            "recovery": review.recovery,
        })
    }

    #[test]
    fn staged_review_is_exact_and_does_not_accept_or_mutate_live_state() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("review.json");
        let review = review();
        let before = state(&review);
        let _lock = inputs::lock().unwrap();
        let write = ReviewWrite::allocate(&review, &target).unwrap();
        let prepared = write.prepare(&review).unwrap();
        assert_eq!(state(&review), before);
        assert!(!target.exists());
        assert!(review.accepted.is_none());
        assert_eq!(
            prepared.bytes(),
            std::fs::read(write.run.artifact("visible-review.json").unwrap()).unwrap()
        );
        let snapshot = write.run.artifact("review.sanger-review.json").unwrap();
        let mut expected = serde_json::to_vec_pretty(
            &review
                .stored_with_evidence(&snapshot, &write.evidence)
                .unwrap(),
        )
        .unwrap();
        expected.push(b'\n');
        assert_eq!(std::fs::read(&snapshot).unwrap(), expected);
        let visible: serde_json::Value = serde_json::from_slice(prepared.bytes()).unwrap();
        assert_eq!(visible["version"], 2);
        assert_eq!(visible["sets"].as_array().unwrap().len(), 2);
        assert_eq!(
            visible["sets"][1],
            serde_json::to_value(review.retained[0].stored_single_at(&target).unwrap()).unwrap()
        );
        prepared.publish(false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), prepared.bytes());
        assert_eq!(
            state(&review),
            before,
            "publication alone is not live acceptance"
        );
        assert!(review.accepted.is_none());
        let reopened = SangerReviewSession::load(&target).unwrap();
        assert_eq!(
            reopened.document_report(false).unwrap().raw,
            review.document_report(false).unwrap().raw
        );
    }

    #[test]
    fn staged_review_preserves_a_concurrently_created_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("review.json");
        let review = review();
        let before = state(&review);
        let _lock = inputs::lock().unwrap();
        let write = ReviewWrite::allocate(&review, &target).unwrap();
        let prepared = write.prepare(&review).unwrap();
        std::fs::write(&target, b"another writer's file").unwrap();
        assert!(prepared.publish(false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"another writer's file");
        assert_eq!(state(&review), before);
        assert!(write.run.directory().join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(write.run.artifact("visible-review.json").unwrap()).unwrap(),
            prepared.bytes()
        );
    }

    #[test]
    fn failed_staging_preserves_the_previous_review_and_live_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("review.json");
        std::fs::write(&target, b"previous review").unwrap();
        let review = review();
        let before = state(&review);
        let _lock = inputs::lock().unwrap();
        let write = ReviewWrite::allocate(&review, &target).unwrap();
        // A real filesystem failure while writing the immutable snapshot.
        std::fs::create_dir(write.run.artifact("review.sanger-review.json").unwrap()).unwrap();
        assert!(write.prepare(&review).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous review");
        assert_eq!(state(&review), before);
        assert!(!write.run.directory().join("manifest.json").exists());
        assert!(
            write.run.directory().exists(),
            "partial evidence is preserved"
        );
    }
}
