//! Support formatting is presentation; scientific strings and inference input survive edits.
use basecraft_ops::records::AlignmentLinkExt;
use basecraft_ops::{Session, files, records::AlignmentLink};
use basecraft_protocol::{Op, SupportKind};
use serde_json::json;

#[test]
fn support_precision_scale_unknown_settings_save_undo_and_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)0.654321/0.987654321,c);").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    let original = session.tree(doc).unwrap().to_newick();
    let mut style = session.tree(doc).unwrap().style().clone();
    style.support_decimals.insert("Support 1".into(), 0);
    style.support_decimals.insert("Support 2".into(), 4);
    style.support_proportions.insert("Support 1".into(), false);
    style.support_fraction.insert("Support 2".into(), true);
    style.support_minimums.insert("Support 1".into(), 80);
    style.support_legend = false;
    style
        .extra
        .insert("future_option".into(), json!({"nested": [1,2,3]}));
    session
        .apply(
            doc,
            Op::TreeStyle {
                style: style.clone(),
                remove_supports: Vec::new(),
            },
        )
        .unwrap();
    let node = session
        .tree(doc)
        .unwrap()
        .resolve(&["a".into(), "b".into()])
        .unwrap();
    assert_eq!(
        session.tree(doc).unwrap().supports()[0]
            .formatted(node, &style)
            .unwrap(),
        "1"
    );
    assert_eq!(
        session.tree(doc).unwrap().supports()[1]
            .formatted(node, &style)
            .unwrap(),
        "0.9877"
    );
    assert_eq!(
        session.tree(doc).unwrap().supports()[0].display_percent(node, &style),
        Some(0.654_321)
    );
    assert_eq!(session.tree(doc).unwrap().to_newick(), original);
    session.undo(doc).unwrap();
    assert!(
        session
            .tree(doc)
            .unwrap()
            .style()
            .support_decimals
            .is_empty()
    );
    session.redo(doc).unwrap();
    assert_eq!(session.tree(doc).unwrap().style(), &style);
    session.save(doc, None).unwrap();
    let reopened = files::load_tree(&path).unwrap().doc;
    assert_eq!(reopened.style(), &style);
    assert_eq!(reopened.to_newick(), original);
    assert!(basecraft_ops::records::replay(dir.path()).unwrap().ok());
}

#[test]
fn support_kind_survives_current_sidecar() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)0.5,c);").unwrap();
    let mut tree = files::load_tree(&path).unwrap().doc;
    assert_eq!(tree.supports()[0].kind, SupportKind::Unknown);
    tree.set_support_kind("Support", SupportKind::Bootstrap);
    files::save_tree_companion(None, &path, &tree).unwrap();
    let reopened = files::load_tree(&path).unwrap().doc;
    assert_eq!(reopened.supports()[0].kind, SupportKind::Bootstrap);
    assert!(!reopened.supports()[0].proportions);
}

#[test]
fn changing_view_link_preserves_origin_and_save_as_rebases_both_links() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    let tree_path = root.join("tree.nwk");
    std::fs::write(&source, ">a\nAC\n>b\nAT\n").unwrap();
    std::fs::write(&other, ">a\nCC\n>b\nCT\n").unwrap();
    std::fs::write(&tree_path, "(a,b);").unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&tree_path).unwrap();
    AlignmentLink::create(&tree_path, &source, 7, true).unwrap();
    let origin_bytes = std::fs::read(basecraft_ops::records::sidecar(&tree_path)).unwrap();
    let origin = AlignmentLink::read(&tree_path).unwrap();
    AlignmentLink::associate(&tree_path, session.tree(doc).unwrap(), &other).unwrap();
    let view = AlignmentLink::read_view(&tree_path).unwrap();
    assert_eq!(view.source, other);
    assert!(!view.inferred);
    assert_eq!(
        origin_bytes,
        std::fs::read(basecraft_ops::records::sidecar(&tree_path)).unwrap()
    );
    let invalid = root.join("invalid.afa");
    std::fs::write(&invalid, ">a\nAC\n>x\nAT\n").unwrap();
    assert!(AlignmentLink::associate(&tree_path, session.tree(doc).unwrap(), &invalid).is_err());
    assert_eq!(AlignmentLink::read_view(&tree_path).unwrap().source, other);
    std::fs::create_dir(root.join("copies")).unwrap();
    let output = root.join("copies/copy.nwk");
    session.save(doc, Some(&output)).unwrap();
    let copied_origin = AlignmentLink::read(&output).unwrap();
    let copied_view = AlignmentLink::read_view(&output).unwrap();
    assert_eq!(copied_origin.source, source);
    assert_eq!(copied_origin.source_hash, origin.source_hash);
    assert_eq!(copied_origin.alignment_version, 7);
    assert_eq!(copied_view.source, other);
    assert!(
        copied_origin
            .checked_alignment(None, &output, session.tree(doc).unwrap())
            .is_ok()
    );
    assert!(
        copied_view
            .checked_alignment(None, &output, session.tree(doc).unwrap())
            .is_ok()
    );
    // No copy is kept: a removed file is missing, a rewritten one changed.
    std::fs::remove_file(&other).unwrap();
    assert!(
        copied_view
            .status(None, &output, session.tree(doc).unwrap())
            .contains("missing or moved")
    );
    assert!(
        copied_view
            .checked_alignment(None, &output, session.tree(doc).unwrap())
            .is_err()
    );
    std::fs::write(&source, ">a\nGG\n>b\nGT\n").unwrap();
    assert!(
        copied_origin
            .status(None, &output, session.tree(doc).unwrap())
            .contains("changed since it was linked")
    );
    assert!(basecraft_ops::records::replay(root).unwrap().ok());
}

#[test]
fn portable_links_and_corrupt_view_never_fall_back_to_scientific_input() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let source = folder.join("source.afa");
    let tree_path = folder.join("tree.nwk");
    std::fs::write(&source, ">a\nAC\n>b\nAT\n").unwrap();
    std::fs::write(&tree_path, "(a,b);").unwrap();
    let tree = files::load_tree(&tree_path).unwrap().doc;
    AlignmentLink::create(&tree_path, &source, 0, true).unwrap();
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(basecraft_ops::records::sidecar(&tree_path)).unwrap(),
    )
    .unwrap();
    assert_eq!(value["source"], "source.afa");
    let moved = dir.path().join("moved");
    std::fs::rename(folder, &moved).unwrap();
    let tree_path = moved.join("tree.nwk");
    let link = AlignmentLink::read(&tree_path).unwrap();
    assert_eq!(link.source, moved.join("source.afa"));
    assert!(!link.source_changed());
    assert!(link.checked_alignment(None, &tree_path, &tree).is_ok());
    std::fs::write(basecraft_ops::records::view_sidecar(&tree_path), "damaged").unwrap();
    assert!(AlignmentLink::read_view(&tree_path).is_err());
    assert!(AlignmentLink::read(&tree_path).is_ok());
}

#[test]
fn staged_support_removal_and_style_are_one_reversible_replayable_edit() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)90.123/0.987,c);").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    let original = session.tree(doc).unwrap().to_newick();
    let mut style = session.tree(doc).unwrap().style().clone();
    style.support_decimals.insert("Support 2".into(), 4);
    session
        .apply(
            doc,
            Op::TreeStyle {
                style: style.clone(),
                remove_supports: vec!["Support 1".into()],
            },
        )
        .unwrap();
    assert_eq!(session.tree(doc).unwrap().supports().len(), 1);
    assert_eq!(session.tree(doc).unwrap().supports()[0].name, "Support 2");
    assert_eq!(session.tree(doc).unwrap().style(), &style);
    session.undo(doc).unwrap();
    assert_eq!(session.tree(doc).unwrap().to_newick(), original);
    assert!(
        session
            .tree(doc)
            .unwrap()
            .style()
            .support_decimals
            .is_empty()
    );
    session.redo(doc).unwrap();
    session.save(doc, None).unwrap();
    let reopened = files::load_tree(&path).unwrap().doc;
    assert_eq!(reopened.supports().len(), 1);
    assert_eq!(reopened.supports()[0].name, "Support 2");
    assert_eq!(reopened.style(), &style);
    assert!(basecraft_ops::records::replay(dir.path()).unwrap().ok());
}

#[test]
fn invalid_style_and_removal_do_not_mutate_or_start_recovery_tracking() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)90.123/0.987,c);").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    let original = session.tree(doc).unwrap().to_newick();
    let version = session.tree(doc).unwrap().version();
    let style = session.tree(doc).unwrap().style().clone();
    assert!(
        session
            .apply(
                doc,
                Op::TreeStyle {
                    style: style.clone(),
                    remove_supports: vec!["Support 1".into(), "missing".into()]
                }
            )
            .is_err()
    );
    assert_eq!(session.tree(doc).unwrap().to_newick(), original);
    assert_eq!(session.tree(doc).unwrap().version(), version);
    assert!(!dir.path().join(".basecraft").exists());
    let mut invalid = style.clone();
    invalid.support_decimals.insert("Support 1".into(), 255);
    assert!(
        session
            .apply(
                doc,
                Op::TreeStyle {
                    style: invalid,
                    remove_supports: Vec::new()
                }
            )
            .is_err()
    );
    assert_eq!(session.tree(doc).unwrap().style(), &style);
    assert!(!dir.path().join(".basecraft").exists());
}

#[test]
fn drawing_edits_replay_in_order_and_skip_stale_clades() {
    use basecraft_protocol::delimitation::TreeDrawingEdit;
    crate::isolated_profile::isolate!();
    let mut tree = basecraft_docs::TreeDoc::from_newick("t", "((a,b),(c,(d,e)));").unwrap();
    let mut style = tree.style().clone();
    style.tip_font = 21;
    basecraft_ops::session::apply_drawing_edits(
        &mut tree,
        &[
            TreeDrawingEdit::Ladderize { small_first: false },
            TreeDrawingEdit::Rotate {
                clade: vec!["d".into(), "e".into()],
            },
            // Not in this tree: skipped, the rest still apply.
            TreeDrawingEdit::Rotate {
                clade: vec!["x".into()],
            },
            TreeDrawingEdit::Style { style },
        ],
    );
    let order: Vec<_> = tree
        .tips_in_order()
        .iter()
        .map(|&i| tree.node(i).name.clone())
        .collect();
    assert_eq!(order, ["e", "d", "c", "a", "b"]);
    assert_eq!(tree.style().tip_font, 21);
}

#[test]
fn imported_supports_retain_recorded_names_kinds_and_evidence() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inferred.nwk");
    let text = "((a,b)95/0.9,c);";
    std::fs::write(&path, text).unwrap();
    let mut source = files::load_tree(&path).unwrap().doc;
    source.rename_support("Support 1", "Standard bootstrap");
    source.rename_support("Support 2", "Posterior probability");
    source.set_support_kind("Standard bootstrap", SupportKind::Bootstrap);
    source.set_support_kind("Posterior probability", SupportKind::Probability);
    files::save_tree_companion(None, &path, &source).unwrap();
    let known = files::load_tree(&path).unwrap().doc;
    assert_eq!(known.supports()[0].name, "Standard bootstrap");
    assert_eq!(known.supports()[1].name, "Posterior probability");
    assert_eq!(known.supports()[0].kind, SupportKind::Bootstrap);
    assert_eq!(known.supports()[1].kind, SupportKind::Probability);
    let target = basecraft_docs::TreeDoc::from_newick("target", "(c,(b,a));").unwrap();
    let matched = target.support_from(&known).unwrap();
    assert_eq!(matched[0].0, "Standard bootstrap");
    assert_eq!(matched[1].0, "Posterior probability");
    std::fs::remove_file(files::companion_path(None, &path)).unwrap();
    let unknown = files::load_tree(&path).unwrap().doc;
    assert_eq!(unknown.supports()[0].name, "Support 1");
    assert_eq!(unknown.supports()[0].kind, SupportKind::Unknown);
}

#[test]
fn unreadable_metadata_never_blocks_the_exchange_tree() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    let original = b"((a,b)0.5,c);\n";
    std::fs::write(&path, original).unwrap();
    let tree = files::load_tree(&path).unwrap().doc;
    let mut bytes = Vec::new();
    let hash = basecraft_io::hash_bytes(original);
    files::tree_companion(&tree, &hash)
        .write(&mut bytes)
        .unwrap();
    let mut side: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let sidecar = files::companion_path(None, &path);
    std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
    for version in [0, 99] {
        side["version"] = version.into();
        std::fs::write(&sidecar, serde_json::to_vec(&side).unwrap()).unwrap();
        let mut session = Session::unlogged(dir.path());
        let id = session.open(&path).unwrap();
        assert_eq!(session.tree(id).unwrap().to_newick(), tree.to_newick());
        assert!(session.tree_metadata_warning(id).unwrap().is_some());
    }
    for bytes in [b"broken JSON".as_slice(), b"{}".as_slice()] {
        std::fs::write(&sidecar, bytes).unwrap();
        let mut session = Session::unlogged(dir.path());
        let id = session.open(&path).unwrap();
        assert!(session.tree_metadata_warning(id).unwrap().is_some());
    }
    std::fs::remove_file(&sidecar).unwrap();
    std::fs::create_dir(&sidecar).unwrap();
    let mut session = Session::unlogged(dir.path());
    let id = session.open(&path).unwrap();
    assert!(session.tree_metadata_warning(id).unwrap().is_some());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
