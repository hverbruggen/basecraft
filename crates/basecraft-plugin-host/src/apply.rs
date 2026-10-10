//! Applying a plugin's proposals to the current document.
//!
//! Proposals refer to the snapshot the plugin saw. If the document changed
//! since, each proposal is mapped through the coordinate changes made after
//! the snapshot, including those of proposals applied earlier in the batch.
//! Proposals whose target no longer exists are dropped, not forced.

use basecraft_docs::coord::{Bias, map_location, map_through};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Actor, Feature, Op};

use crate::Result;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: usize,
    pub dropped: usize,
}

pub fn apply_proposals(
    session: &mut Session,
    doc: DocId,
    from_version: u64,
    ops: Vec<Op>,
    actor: &Actor,
) -> Result<Report> {
    let mut report = Report::default();
    for op in ops {
        let changes: Vec<_> = session
            .doc(doc)
            .changes_since(from_version)
            .copied()
            .collect();
        let mapped = match op {
            Op::FeatureAdd { feature } => {
                map_feature(feature, &changes).map(|feature| Op::FeatureAdd { feature })
            }
            Op::FeatureUpdate { id, feature } => {
                map_feature(feature, &changes).map(|feature| Op::FeatureUpdate { id, feature })
            }
            Op::SeqReplace {
                start,
                end,
                residues,
            } => {
                let s = map_through(&changes, start, Bias::Right);
                let e = map_through(&changes, end, Bias::Left).max(s);
                Some(Op::SeqReplace {
                    start: s,
                    end: e,
                    residues,
                })
            }
            other => Some(other),
        };
        let applied = mapped.is_some_and(|op| session.apply_as(doc, op, actor.clone()).is_ok());
        if applied {
            report.applied += 1;
        } else {
            report.dropped += 1;
        }
    }
    Ok(report)
}

fn map_feature(mut f: Feature, changes: &[basecraft_docs::Change]) -> Option<Feature> {
    for c in changes {
        f.location = map_location(&f.location, c)?;
    }
    Some(f)
}
