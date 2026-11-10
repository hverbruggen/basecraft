use std::path::{Path, PathBuf};

use crate::files::HashingWriter;
use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op, delimitation::DelimitationWorkspace};

use super::Document;
use crate::{Result, error::invalid, files, session::Inverse};

impl Document for DelimitationWorkspace {
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
        let Inverse::Delimitation(before) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        *self = (**before).clone();
        Ok(())
    }
    fn write(
        &self,
        target: &Path,
        format: files::Format,
        _recovery_root: &Path,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != files::Format::Delimitation {
            return Err(invalid(
                "a delimitation workspace requires SPART or SPART-XML",
            ));
        }
        let new_run = if crate::analysis_store::ArtifactRun::for_result(target)?.is_none() {
            Some(crate::analysis_store::ArtifactRun::create(
                target,
                &ulid::Ulid::generate().to_string(),
            )?)
        } else {
            None
        };
        let mut main_hash = String::new();
        let xml = files::delimitation_xml(target);
        basecraft_io::write_atomic(target, |w| {
            let mut hw = HashingWriter::new(w);
            files::write_delimitation(self, xml, &mut hw)?;
            main_hash = hw.hash();
            Ok(())
        })?;
        let sidecar = if let Some(run) = &new_run {
            run.artifact(&format!(
                "{}.basecraft.json",
                target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| invalid("SPART filename must be UTF-8"))?
            ))?
        } else {
            files::tree_sidecar(target)
        };
        let mut side_hash = String::new();
        basecraft_io::write_atomic(&sidecar, |w| {
            let mut hw = HashingWriter::new(w);
            files::write_delimitation_sidecar_at(self, &main_hash, target, &mut hw)?;
            side_hash = hw.hash();
            Ok(())
        })?;
        crate::analysis_store::publish_delimitation(self, target, &main_hash, new_run.as_ref())?;
        Ok(vec![
            (target.to_path_buf(), main_hash),
            (sidecar, side_hash),
        ])
    }
    fn saved_hashes(&self, target: &Path) -> Result<Vec<String>> {
        let mut main = HashingWriter::new(std::io::sink());
        files::write_delimitation(self, files::delimitation_xml(target), &mut main)?;
        let hash = main.hash();
        let mut side = HashingWriter::new(std::io::sink());
        files::write_delimitation_sidecar_at(self, &hash, target, &mut side)?;
        Ok(vec![hash, side.hash()])
    }
}

pub(crate) fn delimitation_state(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> DocState {
    DocState {
        len: work.specimens.len() as u64,
        features: work.hypotheses.len() as u64,
        rows: None,
    }
}

pub(crate) fn execute_delimitation(
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
    op: &Op,
) -> Result<(Inverse, Option<FeatureId>)> {
    let Op::DelimitationEdit { edit } = op else {
        return Err(invalid("this needs a delimitation edit"));
    };
    // Retain the existing inverse policy for these small hypothesis workspaces,
    // which hold assignments/settings rather than sequence residues.
    let before = Box::new(work.clone());
    basecraft_docs::delimitation::apply(work, edit).map_err(|e| invalid(e.0))?;
    Ok((Inverse::Delimitation(before), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::delimitation::DelimitationEdit;

    const SPART: &str = include_str!("../../../../testdata/fixtures/delimitation/manual.spart");

    #[test]
    fn dispatch_rejects_wrong_operations_and_restores_snapshot() {
        let initial = basecraft_io::spart::parse_text(SPART).unwrap();
        let mut loaded = files::Loaded::Delimitation(initial.clone());
        let doc = loaded.as_document_mut().unwrap();
        assert_eq!(doc.revision(42), 42);
        let state = doc.fingerprint();
        assert!(doc.apply(&Op::TreeMidpointRoot {}).is_err());
        assert!(
            doc.revert(&Inverse::Delimitation(Box::new(initial.clone())))
                .is_ok()
        );
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("wrong.fasta");
        assert!(
            doc.write(&target, files::Format::Fasta, dir.path())
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
        let initial = basecraft_io::spart::parse_text(SPART).unwrap();
        for name in ["copy.spart", "copy.spartxml"] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join(name);
            let loaded = files::Loaded::Delimitation(initial.clone());
            let doc = loaded.as_document().unwrap();
            let mut main = Vec::new();
            files::write_delimitation(&initial, files::delimitation_xml(&target), &mut main)
                .unwrap();
            let hash = basecraft_io::hash_bytes(&main);
            let mut side = Vec::new();
            files::write_delimitation_sidecar_at(&initial, &hash, &target, &mut side).unwrap();
            let expected = vec![hash.clone(), basecraft_io::hash_bytes(&side)];
            assert_eq!(doc.saved_hashes(&target).unwrap(), expected);
            let written = doc
                .write(&target, files::Format::Delimitation, dir.path())
                .unwrap();
            assert_eq!(
                written.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(std::fs::read(&written[0].0).unwrap(), main);
            assert_eq!(std::fs::read(&written[1].0).unwrap(), side);
            let run = crate::analysis_store::ArtifactRun::for_result(&target)
                .unwrap()
                .unwrap();
            assert_eq!(std::fs::read(run.join("submitted.fasta")).unwrap(), main);
            let input = crate::input_evidence::read_input(&run).unwrap();
            assert_eq!(input.submitted_hash, hash);
            assert_eq!(input.submitted_size, main.len() as u64);
            assert_eq!(input.state, "generated");
            // Match the pre-extraction sorted JSON record bytes, including the
            // newline added by write_json. Golden typed-record tests also apply.
            let record = serde_json::json!({"schema_version":1,
                "id":run.file_name().unwrap().to_str().unwrap(),
                "result":name,"status":"saved"});
            let mut expected_record = serde_json::to_vec_pretty(&record).unwrap();
            expected_record.push(b'\n');
            assert_eq!(
                std::fs::read(run.join("run.json")).unwrap(),
                expected_record
            );
            assert_eq!(files::load_delimitation(&target).unwrap(), initial);
            // Saving an already-bound result reuses its run and companion.
            assert_eq!(
                doc.write(&target, files::Format::Delimitation, dir.path())
                    .unwrap(),
                written
            );
            assert_eq!(
                crate::analysis_store::ArtifactRun::for_result(&target).unwrap(),
                Some(run)
            );
        }
    }
    #[test]
    fn companion_failure_preserves_main_and_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("partial.spart");
        let work = basecraft_io::spart::parse_text(SPART).unwrap();
        let run = crate::analysis_store::ArtifactRun::create(&target, "partial-test").unwrap();
        run.bind(&target).unwrap();
        let side = files::tree_sidecar(&target);
        std::fs::create_dir(&side).unwrap();
        assert!(
            work.write(&target, files::Format::Delimitation, dir.path())
                .is_err()
        );
        assert_eq!(
            basecraft_io::hash_file(&target).unwrap(),
            work.saved_hashes(&target).unwrap()[0]
        );
        assert!(side.is_dir());
        assert!(!run.artifact("input.json").unwrap().exists());
        assert!(!run.artifact("run.json").unwrap().exists());
    }
}
