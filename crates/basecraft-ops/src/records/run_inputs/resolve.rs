//! Carry out the user's answer to a save over a file that analyses used:
//! keep a copy of the original, remove the analyses, or mark them broken.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_protocol::input_records::InputState;
use basecraft_protocol::provenance::InputReference;

use super::check::{AffectedAnalysis, InputConflict, SavePolicy, same_source};
use super::relink::owning_input;
use super::{lock, read_input, relative, write_input};
use crate::{
    Error, Result,
    error::{changed_on_disk, damaged, invalid, unsupported},
};

/// Retain one owned input record before a temporary source disappears. Uses
/// the same checked, idempotent preserve-copy transition as a document save,
/// without touching other analyses that happen to reference the same file.
pub fn preserve_input(bundle: &Path) -> Result<()> {
    let _locks = lock()?;
    let input = read_input(bundle)?;
    let reference = input
        .source
        .as_ref()
        .ok_or_else(|| unsupported("input has no source reference"))?;
    let target = bundle.join(&reference.path);
    resolve(
        &InputConflict {
            target,
            analyses: vec![AffectedAnalysis {
                run: bundle.into(),
                result: "Retained operation input".into(),
                source_hash: reference.hash.clone(),
                can_preserve: true,
                direct: true,
                backing_analyses: vec![],
            }],
            options: vec![SavePolicy::PreserveCopy],
        },
        SavePolicy::PreserveCopy,
    )
}

/// Resolve a group under the caller's existing mutation guard. Two target
/// groups can contain the very same backing input record. Removal of that exact
/// input record is performed once; equal hashes/names never identify two input records.
/// The original groups remain intact in the Session receipt and action report.
pub(crate) fn resolve_group(
    conflict: &InputConflict,
    policy: SavePolicy,
    removed: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    if policy != SavePolicy::RemoveAnalysis {
        return resolve(conflict, policy);
    }
    let mut remaining = conflict.clone();
    remaining.analyses.retain_mut(|analysis| {
        if analysis.backing_analyses.is_empty() {
            return !removed.contains(&analysis.run);
        }
        analysis
            .backing_analyses
            .retain(|backing| !removed.contains(&backing.run));
        !analysis.backing_analyses.is_empty()
    });
    resolve(&remaining, policy)?;
    for analysis in remaining.analyses {
        if analysis.backing_analyses.is_empty() {
            removed.insert(analysis.run);
        } else {
            removed.extend(
                analysis
                    .backing_analyses
                    .into_iter()
                    .map(|backing| backing.run),
            );
        }
    }
    Ok(())
}

/// Apply explicit decisions before overwrite. A partial decision failure aborts
/// the save; earlier preserved copies/removed metadata remain recoverable.
pub fn resolve(conflict: &InputConflict, policy: SavePolicy) -> Result<()> {
    let _locks = lock()?;
    if conflict.analyses.is_empty() {
        return Ok(());
    }
    if policy == SavePolicy::Cancel {
        return Err(crate::Error::InputConflict(Box::new(conflict.clone())));
    }
    if !conflict.options.contains(&policy) {
        return Err(unsupported(
            "old analysed bytes are unavailable; preserve-copy cannot repair this input",
        ));
    }
    for analysis in &conflict.analyses {
        let logical_owner = &analysis.run;
        let mut preserved: BTreeMap<String, PathBuf> = BTreeMap::new();
        let backing = if analysis.backing_analyses.is_empty() {
            std::slice::from_ref(analysis)
        } else {
            &analysis.backing_analyses
        };
        let mut backing: Vec<_> = backing.iter().collect();
        if policy == SavePolicy::PreserveCopy {
            // A leaf input record can own shared bytes without a cycle. The
            // logical run already depends on that input record; delegate toward the
            // leaf, never from it back to its parent analysis.
            backing.sort_by_key(|analysis| (!analysis.direct, &analysis.run == logical_owner));
        }
        if policy == SavePolicy::RemoveAnalysis {
            // A parent workflow contains its child stores. Move the children
            // first, retaining their trash inside the recoverable parent tree.
            backing.sort_by_key(|analysis| std::cmp::Reverse(analysis.run.components().count()));
        }
        for affected in backing {
            let mut input = read_input(&affected.run)?;
            match policy {
                SavePolicy::PreserveCopy => {
                    if !affected.direct {
                        continue;
                    }
                    let outdated = |what: &str| Error::Outdated(what.into());
                    let reference = input
                        .source
                        .as_ref()
                        .ok_or_else(|| outdated("reference is missing"))?;
                    if reference.hash != affected.source_hash {
                        return Err(outdated("stale input preservation decision"));
                    }
                    let destination = affected.run.join("preserved-input");
                    // A public retry must never touch the live source or replace
                    // retained bytes. The owning input record may have advanced already.
                    if input.state == InputState::Preserved {
                        let (owner, retained) = owning_input(&affected.run)?;
                        let effective = retained
                            .source
                            .as_ref()
                            .ok_or_else(|| damaged(&owner, "reference is missing"))?;
                        verify_preserved(&owner.join("preserved-input"), effective)?;
                        preserved.insert(reference.hash.clone(), owner);
                        continue;
                    }
                    if input.state != InputState::Referenced {
                        return Err(outdated(
                            "input evidence changed; refresh the save decision",
                        ));
                    }
                    let source = affected.run.join(&reference.path);
                    if !same_source(&affected.run, reference, &conflict.target) {
                        return Err(outdated("input reference moved; refresh the save decision"));
                    }
                    if let Some(owner) = preserved.get(&reference.hash) {
                        verify_preserved(&owner.join("preserved-input"), reference)?;
                        input.origin_run = Some(relative(&affected.run, owner)?);
                        input.state = InputState::Preserved;
                        input
                            .extra
                            .insert("preserved_original_bytes".into(), serde_json::json!(0));
                        write_input(&affected.run, &input)?;
                        continue;
                    }
                    let copied = if destination.try_exists()? {
                        // A previous input record write may have failed after publication.
                        verify_preserved(&destination, reference)?;
                        0
                    } else {
                        let mut candidate = tempfile::NamedTempFile::new_in(&affected.run)?;
                        let mut hashing = crate::files::HashingWriter::new(candidate.as_file_mut());
                        let copied =
                            std::io::copy(&mut std::fs::File::open(&source)?, &mut hashing)?;
                        if hashing.hash() != reference.hash || copied != reference.stamp.size {
                            return Err(changed_on_disk(source));
                        }
                        candidate.as_file().sync_all()?;
                        candidate
                            .persist_noclobber(&destination)
                            .map_err(|e| e.error)?;
                        basecraft_platform::files::sync_directory(&affected.run)?;
                        copied
                    };
                    preserved.insert(reference.hash.clone(), affected.run.clone());
                    input.state = InputState::Preserved;
                    input
                        .extra
                        .insert("preserved_original_bytes".into(), serde_json::json!(copied));
                    write_input(&affected.run, &input)?;
                }
                SavePolicy::BreakEvidence => {
                    input.state = InputState::Broken;
                    write_input(&affected.run, &input)?;
                }
                SavePolicy::RemoveAnalysis => {
                    let store = affected
                        .run
                        .parent()
                        .and_then(Path::parent)
                        .ok_or_else(|| invalid("run has no store"))?;
                    let trash = store.join("trash");
                    if !trash.exists() {
                        std::fs::create_dir(&trash)?;
                    }
                    if std::fs::symlink_metadata(&trash)?.file_type().is_symlink() {
                        return Err(damaged(trash, "unsafe analysis trash"));
                    }
                    let destination = trash.join(format!(
                        "{}-{}",
                        affected
                            .run
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        ulid::Ulid::generate()
                    ));
                    std::fs::rename(&affected.run, destination)?;
                    basecraft_platform::files::sync_directory(affected.run.parent().unwrap())?;
                    basecraft_platform::files::sync_directory(&trash)?;
                    basecraft_platform::files::sync_directory(store)?;
                }
                SavePolicy::Cancel => return Err(Error::Cancelled("save cancelled".into())),
            }
        }
    }
    Ok(())
}

fn verify_preserved(path: &Path, reference: &InputReference) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != reference.stamp.size
        || basecraft_io::hash_file(path)? != reference.hash
    {
        return Err(damaged(
            path,
            "retained original is damaged; existing bytes preserved",
        ));
    }
    Ok(())
}
