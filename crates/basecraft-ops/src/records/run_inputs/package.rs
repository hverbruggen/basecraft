//! Portable export: a copy of a result with every record and input it
//! depends on, usable without the original project.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_platform::files::lexical;
use basecraft_protocol::export_records::{ProvenanceExportMap, ProvenanceExportMapping};
use basecraft_protocol::provenance::{ArtifactRevision, Publication};

use crate::records::links::AlignmentLinkExt;

use super::relink::{ancestors, owning_ledger, verified_source};
use super::write_bytes;
use crate::{
    Error, Result,
    error::{changed_on_disk, damaged, invalid, limit, unsupported},
};

/// Explicit portable export materializes referenced dependencies; ordinary analysis does not.
pub fn package(
    result: &Path,
    destination: &Path,
    project: Option<&crate::project::Project>,
) -> Result<()> {
    let issues = crate::records::provenance::verify(result, project)?;
    if !issues.is_empty() {
        return Err(damaged(result, issues.join("; ")));
    }
    if destination.exists() {
        return Err(Error::OutputConflict(
            "package destination already exists".into(),
        ));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-package-")
        .tempdir_in(parent)?;
    let bundle = crate::records::provenance::bundle_for_result(result, project);
    let mut publication = crate::records::provenance::inspect(result, project)?;
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    let copied = staging.path().join(
        result
            .file_name()
            .ok_or_else(|| Error::NoFileName(result.into()))?,
    );
    let target_bundle = crate::records::provenance::companion(&copied);
    std::fs::create_dir_all(target_bundle.join("artifacts"))?;
    // Rebase the whole immutable record graph, not only today's inputs.
    // Original record bytes are retained as checked artifacts; relocated records
    // get new identities and recomputed parent checksums in dependency order.
    let original_id = publication.id.clone();
    let upstream = ancestors(&bundle)?;
    let mut roots = vec![(bundle.clone(), publication.clone())];
    roots.extend(upstream);
    let species =
        crate::files::Format::from_path(result).ok() == Some(crate::files::Format::Delimitation);
    let sanger = if crate::sanger_review::is_review(result) {
        Some(result.to_path_buf())
    } else if publication.origin.starts_with("Sanger accepted consensus;") {
        publication
            .inputs
            .iter()
            .find(|a| a.logical_name == "review.sanger-review.json")
            .and_then(|a| a.retained.as_ref())
            .map(|name| bundle.join(name))
    } else {
        None
    };
    let mut species_paths = BTreeMap::new();
    if species || sanger.is_some() {
        for (directory, record) in &roots {
            for artifact in record.inputs.iter().chain(&record.outputs) {
                let original = if let Some(reference) = &artifact.reference {
                    let (owner, input) = owning_ledger(directory)?;
                    let original = input.source.as_ref().map_or_else(
                        || directory.join(&reference.path),
                        |source| owner.join(&source.path),
                    );
                    Some(original)
                } else {
                    artifact.retained.as_ref().map(|name| directory.join(name))
                };
                if let Some(original) = original {
                    species_paths.insert(lexical(&original), artifact.hash.clone());
                }
            }
        }
    }
    let (mut exported, archived) = export_history(&roots, &target_bundle)?;
    publication = exported.remove(0);
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    for ancestor in exported {
        if !publication.parents.contains(&ancestor.id) {
            publication.parent_hashes.insert(
                ancestor.id.clone(),
                basecraft_io::hash_file(
                    &target_bundle
                        .join("records")
                        .join(format!("{}.json", ancestor.id)),
                )?,
            );
            publication.parents.push(ancestor.id);
        }
    }
    publication.inputs.extend(archived);
    // The packaged copy is a loose file of the package folder: its
    // companion goes in the package's own .basecraft.
    let companion = crate::files::companion_path(project, result);
    // Records the package makes for its copy sit inside the package.
    let in_package = |path: &Path| -> Result<String> {
        Ok(path
            .strip_prefix(staging.path())
            .map_err(|_| invalid("package record lies outside the package"))?
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"))
    };
    let packaged_companion = in_package(&crate::project::Project::document_record(None, &copied))?;
    for (index, artifact) in publication.outputs.iter_mut().enumerate() {
        // A companion is published by its file name, or, for a result bound
        // to a run, by its path from the result's folder.
        let beside = result
            .parent()
            .unwrap_or(Path::new("."))
            .join(&artifact.logical_name);
        let is_companion = index > 0
            && (beside == companion
                || companion
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == artifact.logical_name.as_str()));
        let (source, name) = if index == 0 {
            (
                result.to_path_buf(),
                copied
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        } else if is_companion {
            (companion.clone(), packaged_companion.clone())
        } else {
            let name = format!(
                "support-{}-{}",
                index,
                beside.file_name().unwrap_or_default().to_string_lossy()
            );
            (beside, name)
        };
        let packaged = staging.path().join(&name);
        if let Some(folder) = packaged.parent() {
            std::fs::create_dir_all(folder)?;
        }
        copy(&source, &packaged)?;
        artifact.logical_name = name;
    }
    if species {
        let mut work = crate::files::load_delimitation_in(project, result)?;
        let final_folder = parent.canonicalize()?.join(
            destination
                .file_name()
                .ok_or_else(|| Error::NoFileName(destination.into()))?,
        );
        let final_bundle = crate::records::provenance::companion(
            &final_folder.join(copied.file_name().unwrap_or_default()),
        );
        // Resolve declared paths to the checked, materialized dependency closure.
        // Immutable scientific reports stay byte-for-byte; only the derived
        // workspace metadata is relocated for use without its originals.
        crate::files::map_delimitation_paths(&mut work, |path| {
            species_paths.get(&lexical(Path::new(path))).map_or_else(
                || path.to_owned(),
                |hash| {
                    final_bundle
                        .join("artifacts")
                        .join(hash.trim_start_matches("b3:"))
                        .display()
                        .to_string()
                },
            )
        });
        work.exchange_fields
            .remove("basecraft_display_evidence_run");
        work.exchange_fields
            .remove("basecraft_alignment_evidence_run");
        for hypothesis in &mut work.hypotheses {
            if let Some(source) = hypothesis
                .provenance
                .parameters
                .get_mut("combined_source")
                .and_then(serde_json::Value::as_object_mut)
            {
                source.remove("evidence_run");
            }
        }
        let sidecar = staging.path().join(&packaged_companion);
        let target = final_folder.join(copied.file_name().unwrap_or_default());
        let companion =
            crate::files::delimitation_companion(&work, &publication.outputs[0].hash, &target)?;
        crate::files::companion_folder(&sidecar)?;
        let mut side_hash = String::new();
        basecraft_io::write_atomic(&sidecar, |w| {
            let mut hashing = crate::files::HashingWriter::new(w);
            companion.write(&mut hashing)?;
            side_hash = hashing.hash();
            Ok(())
        })?;
        if let Some(artifact) = publication
            .outputs
            .iter_mut()
            .find(|a| a.logical_name == packaged_companion)
        {
            artifact.hash = side_hash;
            artifact.size = std::fs::metadata(&sidecar)?.len();
            artifact.retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:species-context", publication.id),
                logical_name: packaged_companion.clone(),
                hash: side_hash,
                size: std::fs::metadata(&sidecar)?.len(),
                retained: None,
                reference: None,
            });
        }
    }
    if let Some(original) = &sanger {
        let review = if original == result {
            copied.clone()
        } else {
            copied.with_extension("sanger-review.json")
        };
        if original != result {
            copy(original, &review)?;
        }
        crate::sanger_review::relocate_package(original, &review, &target_bundle, &species_paths)?;
        let hash = basecraft_io::hash_file(&review)?;
        let size = std::fs::metadata(&review)?.len();
        if original == result {
            publication.outputs[0].hash = hash;
            publication.outputs[0].size = size;
            // Relocation changes the visible review, not its retained original artifact.
            publication.outputs[0].retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:sanger-review", publication.id),
                logical_name: review
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash,
                size,
                retained: None,
                reference: None,
            });
        }
    }
    // Complete tree exports materialize their exchange association, so
    // extraction still works with the original store absent.
    if let Ok(tree) = crate::files::load_tree_in(project, result)
        && crate::records::links::metadata_present(&crate::records::links::sidecar_in(
            project, result,
        ))?
    {
        let (link, input_path) =
            crate::records::links::AlignmentLink::checked_in(project, result, &tree.doc)?;
        let indexed = std::sync::Arc::new(basecraft_io::fasta::IndexedFasta::open(&input_path)?);
        let alignment = basecraft_docs::AlignmentDoc::from_fasta("Exported input", &indexed);
        // The package carries the alignment the tree is linked to, with the
        // copy's other records.
        let canonical =
            crate::project::Project::document_record_with(None, &copied, ".alignment.afa");
        crate::files::companion_folder(&canonical)?;
        basecraft_io::write_atomic(&canonical, |w| crate::files::write_alignment(&alignment, w))?;
        let mut paths = crate::records::links::AlignmentLink::create(
            &copied,
            &canonical,
            link.alignment_version,
            link.inferred,
        )?;
        paths.push(canonical);
        for path in paths {
            publication.outputs.push(ArtifactRevision {
                id: format!(
                    "{}:package-link:{}",
                    publication.id,
                    publication.outputs.len()
                ),
                // Link records live in the package's .basecraft.
                logical_name: in_package(&path)?,
                hash: basecraft_io::hash_file(&path)?,
                size: std::fs::metadata(&path)?.len(),
                retained: None,
                reference: None,
            });
        }
    }
    publication.origin = format!(
        "Portable export of revision {}. {}",
        original_id, publication.origin
    );
    crate::records::provenance::publish(&target_bundle, &publication)?;
    let issues = crate::records::provenance::verify(&copied, None)?;
    if !issues.is_empty() {
        return Err(crate::Error::Damaged {
            path: copied.clone(),
            detail: format!(
                "export verification failed: {}; original source and records preserved",
                issues.join("; ")
            ),
        });
    }
    if destination.exists() {
        return Err(Error::OutputConflict(
            "package destination appeared during export".into(),
        ));
    }
    // Publish a new directory without replacing a destination created concurrently.
    basecraft_platform::atomic::rename_noreplace(staging.path(), destination)?;
    Ok(())
}

/// Copy checked bytes into the package's content-addressed artifact directory.
fn materialize(directory: &Path, target: &Path, artifact: &mut ArtifactRevision) -> Result<()> {
    let (source, referenced) = match (&artifact.reference, &artifact.retained) {
        (Some(reference), _) => (verified_source(directory, reference)?, true),
        (None, Some(retained)) => (directory.join(retained), false),
        (None, None) => return Ok(()),
    };
    let name = format!("artifacts/{}", artifact.hash.trim_start_matches("b3:"));
    let destination = target.join(&name);
    if !destination.try_exists()? {
        copy(&source, &destination)?;
    }
    if std::fs::metadata(&destination)?.len() != artifact.size
        || basecraft_io::hash_file(&destination)? != artifact.hash
    {
        // A referenced file was verified just before: it changed meanwhile.
        return Err(if referenced {
            changed_on_disk(source)
        } else {
            damaged(source, "evidence changed during export")
        });
    }
    artifact.retained = Some(name);
    artifact.reference = None;
    Ok(())
}

type RecordKey = (PathBuf, String);
/// A publication and the bytes it was read from.
type ReadRecord = (Publication, Vec<u8>);

/// Bounded, iterative postorder traversal of each bundle's parent graph. Export
/// records are explicit relocations, while original immutable JSON stays intact
/// as a hash-checked input artifact. No source records are rewritten.
fn export_history(
    roots: &[(PathBuf, Publication)],
    target: &Path,
) -> Result<(Vec<Publication>, Vec<ArtifactRevision>)> {
    // An entry leaving the walk carries the record it read on the way in.
    let mut stack: Vec<(RecordKey, Option<ReadRecord>)> = Vec::new();
    let mut root_keys = Vec::new();
    for (directory, publication) in roots {
        let key = (directory.canonicalize()?, publication.id.clone());
        stack.push((key.clone(), None));
        root_keys.push(key);
    }
    let mut states = std::collections::HashMap::new();
    let mut order = Vec::new();
    let mut record_bytes = 0usize;
    let mut scheduled = stack.len();
    while let Some((key, leaving)) = stack.pop() {
        if let Some(record) = leaving {
            states.insert(key.clone(), 2);
            order.push((key, record));
            continue;
        }
        let path = key.0.join("records").join(format!("{}.json", key.1));
        match states.get(&key) {
            Some(1) => return Err(damaged(path, "export history contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        if key.1.is_empty()
            || !key
                .1
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(damaged(&key.0, "unsafe publication identity"));
        }
        if scheduled > 10_000 {
            return Err(limit("export history records", 10_000));
        }
        let bytes = crate::records::provenance::read_record(&path)?;
        record_bytes = record_bytes.saturating_add(bytes.len());
        if record_bytes > 128 << 20 {
            return Err(limit("export history in bytes", 128 << 20));
        }
        let record: Publication = basecraft_protocol::records::decode(&bytes)
            .map_err(|e| damaged(&path, e.to_string()))?;
        if record.schema_version != 1 {
            return Err(unsupported("unsupported export history record"));
        }
        if record.id != key.1 {
            return Err(damaged(path, "inconsistent export history record"));
        }
        states.insert(key.clone(), 1);
        let parents: Vec<_> = record
            .parents
            .iter()
            .map(|parent| ((key.0.clone(), parent.clone()), None))
            .collect();
        scheduled += parents.len();
        stack.push((key, Some((record, bytes))));
        stack.extend(parents);
    }
    let mut rebased = std::collections::HashMap::<RecordKey, Publication>::new();
    let mut hashes = std::collections::HashMap::<RecordKey, String>::new();
    let mut archives = Vec::new();
    let mut mapping = Vec::new();
    std::fs::create_dir_all(target.join("records"))?;
    for (key, (mut publication, bytes)) in order {
        let hash = basecraft_io::hash_bytes(&bytes);
        let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
        basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
        let old_id = publication.id.clone();
        publication.id = ulid::Ulid::generate().to_string();
        mapping.push(ProvenanceExportMapping {
            original_id: old_id.clone(),
            original_hash: hash.clone(),
            export_id: publication.id.clone(),
            extra: BTreeMap::new(),
        });
        archives.push(ArtifactRevision {
            id: format!("{}:original-record", publication.id),
            logical_name: format!("original-record-{old_id}.json"),
            hash,
            size: bytes.len() as u64,
            retained: Some(name),
            reference: None,
        });
        let mut parents = Vec::new();
        let mut parent_hashes = BTreeMap::new();
        for parent in &publication.parents {
            // The walk wrote every parent before its children.
            let parent_key = (key.0.clone(), parent.clone());
            let rewritten = &rebased[&parent_key];
            parents.push(rewritten.id.clone());
            parent_hashes.insert(rewritten.id.clone(), hashes[&parent_key].clone());
        }
        publication.parents = parents;
        publication.parent_hashes = parent_hashes;
        for artifact in publication
            .inputs
            .iter_mut()
            .chain(&mut publication.outputs)
        {
            materialize(&key.0, target, artifact)?;
        }
        publication.origin = format!(
            "Relocated export record of {old_id}. {}",
            publication.origin
        );
        let path = target
            .join("records")
            .join(format!("{}.json", publication.id));
        let bytes = basecraft_protocol::records::encode(&publication)?;
        write_bytes(&path, &bytes)?;
        hashes.insert(key.clone(), basecraft_io::hash_bytes(&bytes));
        rebased.insert(key, publication);
    }
    let bytes = basecraft_protocol::records::encode(&ProvenanceExportMap {
        schema_version: 1,
        records: mapping,
        extra: BTreeMap::new(),
    })?;
    let hash = basecraft_io::hash_bytes(&bytes);
    let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
    basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
    archives.push(ArtifactRevision {
        id: format!("{}:export-map", ulid::Ulid::generate()),
        logical_name: "export-map.json".into(),
        hash,
        size: bytes.len() as u64,
        retained: Some(name),
        reference: None,
    });
    let exported = root_keys.iter().map(|key| rebased[key].clone()).collect();
    Ok((exported, archives))
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    basecraft_io::write_atomic(destination, |w| {
        std::io::copy(&mut std::fs::File::open(source)?, w)?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod export_record_format_tests;
