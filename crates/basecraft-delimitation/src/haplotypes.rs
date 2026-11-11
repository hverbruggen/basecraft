//! Exact aligned DNA identity, with no pairwise-deletion inference or weights.
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

pub const POLICY: &str = "aligned-dna-case-u-to-t-literal-gap-ambiguity-v1";
pub use basecraft_protocol::producer_records::Haplotypes;

/// Representatives are the lexicographically smallest stable specimen IDs.
/// `BTreeMap` compares entire byte strings, so hash collisions cannot merge rows.
pub fn deduplicate(rows: &[(String, String)]) -> Result<Haplotypes> {
    let width = rows.first().map_or(0, |(_, s)| s.len());
    if rows.is_empty() || width == 0 {
        return Err(Error("deduplication requires a nonempty alignment".into()));
    }
    let mut ids = BTreeSet::new();
    let mut grouped = BTreeMap::<String, BTreeSet<String>>::new();
    let mut canonical = BTreeMap::new();
    for (id, sequence) in rows {
        if id.is_empty() || !ids.insert(id.clone()) {
            return Err(Error("duplicate or empty specimen ID".into()));
        }
        if sequence.len() != width {
            return Err(Error("alignment is not rectangular".into()));
        }
        let sequence = sequence.to_ascii_uppercase().replace('U', "T");
        if !sequence.bytes().all(|b| b"ACGTRYSWKMBDHVN?-.".contains(&b)) {
            return Err(Error("invalid aligned DNA symbol".into()));
        }
        canonical.insert(id, sequence.clone());
        grouped.entry(sequence).or_default().insert(id.clone());
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(POLICY.as_bytes());
    for (id, sequence) in canonical {
        hasher.update(&(id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
        hasher.update(&(sequence.len() as u64).to_le_bytes());
        hasher.update(sequence.as_bytes());
    }
    let mut unique = Vec::new();
    let mut members = BTreeMap::new();
    for (sequence, ids) in grouped {
        let representative = ids.first().unwrap().clone();
        unique.push((representative.clone(), sequence));
        members.insert(representative, ids);
    }
    unique.sort();
    Ok(Haplotypes {
        policy: POLICY.into(),
        input_hash: format!("b3:{}", hasher.finalize()),
        original_count: rows.len(),
        unique,
        members,
    })
}

pub fn expand(
    groups: &[BTreeSet<String>],
    mapping: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<BTreeSet<String>>> {
    let mut used = BTreeSet::new();
    let mut originals = BTreeSet::new();
    for (representative, members) in mapping {
        if !members.contains(representative) {
            return Err(Error(
                "representative mapping must contain its own specimen ID".into(),
            ));
        }
        for id in members {
            if !originals.insert(id) {
                return Err(Error("overlapping specimen mappings".into()));
            }
        }
    }
    let expanded = groups
        .iter()
        .map(|group| {
            let mut result = BTreeSet::new();
            for id in group {
                if !used.insert(id) {
                    return Err(Error("representative appears in multiple groups".into()));
                }
                let members = mapping
                    .get(id)
                    .ok_or_else(|| Error(format!("unmapped tip {id}")))?;
                result.extend(members.clone());
            }
            Ok(result)
        })
        .collect::<Result<Vec<_>>>()?;
    if used.len() != mapping.len() {
        return Err(Error(
            "partition does not cover every representative".into(),
        ));
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_identity_is_stable_and_keeps_near_haplotypes() {
        let mut rows = vec![
            ("b".into(), "auN-".into()),
            ("a".into(), "ATN-".into()),
            ("c".into(), "AT?-".into()),
            ("d".into(), "ATN.".into()),
        ];
        let first = deduplicate(&rows).unwrap();
        rows.reverse();
        assert_eq!(deduplicate(&rows).unwrap(), first);
        assert_eq!(first.unique.len(), 3);
        assert_eq!(first.members["a"], ["a".into(), "b".into()].into());
        let expanded = expand(
            &[["a".into(), "c".into()].into(), ["d".into()].into()],
            &first.members,
        )
        .unwrap();
        assert_eq!(expanded.iter().map(BTreeSet::len).sum::<usize>(), 4);
    }
    #[test]
    fn expansion_rejects_incomplete_overlapping_and_nonreversible_mappings() {
        let mapping = [
            ("a".into(), ["a".into(), "b".into()].into()),
            ("c".into(), ["c".into()].into()),
        ]
        .into();
        assert!(expand(&[["a".into()].into()], &mapping).is_err());
        assert!(
            expand(
                &[["a".into(), "c".into()].into(), ["a".into()].into()],
                &mapping
            )
            .is_err()
        );
        let invalid = [("a".into(), ["b".into()].into())].into();
        assert!(expand(&[["a".into()].into()], &invalid).is_err());
        let overlapping = [
            ("a".into(), ["a".into(), "b".into()].into()),
            ("c".into(), ["b".into(), "c".into()].into()),
        ]
        .into();
        assert!(expand(&[["a".into(), "c".into()].into()], &overlapping).is_err());
    }
    #[test]
    fn pairwise_deletion_zero_does_not_define_identity() {
        let rows = vec![
            ("a".into(), "A-".into()),
            ("b".into(), "-C".into()),
            ("c".into(), "AC".into()),
        ];
        assert_eq!(deduplicate(&rows).unwrap().unique.len(), 3);
        assert!(deduplicate(&[("a".into(), "AC".into()), ("a".into(), "AC".into())]).is_err());
    }
}
