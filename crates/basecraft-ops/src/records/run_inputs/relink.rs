//! Find a run's original input, follow a run's recorded ancestry, and link a
//! run to an original that moved.
use std::path::{Path, PathBuf};

use basecraft_protocol::input_records::EvidenceState;
use basecraft_protocol::provenance::{InputReference, Publication};

use super::{SubmittedInput, lock, read_input, relative, stamp, write_input};
use crate::{
    Result,
    error::{changed_on_disk, damaged, invalid, limit, unsupported},
};

/// Follow only the bounded Save As delegation, never a source file or cache.
pub(super) fn owning_ledger(bundle: &Path) -> Result<(PathBuf, SubmittedInput)> {
    let mut location = bundle.canonicalize()?;
    let mut input = read_input(&location)?;
    let mut visited = std::collections::HashSet::from([location.clone()]);
    for _ in 0..32 {
        let Some(origin) = &input.origin_run else {
            break;
        };
        location = location.join(origin).canonicalize()?;
        if !visited.insert(location.clone()) {
            return Err(damaged(location, "input evidence contains a cycle"));
        }
        input = read_input(&location)?;
    }
    if input.origin_run.is_some() {
        return Err(limit("input evidence ancestry", 32));
    }
    Ok((location, input))
}

/// Resolve evidence only on explicit verification/export/relink. No UI-event hashing.
pub fn verified_source(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let path = source_location(bundle, reference)?;
    if basecraft_io::hash_file(&path)? != reference.hash
        || std::fs::metadata(&path)?.len() != reference.stamp.size
    {
        return Err(changed_on_disk(path));
    }
    Ok(path.canonicalize()?)
}

pub(crate) fn source_location(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let (mut location, mut input) = owning_ledger(bundle)?;
    if input.state == EvidenceState::Broken {
        return Err(damaged(location, "evidence chain explicitly broken"));
    }
    let primary_matches = input.source.as_ref().is_some_and(|source| {
        source.hash == reference.hash && source.stamp.size == reference.stamp.size
    });
    // Multi-input publications retain one ordinary ledger per original. Follow
    // only declared, bounded, hash-checked ancestry, with no filesystem scan.
    let mut matching = None;
    for (directory, publication) in ancestors(bundle)? {
        let declared = publication.inputs.iter().any(|artifact| {
            artifact.hash == reference.hash
                && artifact.size == reference.stamp.size
                && artifact.reference.as_ref().is_some_and(|source| {
                    source.hash == reference.hash && source.stamp.size == reference.stamp.size
                })
        });
        if !declared {
            continue;
        }
        let (owner, candidate) = owning_ledger(&directory)?;
        if candidate.source.as_ref().is_some_and(|source| {
            source.hash == reference.hash && source.stamp.size == reference.stamp.size
        }) {
            // Check every declared matching owner before selecting one: an
            // intact same-hash sibling must not hide an explicit evidence break.
            if candidate.state == EvidenceState::Broken {
                return Err(damaged(owner, "evidence chain explicitly broken"));
            }
            if matching.is_none() {
                matching = Some((owner, candidate));
            }
        }
    }
    if !primary_matches && let Some((owner, candidate)) = matching {
        location = owner;
        input = candidate;
    }
    let effective = input
        .source
        .as_ref()
        .ok_or_else(|| damaged(&location, "reference evidence is missing"))?;
    if effective.hash != reference.hash {
        return Err(damaged(location, "input reference identity changed"));
    }
    let path = if input.state == EvidenceState::Preserved {
        location.join("preserved-input")
    } else {
        location.join(&effective.path)
    };
    Ok(path)
}

/// Honest repair: only a byte-verified original may replace a missing/changed reference.
pub fn relink(bundle: &Path, original: &Path) -> Result<()> {
    let locks = lock()?;
    locks.source(original)?;
    let local = read_input(bundle)?;
    let (owner, mut input) = owning_ledger(bundle)?;
    if local.source.as_ref().map(|r| &r.hash) != input.source.as_ref().map(|r| &r.hash) {
        return Err(damaged(owner, "delegated input reference identity changed"));
    }
    let reference = input
        .source
        .as_mut()
        .ok_or_else(|| unsupported("generated input has no disk reference"))?;
    let original = original.canonicalize()?;
    if basecraft_io::hash_file(&original)? != reference.hash
        || std::fs::metadata(&original)?.len() != reference.stamp.size
    {
        return Err(invalid("selected file is not the analysed original"));
    }
    reference.path = relative(&owner, &original)?;
    reference.stamp = stamp(&original)?;
    let hash = reference.hash.clone();
    input.state = "referenced".into();
    write_input(&owner, &input)?;
    // A registered run keeps one reference ledger per original beside the run.
    // Relink each ledger in this result's declared ancestry that names the same
    // bytes; an explicitly broken ledger stays broken.
    for (directory, _) in ancestors(bundle)? {
        let directory = directory.canonicalize()?;
        if directory == owner || !directory.join("input.json").is_file() {
            continue;
        }
        let mut ledger = read_input(&directory)?;
        if ledger.state == EvidenceState::Broken || ledger.origin_run.is_some() {
            continue;
        }
        let Some(source) = ledger.source.as_mut() else {
            continue;
        };
        if source.hash != hash {
            continue;
        }
        source.path = relative(&directory, &original)?;
        source.stamp = stamp(&original)?;
        ledger.state = "referenced".into();
        write_input(&directory, &ledger)?;
    }
    Ok(())
}

/// Walk only declared immutable ancestor records. No ancestor input is copied.
/// The mutable evidence ledger stays in its owning run so later preservation or
/// explicit evidence-break decisions remain visible to descendants.
pub(crate) fn ancestors(bundle: &Path) -> Result<Vec<(PathBuf, Publication)>> {
    let mut stack = vec![(bundle.to_path_buf(), false)];
    let mut states = std::collections::HashMap::new();
    let mut seen_records = std::collections::HashSet::new();
    let mut records = Vec::new();
    let mut bytes_read = 0usize;
    while let Some((location, leaving)) = stack.pop() {
        let location = location.canonicalize()?;
        if leaving {
            states.insert(location, 2);
            continue;
        }
        match states.get(&location) {
            Some(1) => return Err(damaged(location, "evidence ancestry contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        states.insert(location.clone(), 1);
        stack.push((location.clone(), true));
        if !location.join("input.json").is_file() {
            continue;
        }
        let input = read_input(&location)?;
        if let Some(origin) = &input.origin_run {
            stack.push((location.join(origin), false));
        }
        for reference in &input.upstream {
            if records.len() + stack.len() >= 10_000 {
                return Err(limit("evidence ancestry", 10_000));
            }
            let directory = location.join(&reference.bundle).canonicalize()?;
            let id = &reference.id;
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(damaged(
                    location.join("input.json"),
                    "unsafe ancestor identity",
                ));
            }
            stack.push((directory.clone(), false));
            if !seen_records.insert((directory.clone(), id.to_owned())) {
                continue;
            }
            let path = directory.join("records").join(format!("{id}.json"));
            let bytes = crate::records::provenance::read_record(&path)?;
            bytes_read = bytes_read.saturating_add(bytes.len());
            if bytes_read > 128 << 20 {
                return Err(limit("evidence ancestry in bytes", 128 << 20));
            }
            if reference.hash != basecraft_io::hash_bytes(&bytes) {
                return Err(damaged(path, "ancestor record hash differs"));
            }
            let publication: Publication = basecraft_protocol::records::decode(&bytes)
                .map_err(|e| damaged(&path, e.to_string()))?;
            if publication.schema_version != 1 {
                return Err(unsupported("unsupported ancestor record"));
            }
            if &publication.id != id {
                return Err(damaged(path, "inconsistent ancestor record"));
            }
            records.push((directory, publication));
        }
    }
    Ok(records)
}

/// Verify the immutable upstream records using the shared bounded ancestry walk.
pub fn verify_ancestors(bundle: &Path) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    for (directory, publication) in ancestors(bundle)? {
        issues.extend(crate::records::provenance::verify_records(
            &directory,
            &publication,
        )?);
    }
    Ok(issues)
}
