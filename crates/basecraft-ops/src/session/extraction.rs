//! Derived sequence ancestry uses the ordinary publication and input-evidence owners.
use super::{DocId, LogEntry, Path, Result, Session, files, portable_analysis_activity};
use basecraft_protocol::provenance::{ArtifactRevision, Completeness, Publication};
use std::collections::BTreeMap;

impl Session {
    pub(super) fn publish_extraction(
        &self,
        id: DocId,
        path: &Path,
        activity: &LogEntry,
    ) -> Result<()> {
        use basecraft_platform::settings::ProvenancePolicy;
        if !self.tracked || self.provenance_policy == ProvenancePolicy::None {
            return Ok(());
        }
        let run = if self.provenance_policy == ProvenancePolicy::Portable {
            Some(crate::analysis_store::ArtifactRun::create(
                path,
                &activity.id,
            )?)
        } else {
            None
        };
        let bundle = run.as_ref().map_or_else(
            || {
                self.local_provenance_root.join(
                    basecraft_io::hash_bytes(path.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                )
            },
            |r| r.directory().to_path_buf(),
        );
        std::fs::create_dir_all(&bundle)?;
        let snapshot = self.document_record(id)?;
        let source = self.saved_source(id)?;
        let (input, artifact) = if let Some((source, stamp, hash)) = source {
            let reference = basecraft_protocol::provenance::InputReference {
                path: crate::input_evidence::relative(&bundle.canonicalize()?, &source)?,
                stamp,
                hash: hash.clone(),
                extra: BTreeMap::default(),
            };
            (
                crate::input_evidence::SubmittedInput {
                    schema_version: 1,
                    source: Some(reference.clone()),
                    submitted_hash: hash.clone(),
                    submitted_size: reference.stamp.size,
                    transformation: "identity-v1".into(),
                    state: "referenced".into(),
                    analysis_run: None,
                    proposal_target: None,
                    upstream: Vec::new(),
                    origin_run: None,
                    extra: BTreeMap::default(),
                },
                ArtifactRevision {
                    id: format!("{}:source", activity.id),
                    logical_name: source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    hash,
                    size: reference.stamp.size,
                    retained: None,
                    reference: Some(reference),
                },
            )
        } else {
            // Unsaved edits need their exact current generated input. This is
            // not an automatic copy of an unchanged original.
            let mut bytes = Vec::new();
            files::write_main(self.sequence_of(id)?, &mut bytes)?;
            let hash = basecraft_io::hash_bytes(&bytes);
            basecraft_io::write_atomic(&bundle.join("submitted.fasta"), |w| {
                std::io::Write::write_all(w, &bytes)
            })?;
            (
                crate::input_evidence::SubmittedInput {
                    schema_version: 1,
                    source: None,
                    submitted_hash: hash.clone(),
                    submitted_size: bytes.len() as u64,
                    transformation: "identity-v1".into(),
                    state: "generated".into(),
                    analysis_run: None,
                    proposal_target: None,
                    upstream: Vec::new(),
                    origin_run: None,
                    extra: BTreeMap::default(),
                },
                ArtifactRevision {
                    id: format!("{}:source", activity.id),
                    logical_name: self.doc_key(id).into(),
                    hash,
                    size: bytes.len() as u64,
                    retained: Some("submitted.fasta".into()),
                    reference: None,
                },
            )
        };
        let mut input = input;
        let mut upstream = Vec::new();
        for dependency in snapshot.origin_bundles() {
            let bytes = crate::records::provenance::read_record(&dependency.join("manifest.json"))?;
            let parent: Publication = serde_json::from_slice(&bytes)?;
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &dependency.canonicalize()?,
                )?)
                .into(),
                id: parent.id.clone(),
                hash: basecraft_io::hash_bytes(&bytes),
            });
        }
        input.upstream = upstream;
        input.extra.insert("extraction".into(), serde_json::to_value(basecraft_protocol::records::ExtractionRecord { schema_version: 1, source_revision: snapshot.revision.clone(), selection: activity.op.clone(), coordinates: "zero-based-half-open; segments in biological order; reverse-complement after splicing".into(), extra: BTreeMap::new() })?);
        crate::input_evidence::write_json(&bundle.join("input.json"), &input)?;
        if let Some(run) = &run {
            crate::input_evidence::write_json(
                &run.artifact("run.json")?,
                &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
                    schema_version: 1,
                    id: run.id().into(),
                    result: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned),
                    status: basecraft_protocol::records::AnalysisRunStatus::Saved,
                    outputs: None,
                    extra: BTreeMap::new(),
                })?,
            )?;
            crate::input_evidence::register(run)?;
        }
        let (events, effective_edits) = portable_analysis_activity(&snapshot, activity);
        let hash = basecraft_io::hash_file(path)?;
        let publication = Publication {
            schema_version: 1,
            id: activity.id.clone(),
            session: self.id.clone(),
            parents: Vec::new(),
            parent_hashes: BTreeMap::default(),
            created: activity.t.clone(),
            origin: "Sequence extraction from the recorded source revision".into(),
            inputs: vec![artifact],
            outputs: vec![ArtifactRevision {
                id: format!("{}:result", activity.id),
                logical_name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash,
                size: std::fs::metadata(path)?.len(),
                retained: None,
                reference: None,
            }],
            events,
            effective_edits,
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        crate::records::provenance::publish(&bundle, &publication)?;
        if let Some(run) = &run {
            run.bind(path)?;
        }
        Ok(())
    }
}

use super::{Op, OpResult, invalid};
use crate::files::HashingWriter;

impl Session {
    /// Extract residues `start..end` with their annotations to a new
    /// GenBank file at `path`, and log it.
    pub fn extract(
        &mut self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
        path: &Path,
    ) -> Result<()> {
        self.doc(id)?;
        let _reserved = self.derived_target(path)?;
        self.track(id)?;
        self.flush()?;
        let doc = &self.sequence_of(id)?.doc;
        if start >= end || end > doc.len() {
            return Err(invalid(format!(
                "range {start}..{end} outside 0..{}",
                doc.len()
            )));
        }
        if segments.iter().any(|(a, b)| a >= b || *b > doc.len()) {
            return Err(invalid(
                "extraction segments are outside the source sequence",
            ));
        }
        // Check the accepted source before publishing any derived bytes.
        self.saved_source(id)?;
        let rec = files::extract_record(doc, start, end, segments, reverse);
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            basecraft_io::genbank::write(&mut hw, &rec)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Extract {
                start,
                end,
                path: key.clone(),
                segments: segments.to_vec(),
                reverse,
            },
            OpResult {
                hashes: vec![(key, hash)],
                feature: None,
            },
        );
        self.write(&entry)?;
        self.publish_extraction(id, path, &entry).map_err(|e| invalid(format!("extracted data saved; provenance incomplete: {e}. Keep the source session and retry to a new destination")))
    }

    /// Write a derived file, such as a figure, and log it.
    pub fn export(
        &mut self,
        id: DocId,
        path: &Path,
        format: &str,
        f: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
    ) -> Result<()> {
        let _reserved = self.derived_target(path)?;
        self.track(id)?;
        self.flush()?;
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            f(&mut hw)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Export {
                path: key.clone(),
                format: format.to_string(),
            },
            OpResult {
                hashes: vec![(key, hash)],
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// The hash of the file an extract would write, without writing it.
    pub fn extract_hash(
        &self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
    ) -> Result<String> {
        let rec = files::extract_record(&self.sequence_of(id)?.doc, start, end, segments, reverse);
        let mut hw = HashingWriter::new(std::io::sink());
        basecraft_io::genbank::write(&mut hw, &rec)?;
        Ok(hw.hash())
    }
}
