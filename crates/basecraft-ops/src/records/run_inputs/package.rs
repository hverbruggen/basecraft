//! Portable export: a copy of a result with every record and input it
//! depends on, usable without the original project.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_platform::files::lexical;
use basecraft_protocol::export_records::{ProvenanceExportMap, ProvenanceExportMapping};
use basecraft_protocol::provenance::{ArtifactRevision, Publication};

use crate::records::links::AlignmentLinkExt;

use super::relink::{ancestors, owning_input, verified_source};
use super::write_bytes;
use crate::{
    Error, Result,
    error::{changed_on_disk, damaged, invalid, limit, unsupported},
};

/// Write a portable copy of `result` to the new folder `destination`: the
/// result, its records and every input they name, so it can be checked and
/// reused without the original project. Ordinary analysis copies nothing.
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
    result_first(&mut publication, result);
    let copied = staging.path().join(
        result
            .file_name()
            .ok_or_else(|| Error::NoFileName(result.into()))?,
    );
    let package = Package {
        result,
        project,
        staging: staging.path(),
        bundle: crate::records::provenance::companion(&copied),
        companion: in_package(
            staging.path(),
            &crate::project::Project::document_record(None, &copied),
        )?,
        final_copy: parent
            .canonicalize()?
            .join(
                destination
                    .file_name()
                    .ok_or_else(|| Error::NoFileName(destination.into()))?,
            )
            .join(copied.file_name().unwrap_or_default()),
        copied,
    };
    std::fs::create_dir_all(package.bundle.join("artifacts"))?;
    // Rebase the whole immutable record graph, not only today's inputs.
    // Original record bytes are retained as checked artifacts; relocated records
    // get new identities and recomputed parent checksums in dependency order.
    let original_id = publication.id.clone();
    let mut roots = vec![(bundle.clone(), publication.clone())];
    roots.extend(ancestors(&bundle)?);
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
    let originals = if species || sanger.is_some() {
        original_hashes(&roots)?
    } else {
        BTreeMap::new()
    };
    let (mut exported, archived) = export_history(&roots, &package.bundle)?;
    publication = exported.remove(0);
    result_first(&mut publication, result);
    for ancestor in exported {
        if !publication.parents.contains(&ancestor.id) {
            publication.parent_hashes.insert(
                ancestor.id.clone(),
                basecraft_io::hash_file(
                    &package
                        .bundle
                        .join("records")
                        .join(format!("{}.json", ancestor.id)),
                )?,
            );
            publication.parents.push(ancestor.id);
        }
    }
    publication.inputs.extend(archived);
    package.copy_outputs(&mut publication)?;
    if species {
        package.species_workspace(&mut publication, &originals)?;
    }
    if let Some(original) = &sanger {
        package.sanger_review(&mut publication, original, &originals)?;
    }
    package.tree_alignment(&mut publication)?;
    publication.origin = format!(
        "Portable export of revision {}. {}",
        original_id, publication.origin
    );
    crate::records::provenance::publish(&package.bundle, &publication)?;
    let issues = crate::records::provenance::verify(&package.copied, None)?;
    if !issues.is_empty() {
        return Err(crate::Error::Damaged {
            path: package.copied.clone(),
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

/// One package being written in its staging folder.
struct Package<'a> {
    result: &'a Path,
    project: Option<&'a crate::project::Project>,
    staging: &'a Path,
    /// The copy of the result in the staging folder.
    copied: PathBuf,
    /// Where the copy will be once the staging folder is renamed.
    final_copy: PathBuf,
    /// The history bundle of the copy.
    bundle: PathBuf,
    /// The copy's document record, by its path in the package.
    companion: String,
}

/// Put the output that is `result` first.
fn result_first(publication: &mut Publication, result: &Path) {
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
}

/// `path`'s place in the package folder `staging`, with `/` between parts.
fn in_package(staging: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(staging)
        .map_err(|_| invalid("package record lies outside the package"))?
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

/// The hash of every original input and retained file the records in
/// `roots` name, by its lexical path: the package keeps each under its hash.
fn original_hashes(roots: &[(PathBuf, Publication)]) -> Result<BTreeMap<PathBuf, String>> {
    let mut originals = BTreeMap::new();
    for (directory, record) in roots {
        for artifact in record.inputs.iter().chain(&record.outputs) {
            let original = if let Some(reference) = &artifact.reference {
                let (owner, input) = owning_input(directory)?;
                Some(input.source.as_ref().map_or_else(
                    || directory.join(&reference.path),
                    |source| owner.join(&source.path),
                ))
            } else {
                artifact.retained.as_ref().map(|name| directory.join(name))
            };
            if let Some(original) = original {
                originals.insert(lexical(&original), artifact.hash.clone());
            }
        }
    }
    Ok(originals)
}

impl Package<'_> {
    /// Copy the result and the files saved with it into the package, and
    /// name them there.
    fn copy_outputs(&self, publication: &mut Publication) -> Result<()> {
        // The packaged copy is a loose file of the package folder: its
        // companion goes in the package's own .basecraft.
        let companion = crate::files::companion_path(self.project, self.result);
        for (index, artifact) in publication.outputs.iter_mut().enumerate() {
            // A companion is published by its file name, or, for a result bound
            // to a run, by its path from the result's folder.
            let beside = self
                .result
                .parent()
                .unwrap_or(Path::new("."))
                .join(&artifact.logical_name);
            let is_companion = index > 0
                && (beside == companion
                    || companion.file_name().is_some_and(|name| {
                        name.to_string_lossy() == artifact.logical_name.as_str()
                    }));
            let (source, name) = if index == 0 {
                (
                    self.result.to_path_buf(),
                    self.copied
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                )
            } else if is_companion {
                (companion.clone(), self.companion.clone())
            } else {
                let name = format!(
                    "support-{}-{}",
                    index,
                    beside.file_name().unwrap_or_default().to_string_lossy()
                );
                (beside, name)
            };
            let packaged = self.staging.join(&name);
            if let Some(folder) = packaged.parent() {
                std::fs::create_dir_all(folder)?;
            }
            copy(&source, &packaged)?;
            artifact.logical_name = name;
        }
        Ok(())
    }

    /// Point a species workspace's paths at the package's copies of its
    /// inputs and write its companion there. Its scientific reports keep
    /// their bytes; only the workspace's own paths change.
    fn species_workspace(
        &self,
        publication: &mut Publication,
        originals: &BTreeMap<PathBuf, String>,
    ) -> Result<()> {
        let mut work = crate::files::load_delimitation_in(self.project, self.result)?;
        let final_bundle = crate::records::provenance::companion(&self.final_copy);
        crate::files::map_delimitation_paths(&mut work, |path| {
            originals.get(&lexical(Path::new(path))).map_or_else(
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
        let sidecar = self.staging.join(&self.companion);
        let companion = crate::files::delimitation_companion(
            &work,
            &publication.outputs[0].hash,
            &self.final_copy,
        )?;
        crate::files::companion_folder(&sidecar)?;
        let mut side_hash = String::new();
        basecraft_io::write_atomic(&sidecar, |w| {
            let mut hashing = crate::files::HashingWriter::new(w);
            companion.write(&mut hashing)?;
            side_hash = hashing.hash();
            Ok(())
        })?;
        let size = std::fs::metadata(&sidecar)?.len();
        if let Some(artifact) = publication
            .outputs
            .iter_mut()
            .find(|a| a.logical_name == self.companion)
        {
            artifact.hash = side_hash;
            artifact.size = size;
            artifact.retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:species-context", publication.id),
                logical_name: self.companion.clone(),
                hash: side_hash,
                size,
                retained: None,
                reference: None,
            });
        }
        Ok(())
    }

    /// Copy the Sanger review `original` (the result itself, or the review a
    /// consensus came from) and point it at the package's copies of its
    /// traces.
    fn sanger_review(
        &self,
        publication: &mut Publication,
        original: &Path,
        originals: &BTreeMap<PathBuf, String>,
    ) -> Result<()> {
        let is_result = original == self.result;
        let review = if is_result {
            self.copied.clone()
        } else {
            self.copied.with_extension("sanger-review.json")
        };
        if !is_result {
            copy(original, &review)?;
        }
        crate::sanger_review::relocate_package(original, &review, &self.bundle, originals)?;
        let hash = basecraft_io::hash_file(&review)?;
        let size = std::fs::metadata(&review)?.len();
        if is_result {
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
        Ok(())
    }

    /// A tree linked to an alignment takes a copy of that alignment and a
    /// link to it, so extraction still works without the original store.
    fn tree_alignment(&self, publication: &mut Publication) -> Result<()> {
        let Ok(tree) = crate::files::load_tree_in(self.project, self.result) else {
            return Ok(());
        };
        if !crate::records::links::metadata_present(&crate::records::links::sidecar_in(
            self.project,
            self.result,
        ))? {
            return Ok(());
        }
        let (link, input_path) =
            crate::records::links::AlignmentLink::checked_in(self.project, self.result, &tree.doc)?;
        let indexed = std::sync::Arc::new(basecraft_io::fasta::IndexedFasta::open(&input_path)?);
        let alignment = basecraft_docs::AlignmentDoc::from_fasta("Exported input", &indexed);
        let canonical =
            crate::project::Project::document_record_with(None, &self.copied, ".alignment.afa");
        crate::files::companion_folder(&canonical)?;
        basecraft_io::write_atomic(&canonical, |w| crate::files::write_alignment(&alignment, w))?;
        let mut paths = crate::records::links::AlignmentLink::create(
            &self.copied,
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
                logical_name: in_package(self.staging, &path)?,
                hash: basecraft_io::hash_file(&path)?,
                size: std::fs::metadata(&path)?.len(),
                retained: None,
                reference: None,
            });
        }
        Ok(())
    }
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
