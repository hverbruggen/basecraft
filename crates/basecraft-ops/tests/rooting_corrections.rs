//! SCI-0003 correction policies supplement the unchanged independent probes.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{DocId, Session, files};
use basecraft_protocol::Op;

fn methods() -> [Op; 2] {
    [Op::TreeMidpointRoot {}, Op::TreeMinimumVarianceRoot {}]
}

#[test]
fn unrepresentable_tip_support_is_refused_before_recovery_tracking() {
    crate::isolated_profile::isolate!();
    for text in [
        "(A:1,(B:10,C:1)90:1);",
        "((B:10,C:1)90:1,A:1);",
        "(A:1,(B:10,C:1)90/0.8:1);",
    ] {
        for op in methods() {
            let dir = tempfile::tempdir().unwrap();
            let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
                (key == "BASECRAFT_PROFILE_DIR")
                    .then(|| dir.path().join("profile").into_os_string())
            })
            .unwrap();
            let path = dir.path().join("supported.nwk");
            std::fs::write(&path, text).unwrap();
            let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
            let id = session.open(&path).unwrap();
            let before = session.tree(id).unwrap().clone();
            let revision = session.record_revision(id).unwrap();
            let error = session.apply(id, op).unwrap_err().to_string();
            assert!(error.contains("support on a tip"), "{error}");
            assert_eq!(session.tree(id).unwrap().nodes(), before.nodes());
            assert_eq!(session.tree(id).unwrap().supports(), before.supports());
            assert_eq!(
                session.tree(id).unwrap().annotations(),
                before.annotations()
            );
            assert_eq!(session.tree(id).unwrap().version(), before.version());
            assert_eq!(session.record_revision(id).unwrap(), revision);
            assert!(!session.can_undo(id).unwrap());
            assert!(!session.can_redo(id).unwrap());
            assert!(!session.recovery_root().exists());
            assert!(!files::tree_sidecar(&path).exists());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }
}

#[test]
fn tip_support_refusal_preserves_existing_history_and_redo() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("supported.nwk");
    std::fs::write(&path, "(A:1,(B:10,C:1)90:1);").unwrap();
    let mut session = Session::new(dir.path());
    let id = session.open(&path).unwrap();
    session
        .apply(
            id,
            Op::TreeRotate {
                clade: vec!["B".into(), "C".into()],
            },
        )
        .unwrap();
    session.undo(id).unwrap();
    let before = session.tree(id).unwrap().to_newick();
    let revision = session.record_revision(id).unwrap();
    for op in methods() {
        assert!(
            session
                .apply(id, op)
                .unwrap_err()
                .to_string()
                .contains("support on a tip")
        );
        assert_eq!(session.tree(id).unwrap().to_newick(), before);
        assert_eq!(session.record_revision(id).unwrap(), revision);
        assert!(!session.can_undo(id).unwrap());
        assert!(session.can_redo(id).unwrap());
    }
    session.redo(id).unwrap();
    session.undo(id).unwrap();
    assert_eq!(session.tree(id).unwrap().to_newick(), before);
}

#[test]
fn automatic_root_checks_out_of_range_ids_in_populated_session() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "(A:1,B:2,C:3);").unwrap();
    let mut session = Session::new(dir.path());
    let valid = session.open(&path).unwrap();
    let revision = session.record_revision(valid).unwrap();
    for id in [DocId(1), DocId(usize::MAX)] {
        for op in methods() {
            let error = session.apply(id, op).unwrap_err().to_string();
            assert!(error.contains("unknown document"), "{error}");
            assert_eq!(session.record_revision(valid).unwrap(), revision);
            assert!(!session.can_undo(valid).unwrap());
        }
    }
}
