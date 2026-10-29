//! Project invocation evidence onto the typed edits still retained in a revision.
//! Chronology is never changed. Unknown legacy invocations acquire no invented links.
use basecraft_protocol::{LogEntry, Op, provenance::EditActivity};
use std::collections::{HashMap, HashSet};

type FeatureState<'a> = (Vec<&'a str>, Option<basecraft_protocol::Location>);

/// Both reports and publication ancestry consume this projection. A run owns
/// explicit edit identities, never a display label or the nearest historical run.
pub(crate) fn project(events: &[LogEntry], effective: &mut Vec<EditActivity>) {
    let active: HashSet<_> = effective.iter().map(|e| e.id.as_str()).collect();
    // Feature IDs are stable within a session. GenBank/GFF saves compact deleted
    // slots; the next open assigns IDs in the saved feature order. Retain owner
    // identities while rebasing those slots, using the existing coordinate map.
    let mut features: std::collections::BTreeMap<(&str, u32), FeatureState<'_>> =
        std::collections::BTreeMap::new();
    let mut sessions = HashMap::new();
    let mut saved_documents: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut supports = HashMap::new();
    for event in events {
        if matches!(event.op, Op::Save { .. })
            && let Some((_, hash)) = event.result.hashes.first()
        {
            saved_documents
                .entry(hash.as_str())
                .or_default()
                .insert(event.doc.as_str());
        }
        if let Op::Open { .. } = event.op
            && sessions.insert(event.doc.as_str(), event.session.as_str())
                != Some(event.session.as_str())
        {
            // Save As can change the document key. Rebase only through an
            // exact previously saved content hash, never actor/time proximity.
            let prior_doc = event
                .result
                .hashes
                .first()
                .and_then(|(_, hash)| saved_documents.get(hash.as_str()))
                .filter(|docs| docs.len() == 1)
                .and_then(|docs| docs.iter().next())
                .copied()
                .unwrap_or(event.doc.as_str());
            if prior_doc != event.doc {
                let moved: Vec<_> = supports
                    .iter()
                    .filter(|((doc, _), _)| *doc == prior_doc)
                    .map(|((_, name), owner)| (*name, *owner))
                    .collect();
                supports.retain(|(doc, _), _| *doc != prior_doc);
                for (name, owner) in moved {
                    supports.insert((event.doc.as_str(), name), owner);
                }
            }
            let prior: Vec<_> = features
                .iter()
                .filter(|((doc, _), _)| *doc == prior_doc)
                .map(|(_, value)| value.clone())
                .collect();
            features.retain(|(doc, _), _| *doc != prior_doc);
            if prior.len() as u64 == event.state.features {
                for (id, value) in (0u32..).zip(prior) {
                    features.insert((&event.doc, id), value);
                }
            } else {
                // Imported baseline annotations have no recorded owner.
                for id in 0..u32::try_from(event.state.features.min(100_000)).unwrap_or(0) {
                    features.insert((&event.doc, id), (Vec::new(), None));
                }
            }
        }
        if !active.contains(event.id.as_str()) {
            continue;
        }
        match &event.op {
            Op::FeatureAdd { feature } => {
                if let Some(id) = event.result.feature {
                    features.insert(
                        (&event.doc, id),
                        (vec![event.id.as_str()], Some(feature.location.clone())),
                    );
                }
            }
            Op::FeatureRemove { id } => {
                features.remove(&(event.doc.as_str(), *id));
            }
            Op::FeatureUpdate { id, feature } => {
                if let Some((owners, location)) = features.get_mut(&(event.doc.as_str(), *id)) {
                    owners.push(event.id.as_str());
                    *location = Some(feature.location.clone());
                }
            }
            Op::SeqReplace {
                start,
                end,
                residues,
            } => {
                let change = basecraft_docs::Change {
                    pos: *start,
                    deleted: end - start,
                    inserted: residues.len() as u64,
                };
                features.retain(|(doc, _), (_, location)| {
                    if *doc != event.doc {
                        return true;
                    }
                    if let Some(previous) = location {
                        *location = basecraft_docs::coord::map_location(previous, &change);
                        location.is_some()
                    } else {
                        true
                    }
                });
            }
            Op::TreeAddSupport { name, .. } => {
                supports.insert((event.doc.as_str(), name.as_str()), event.id.as_str());
            }
            Op::TreeRemoveSupport { name } => {
                supports.remove(&(event.doc.as_str(), name.as_str()));
            }
            Op::TreeStyle {
                remove_supports, ..
            } => {
                for name in remove_supports {
                    supports.remove(&(event.doc.as_str(), name.as_str()));
                }
            }
            _ => {}
        }
    }
    let survivors: HashSet<_> = features
        .values()
        .flat_map(|(owners, _)| owners.iter().copied())
        .chain(supports.values().copied())
        .chain(
            events
                .iter()
                .filter(|event| {
                    active.contains(event.id.as_str()) && matches!(event.op, Op::SeqReplace { .. })
                })
                .map(|event| event.id.as_str()),
        )
        .collect();
    let mut retained_runs = HashSet::new();
    for event in events {
        if let Op::ToolRun { params, .. } = &event.op
            && params
                .pointer("/contributions/schema_version")
                .and_then(serde_json::Value::as_u64)
                == Some(1)
            && let Some(edits) = params
                .pointer("/contributions/edits")
                .and_then(serde_json::Value::as_array)
            && edits
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|id| active.contains(id) && survivors.contains(id))
        {
            retained_runs.insert(event.id.as_str());
        }
    }
    // Recompute rather than append: a previous save may have retained this run.
    effective.retain(|edit| !matches!(&edit.operation, Op::ToolRun { params, .. } if params.get("contributions").is_some()));
    for event in events {
        if retained_runs.contains(event.id.as_str()) {
            effective.push(EditActivity {
                id: event.id.clone(),
                operation: event.op.clone(),
            });
        }
    }
}
