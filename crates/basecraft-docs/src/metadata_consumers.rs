//! Consumers of the canonical roster; no name matching or identity rewriting.
use crate::alignment::EditError;
use basecraft_protocol::{
    delimitation::{DelimitationWorkspace, Hypothesis},
    metadata::{Cell, ComposedLabel, LabelField, ProjectMetadata},
};
use std::collections::{BTreeMap, BTreeSet};

/// Current recorded identifications followed by saved delimitation partitions.
pub fn partitions(meta: &ProjectMetadata) -> Vec<Hypothesis> {
    std::iter::once(super::metadata::identification_partition(meta))
        .chain(meta.partitions.iter().cloned())
        .collect()
}

/// Validate using the existing partition invariants, with the project roster.
pub fn validate_partitions(meta: &ProjectMetadata) -> Result<(), EditError> {
    if meta.partitions.iter().any(|p| p.id == "identifications") {
        return Err(EditError(
            "identifications is reserved for recorded identifications".into(),
        ));
    }
    crate::delimitation::validate(&DelimitationWorkspace {
        version: 1,
        name: "Project partitions".into(),
        specimens: meta
            .specimens
            .rows
            .iter()
            .map(|s| basecraft_protocol::delimitation::Specimen {
                id: s.id.clone(),
                label: s.id.clone(),
                sources: BTreeMap::new(),
                attributes: BTreeMap::new(),
            })
            .collect(),
        hypotheses: meta.partitions.clone(),
        view: Default::default(),
        exchange_fields: BTreeMap::new(),
    })
}

/// Project a result using explicitly stored workspace -> roster links. Reject
/// partial links and conflicting assignments rather than merging biological units.
/// Source scores stay in the exact source hypothesis: per-record fitted scores
/// cannot be silently turned into per-specimen scores for many-to-one links.
pub fn project_partition(
    meta: &ProjectMetadata,
    work: &DelimitationWorkspace,
    source: &str,
    id: &str,
) -> Result<Hypothesis, EditError> {
    crate::delimitation::validate(work)?;
    super::metadata::validate(meta)?;
    let original = work
        .hypotheses
        .iter()
        .find(|h| h.id == source)
        .ok_or_else(|| EditError("unknown source hypothesis".into()))?;
    let roster: BTreeSet<_> = meta.specimens.rows.iter().map(|s| s.id.as_str()).collect();
    let mut links = BTreeMap::new();
    for s in &work.specimens {
        let target = s
            .attributes
            .get(crate::delimitation::ROSTER_ID)
            .ok_or_else(|| {
                EditError(format!(
                    "workspace specimen {} has no stored roster link",
                    s.id
                ))
            })?;
        if !roster.contains(target.as_str()) {
            return Err(EditError(format!(
                "workspace specimen {} links to unknown roster ID {target}",
                s.id
            )));
        }
        links.insert(s.id.as_str(), target.clone());
    }
    let mut result = original.clone();
    result.id = id.into();
    result.parent = None;
    result.scores.clear();
    result.provenance.haplotypes.clear();
    result.provenance.parameters.insert(
        "source_hypothesis".into(),
        serde_json::to_value(original).map_err(|e| EditError(e.to_string()))?,
    );
    result.provenance.parameters.insert(
        "roster_links".into(),
        serde_json::to_value(&links).map_err(|e| EditError(e.to_string()))?,
    );
    let mut assignments: BTreeMap<String, Option<String>> = BTreeMap::new();
    for group in &original.groups {
        for member in &group.members {
            let target = &links[member.as_str()];
            let assignment = Some(group.id.clone());
            if let Some(previous) = assignments.insert(target.clone(), assignment.clone()) {
                if previous != assignment {
                    return Err(EditError(format!(
                        "roster specimen {target} belongs to conflicting source groups"
                    )));
                }
            }
        }
    }
    for member in &original.unassigned {
        let target = &links[member.as_str()];
        if assignments
            .insert(target.clone(), None)
            .is_some_and(|previous| previous.is_some())
        {
            return Err(EditError(format!(
                "roster specimen {target} has both assigned and unassigned records"
            )));
        }
    }
    for group in &mut result.groups {
        group.members = assignments
            .iter()
            .filter(|(_, g)| g.as_deref() == Some(group.id.as_str()))
            .map(|(s, _)| s.clone())
            .collect();
        group.scores.clear();
        group.individual_scores.clear();
    }
    result.unassigned = roster
        .into_iter()
        .filter(|s| assignments.get(*s).is_none_or(Option::is_none))
        .map(str::to_string)
        .collect();
    let mut check = meta.clone();
    check.partitions.retain(|p| p.id != id);
    check.partitions.push(result.clone());
    validate_partitions(&check)?;
    Ok(result)
}

/// Compose labels from stored cells only. Missing fields are skipped; empty,
/// missing or ambiguous links fall back to the unchanged record identity.
pub fn compose_labels(
    meta: &ProjectMetadata,
    document: &str,
    records: &[String],
    fields: &[LabelField],
) -> Vec<ComposedLabel> {
    let mut counts = BTreeMap::new();
    for record in records {
        *counts.entry(record).or_insert(0usize) += 1;
    }
    records
        .iter()
        .map(|record| {
            let matching: Vec<_> = meta
                .links
                .iter()
                .filter(|l| l.document == document && &l.record == record)
                .collect();
            let mut out = ComposedLabel {
                record: record.clone(),
                specimen: None,
                label: record.clone(),
                issue: None,
            };
            if counts[record] > 1 || matching.len() > 1 {
                out.issue = Some("ambiguous record identity or stored link".into());
                return out;
            }
            let Some(link) = matching.first() else {
                out.issue = Some("record has no stored specimen link".into());
                return out;
            };
            let Some(row) = meta.specimens.rows.iter().find(|s| s.id == link.specimen) else {
                out.issue = Some("stored link refers to an unknown specimen".into());
                return out;
            };
            out.specimen = Some(row.id.clone());
            let parts: Vec<_> = fields
                .iter()
                .filter_map(|field| {
                    let name = match field {
                        LabelField::Specimen => return Some(row.id.clone()),
                        LabelField::Genus => "genus",
                        LabelField::Species => "specificEpithet",
                        LabelField::Accession => "accession",
                        LabelField::Column { name } => name,
                    };
                    row.cells
                        .get(name)
                        .filter(|c| !matches!(c, Cell::Missing(_)))
                        .map(super::metadata::format_cell)
                        .filter(|v| !v.trim().is_empty())
                })
                .collect();
            if !parts.is_empty() {
                out.label = parts.join(" ");
            }
            out
        })
        .collect()
}
