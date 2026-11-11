#![cfg(feature = "test-heap")]
#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
#[test]
fn ten_tip_renames_on_fifty_thousand_tips_keep_less_than_one_mib() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tree.nwk");
    let tips = (0..50_000)
        .map(|i| format!("tip{i}:1"))
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(&path, format!("({tips});")).unwrap();
    let mut session = basecraft_ops::Session::new(root.path());
    let id = session.open(&path).unwrap();
    // Establish the ordinary journal baseline before measuring retained edits.
    session
        .apply(
            id,
            basecraft_protocol::Op::TreeRenameTip {
                from: "tip0".into(),
                to: "warmup".into(),
            },
        )
        .unwrap();
    session.undo(id).unwrap();
    let baseline = basecraft_io::heap::live_bytes().unwrap();
    for i in 0..10 {
        session
            .apply(
                id,
                basecraft_protocol::Op::TreeRenameTip {
                    from: format!("tip{i}"),
                    to: format!("renamed{i}"),
                },
            )
            .unwrap();
    }
    let growth = basecraft_io::heap::live_bytes()
        .unwrap()
        .saturating_sub(baseline);
    assert!(
        growth < 1024 * 1024,
        "ten rename inverses retained {growth} bytes"
    );
    for _ in 0..10 {
        session.undo(id).unwrap();
    }
    for i in 0..10 {
        assert!(
            session
                .tree(id)
                .unwrap()
                .find_tip(&format!("tip{i}"))
                .is_some()
        );
    }
}
