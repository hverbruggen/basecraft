//! Project invocation evidence onto the typed edits still retained in a revision.
//! Chronology is never changed. Unknown legacy invocations acquire no invented links.
use basecraft_protocol::{LogEntry, Op, provenance::EditActivity};
use std::collections::{HashMap, HashSet};

/// Both reports and publication ancestry consume this projection. A run owns
/// explicit edit identities, never a display label or the nearest historical run.
pub(crate) fn project(events: &[LogEntry], effective: &mut Vec<EditActivity>) {
    let event_map: HashMap<_, _> = events.iter().map(|e| (e.id.as_str(), e)).collect();
    let active: HashSet<_> = effective.iter().map(|e| e.id.as_str()).collect();
    let mut features = HashMap::new();
    let mut supports = HashMap::new();
    for edit in effective.iter() {
        let Some(event) = event_map.get(edit.id.as_str()) else {
            continue;
        };
        match &edit.operation {
            Op::FeatureAdd { .. } => {
                if let Some(id) = event.result.feature {
                    features.insert((event.session.as_str(), id), edit.id.as_str());
                }
            }
            Op::FeatureRemove { id } => {
                features.remove(&(event.session.as_str(), *id));
            }
            Op::TreeAddSupport { name, .. } => {
                supports.insert(name.as_str(), edit.id.as_str());
            }
            Op::TreeRemoveSupport { name } => {
                supports.remove(name.as_str());
            }
            Op::TreeStyle {
                remove_supports, ..
            } => {
                for name in remove_supports {
                    supports.remove(name.as_str());
                }
            }
            _ => {}
        }
    }
    let survivors: HashSet<_> = features
        .values()
        .chain(supports.values())
        .copied()
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
