use std::path::{Path, PathBuf};

use crate::files::HashingWriter;
use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op, delimitation::DelimitationWorkspace};

use super::Document;
use crate::{Result, error::invalid, files, session::Inverse};

impl Document for DelimitationWorkspace {
    fn accepts(&self, op: &Op) -> bool {
        matches!(op, Op::DelimitationEdit { .. })
    }

    fn kind(&self) -> crate::files::Kind {
        crate::files::Kind::Delimitation
    }

    fn retains_companions(&self) -> bool {
        true
    }
    fn rebase_replay_paths(&mut self, from: &Path, to: &Path) {
        files::rebase_delimitation_paths(self, from, to);
    }

    fn proposal_candidate(&self) -> Result<files::Loaded> {
        Ok(files::Loaded::Delimitation(self.clone()))
    }
    fn apply_proposal(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        use basecraft_protocol::delimitation::DelimitationEdit;
        if !matches!(op, Op::DelimitationEdit { edit } if matches!(edit.as_ref(), DelimitationEdit::Remove { .. } | DelimitationEdit::Combine { .. }))
        {
            return Err(invalid(
                "proposal contains an unsupported hypothesis edit or host/lifecycle operation",
            ));
        }
        self.apply(op)
    }
    fn revision(&self, generation: u64) -> u64 {
        generation
    }
    fn fingerprint(&self) -> DocState {
        delimitation_state(self)
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        // Preserve the existing workspace snapshot inverse. Edit-sized inverses
        // remain separate debt; this migration does not change undo policy.
        execute_delimitation(self, op)
    }
    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Delimitation(undo) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        undo.undo(self);
        Ok(())
    }
    fn saved_companions(
        &self,
        project: Option<&crate::project::Project>,
        target: &Path,
    ) -> Vec<PathBuf> {
        vec![files::companion_path(project, target)]
    }

    fn prepare_write(
        &self,
        project: Option<&crate::project::Project>,
        target: &Path,
        format: files::Format,
        _recovery_root: &Path,
        publication: &super::WritePublication,
    ) -> Result<super::PreparedWrite> {
        if format != files::Format::Delimitation {
            return Err(invalid(
                "a delimitation workspace requires SPART or SPART-XML",
            ));
        }
        let new_run = if crate::records::analysis_store::ArtifactRun::for_result(target)?.is_none()
        {
            Some(crate::records::analysis_store::ArtifactRun::create(
                target,
                &ulid::Ulid::generate().to_string(),
            )?)
        } else {
            None
        };
        let mut prepared = super::PreparedWrite::default();
        let xml = files::delimitation_xml(target);
        let main_hash = prepared.push(target, |w| files::write_delimitation(self, xml, w))?;
        let sidecar = if let Some(run) = &new_run {
            run.artifact(&files::run_companion_name(target))?
        } else {
            let companion = files::companion_path(project, target);
            files::companion_folder(&companion)?;
            companion
        };
        prepared.check_record::<basecraft_protocol::companion_records::Companion>(&sidecar);
        prepared.push(&sidecar, |w| {
            files::delimitation_companion(
                self,
                &main_hash,
                &publication.logical(target).map_err(std::io::Error::other)?,
            )?
            .write(w)?;
            Ok(())
        })?;
        prepared.run = new_run;
        Ok(prepared)
    }
    fn finish_write(
        &self,
        target: &Path,
        hashes: &[(PathBuf, String)],
        run: Option<&crate::records::analysis_store::ArtifactRun>,
        publication: &super::WritePublication,
    ) -> Result<()> {
        crate::records::analysis_store::publish_delimitation(
            self,
            target,
            &hashes[0].1,
            run,
            publication,
        )
    }
    fn saved_hashes(
        &self,
        target: &Path,
        publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        let mut main = HashingWriter::new(std::io::sink());
        files::write_delimitation(self, files::delimitation_xml(target), &mut main)?;
        let hash = main.hash();
        let mut side = HashingWriter::new(std::io::sink());
        files::delimitation_companion(self, &hash, &publication.logical(target)?)?
            .write(&mut side)?;
        Ok(vec![hash, side.hash()])
    }
}

pub(crate) fn delimitation_state(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> DocState {
    DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Delimitation {
        specimens: work.specimens.len() as u64,
        hypotheses: work.hypotheses.len() as u64,
    })
}

pub(crate) fn execute_delimitation(
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
    op: &Op,
) -> Result<(Inverse, Option<FeatureId>)> {
    let Op::DelimitationEdit { edit } = op else {
        return Err(invalid("this needs a delimitation edit"));
    };
    let before = work.clone();
    basecraft_docs::delimitation::apply(work, edit).map_err(|e| invalid(e.0))?;
    let undo = crate::session::WorkspaceUndo::between(before, work);
    Ok((Inverse::Delimitation(Box::new(undo)), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::delimitation::DelimitationEdit;

    const SPART: &str = include_str!("../../../../testdata/fixtures/delimitation/manual.spart");

    #[test]
    fn dispatch_rejects_wrong_operations_and_restores_snapshot() {
        crate::isolated_profile::isolate!();
        let initial = basecraft_io::spart::parse_text(SPART).unwrap();
        let mut loaded = files::Loaded::Delimitation(initial.clone());
        let doc = loaded.as_document_mut();
        assert_eq!(doc.revision(42), 42);
        let state = doc.fingerprint();
        assert!(doc.apply(&Op::TreeMidpointRoot {}).is_err());
        assert!(doc.revert(&Inverse::Alignment(Vec::new())).is_err());
        assert!(
            doc.revert(&Inverse::Delimitation(Box::new(
                crate::session::WorkspaceUndo::between(initial.clone(), &initial)
            )))
            .is_ok()
        );
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("wrong.fasta");
        assert!(
            doc.write(
                None,
                &target,
                files::Format::Fasta,
                dir.path(),
                &super::super::WritePublication::Immediate
            )
            .is_err()
        );
        assert!(!target.exists());
        assert!(!dir.path().join(".basecraft").exists());
        assert_eq!(doc.fingerprint(), state);
        let (inverse, feature) = doc
            .apply(&Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Rename {
                    hypothesis: initial.hypotheses[0].id.clone(),
                    name: "Reviewed".into(),
                }),
            })
            .unwrap();
        assert!(feature.is_none());
        doc.revert(&inverse).unwrap();
        let files::Loaded::Delimitation(actual) = loaded else {
            unreachable!()
        };
        assert_eq!(actual, initial);
    }

    #[test]
    fn exchange_companion_and_record_bytes_keep_existing_writers() {
        crate::isolated_profile::isolate!();
        // The Session save boundary holds this lock and prepares registry storage.
        let _lock = crate::records::input_evidence::lock().unwrap();
        let initial = basecraft_io::spart::parse_text(SPART).unwrap();
        for name in ["copy.spart", "copy.spartxml"] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join(name);
            let loaded = files::Loaded::Delimitation(initial.clone());
            let doc = loaded.as_document();
            let mut main = Vec::new();
            files::write_delimitation(&initial, files::delimitation_xml(&target), &mut main)
                .unwrap();
            let hash = basecraft_io::hash_bytes(&main);
            let mut side = Vec::new();
            files::delimitation_companion(&initial, &hash, &target)
                .unwrap()
                .write(&mut side)
                .unwrap();
            let expected = vec![hash.clone(), basecraft_io::hash_bytes(&side)];
            assert_eq!(
                doc.saved_hashes(&target, &super::super::WritePublication::Immediate)
                    .unwrap(),
                expected
            );
            let written = doc
                .write(
                    None,
                    &target,
                    files::Format::Delimitation,
                    dir.path(),
                    &super::super::WritePublication::Immediate,
                )
                .unwrap();
            assert_eq!(
                written.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(std::fs::read(&written[0].0).unwrap(), main);
            assert_eq!(std::fs::read(&written[1].0).unwrap(), side);
            let run = crate::records::analysis_store::ArtifactRun::for_result(&target)
                .unwrap()
                .unwrap();
            // A SPART input is kept under a SPART name.
            let retained = crate::records::analysis_store::submitted_input(&run).unwrap();
            assert!(retained.to_string_lossy().contains("submitted.spart"));
            assert_eq!(std::fs::read(retained).unwrap(), main);
            let input = crate::records::input_evidence::read_input(&run).unwrap();
            assert_eq!(input.submitted_hash, hash);
            assert_eq!(input.submitted_size, main.len() as u64);
            assert_eq!(
                input.state,
                basecraft_protocol::input_records::EvidenceState::Generated
            );
            // Match the pre-extraction sorted JSON record bytes, including the
            // newline added by write_json. Golden typed-record tests also apply.
            let record = serde_json::json!({"schema_version":1,
                "id":run.file_name().unwrap().to_str().unwrap(),
                "result":name,"status":"saved"});
            let expected_record = basecraft_protocol::records::encode(&record).unwrap();
            assert_eq!(
                std::fs::read(run.join("run.json")).unwrap(),
                expected_record
            );
            assert_eq!(files::load_delimitation(&target).unwrap(), initial);
            // Saving an already-bound result reuses its run and companion.
            assert_eq!(
                doc.write(
                    None,
                    &target,
                    files::Format::Delimitation,
                    dir.path(),
                    &super::super::WritePublication::Immediate
                )
                .unwrap(),
                written
            );
            assert_eq!(
                crate::records::analysis_store::ArtifactRun::for_result(&target).unwrap(),
                Some(run)
            );
        }
    }
    #[test]
    fn companion_failure_preserves_main_and_reports_error() {
        crate::isolated_profile::isolate!();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("partial.spart");
        let work = basecraft_io::spart::parse_text(SPART).unwrap();
        let run =
            crate::records::analysis_store::ArtifactRun::create(&target, "partial-test").unwrap();
        std::fs::write(&target, SPART).unwrap();
        run.bind(&target, &basecraft_io::hash_file(&target).unwrap())
            .unwrap();
        let side = files::companion_path(None, &target);
        std::fs::create_dir(&side).unwrap();
        assert!(
            work.write(
                None,
                &target,
                files::Format::Delimitation,
                dir.path(),
                &super::super::WritePublication::Immediate
            )
            .is_err()
        );
        assert_eq!(
            basecraft_io::hash_file(&target).unwrap(),
            work.saved_hashes(&target, &super::super::WritePublication::Immediate)
                .unwrap()[0]
        );
        assert!(side.is_dir());
        assert!(!run.artifact("input.json").unwrap().exists());
        assert!(!run.artifact("run.json").unwrap().exists());
    }
}
