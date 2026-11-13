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
    session.doc(doc)?;
    let mut report = Report::default();
    // One group: a single Undo removes everything the plugin added.
    session.begin_group();
    for op in ops {
        let changes: Vec<_> = session
            .doc(doc)?
            .changes_since(from_version)
            .copied()
            .collect();
        let mapped = match op {
            Op::FeatureAdd { feature, .. } => map_feature(feature, &changes).map(Op::feature_add),
            Op::FeatureUpdate { id, feature, .. } => {
                map_feature(feature, &changes).map(|feature| Op::feature_update(id, feature))
            }
            Op::SeqReplace {
                start,
                end,
                residues,
                ..
            } => {
                let s = map_through(&changes, start, Bias::Right);
                let e = map_through(&changes, end, Bias::Left).max(s);
                Some(Op::seq_replace(s, e, residues))
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
    session.end_group();
    Ok(report)
}

fn map_feature(mut f: Feature, changes: &[basecraft_docs::Change]) -> Option<Feature> {
    for c in changes {
        f.location = map_location(&f.location, c)?;
    }
    Some(f)
}

#[cfg(test)]
mod checked_access_tests {
    use super::*;
    use basecraft_ops::Error;

    #[test]
    fn rejected_targets_do_not_open_an_undo_group() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("tree.nwk");
        std::fs::write(&tree, "(A:1,B:1);").unwrap();
        let mut session = Session::unlogged(dir.path());
        let wrong = session.open(&tree).unwrap();
        let actor = Actor::Script {
            client: "checked-access".into(),
        };
        for id in [wrong, DocId(usize::MAX)] {
            let error = apply_proposals(&mut session, id, 0, vec![], &actor).unwrap_err();
            assert!(matches!(
                error,
                crate::Error::Ops(Error::WrongKind { .. } | Error::UnknownDocument { .. })
            ));
        }
        let sequence = session.create_sequence("sequence", "ACGT").unwrap();
        for (start, residues) in [(0, "T"), (1, "G")] {
            session
                .apply(sequence, Op::seq_replace(start, start + 1, residues))
                .unwrap();
        }
        session.undo(sequence).unwrap();
        assert_eq!(session.doc(sequence).unwrap().read(0, 4), b"TCGT");
        session.undo(sequence).unwrap();
        assert_eq!(session.doc(sequence).unwrap().read(0, 4), b"ACGT");
        assert_eq!(std::fs::read(tree).unwrap(), b"(A:1,B:1);");
    }
}
