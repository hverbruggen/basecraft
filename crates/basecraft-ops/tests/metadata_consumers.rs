//! Canonical identity boundaries: explicit links, projection, display-only labels.
use basecraft_ops::{Session, metadata, replay};
use basecraft_protocol::{
    Op,
    delimitation::{DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin, Specimen},
    metadata::{Cell, CellChange, ChangeSet, LabelField, RecordLink},
};
use std::collections::BTreeMap;

fn setup(root: &std::path::Path) -> (Session, basecraft_ops::DocId, basecraft_ops::DocId) {
    let mut s = Session::new(root);
    let m = metadata::open(&mut s, true).unwrap().unwrap();
    metadata::apply_plan(
        &mut s,
        m,
        ChangeSet {
            specimens: vec!["roster-A".into(), "roster-B".into()],
            columns: ["genus", "specificEpithet", "accession"]
                .iter()
                .map(|n| basecraft_docs::metadata::known_column(n).unwrap().column())
                .collect(),
            cells: vec![CellChange {
                specimen: "roster-A".into(),
                column: "genus".into(),
                value: Some(Cell::Text("Caulerpa".into())),
            }],
            links: vec![
                RecordLink {
                    document: "tree.nwk".into(),
                    record: "r1".into(),
                    specimen: "roster-A".into(),
                },
                RecordLink {
                    document: "tree.nwk".into(),
                    record: "r2".into(),
                    specimen: "roster-A".into(),
                },
            ],
            ..ChangeSet::default()
        },
    )
    .unwrap();
    let work = DelimitationWorkspace {
        version: 1,
        name: "results".into(),
        specimens: ["r1", "r2", "r3"]
            .iter()
            .map(|id| Specimen {
                id: (*id).into(),
                label: (*id).into(),
                sources: BTreeMap::new(),
                attributes: BTreeMap::new(),
            })
            .collect(),
        hypotheses: vec![Hypothesis {
            id: "result".into(),
            name: "ASAP".into(),
            origin: HypothesisOrigin::Inferred,
            parent: None,
            groups: vec![Group {
                id: "g1".into(),
                label: "one".into(),
                color: String::new(),
                members: ["r1".into(), "r2".into()].into(),
                scores: vec![],
                individual_scores: BTreeMap::new(),
                attributes: BTreeMap::new(),
            }],
            unassigned: ["r3".into()].into(),
            scores: vec![],
            provenance: basecraft_protocol::delimitation::Provenance::default(),
            attributes: BTreeMap::new(),
        }],
        view: basecraft_protocol::delimitation::DelimitationView::default(),
        exchange_fields: BTreeMap::new(),
    };
    let path = root.join("result.spart");
    basecraft_io::write_atomic(&path, |w| {
        basecraft_ops::files::write_delimitation(&work, false, w)
    })
    .unwrap();
    let d = s.open(&path).unwrap();
    (s, m, d)
}

#[test]
fn explicit_roster_links_partitions_undo_replay_and_moved_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("original");
    std::fs::create_dir(&root).unwrap();
    {
        let (mut s, m, d) = setup(&root);
        let work = s.delimitation(d).clone();
        assert!(
            basecraft_docs::metadata_consumers::project_partition(
                s.metadata(m),
                &work,
                &work.hypotheses[0].id,
                "p1"
            )
            .unwrap_err()
            .0
            .contains("no stored roster link")
        );
        let local: Vec<_> = work.specimens.iter().map(|s| s.id.clone()).collect();
        let links = local
            .iter()
            .zip(["roster-A", "roster-A", "roster-B"])
            .map(|(s, r)| (s.clone(), r.into()))
            .collect();
        s.save(m, None).unwrap();
        let planned = metadata::consumer_plan(
            &s,
            m,
            &metadata::ConsumerRequest::LinkWorkspace {
                workspace: "result.spart".into(),
                links,
            },
        )
        .unwrap();
        s.apply(
            d,
            serde_json::from_value(planned["operation"].clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            s.delimitation(d)
                .specimens
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            local
        );
        s.undo(d).unwrap();
        assert_eq!(s.delimitation(d), &work);
        s.redo(d).unwrap();
        s.save(d, None).unwrap();
        let request = metadata::ConsumerRequest::PromotePartition {
            workspace: "result.spart".into(),
            hypothesis: s.delimitation(d).hypotheses[0].id.clone(),
            id: "p1".into(),
        };
        let plan = metadata::consumer_plan(&s, m, &request).unwrap();
        s.apply(
            m,
            serde_json::from_value(plan["operation"].clone()).unwrap(),
        )
        .unwrap();
        let partition = &s.metadata(m).partitions[0];
        assert_eq!(partition.groups[0].members, ["roster-A".into()].into());
        assert!(partition.unassigned.contains("roster-B"));
        assert_eq!(
            partition.provenance.inputs["project_metadata"],
            plan["metadata_hash"].as_str().unwrap()
        );
        s.undo(m).unwrap();
        assert_eq!(s.metadata(m).partitions, [] as [Hypothesis; 0]);
        s.redo(m).unwrap();
        s.save(m, None).unwrap();
        assert_eq!(
            basecraft_docs::metadata_consumers::partitions(s.metadata(m)).len(),
            2
        );
        metadata::apply_plan(
            &mut s,
            m,
            ChangeSet {
                specimens: vec!["new-specimen".into()],
                ..ChangeSet::default()
            },
        )
        .unwrap();
        assert!(
            s.metadata(m).partitions[0]
                .unassigned
                .contains("new-specimen")
        );
        s.undo(m).unwrap();
        s.apply(
            m,
            Op::MetadataEdit {
                edit: Box::new(
                    basecraft_protocol::metadata::MetadataEdit::RemovePartition { id: "p1".into() },
                ),
            },
        )
        .unwrap();
        assert_eq!(s.metadata(m).partitions, [] as [Hypothesis; 0]);
        s.undo(m).unwrap();
        s.save(m, None).unwrap();
    }
    let replayed = replay::replay(&root).unwrap();
    assert!(replayed.problems.is_empty(), "{:?}", replayed.problems);
    let moved = dir.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    let mut reopened = Session::unlogged(&moved);
    let m = metadata::open(&mut reopened, false).unwrap().unwrap();
    let d = reopened.open(&moved.join("result.spart")).unwrap();
    assert_eq!(
        reopened.metadata(m).partitions[0].groups[0].members,
        ["roster-A".into()].into()
    );
    assert_eq!(
        reopened.delimitation(d).specimens[0].attributes[basecraft_docs::delimitation::ROSTER_ID],
        "roster-A"
    );
    let moved_plan = metadata::consumer_plan(
        &reopened,
        m,
        &metadata::ConsumerRequest::PromotePartition {
            workspace: "result.spart".into(),
            hypothesis: reopened.delimitation(d).hypotheses[0].id.clone(),
            id: "after-move".into(),
        },
    )
    .unwrap();
    assert!(moved_plan["operation"].is_object());
}

#[test]
fn partial_unknown_and_conflicting_links_are_truthful() {
    let dir = tempfile::tempdir().unwrap();
    let (s, m, d) = setup(dir.path());
    let mut work = s.delimitation(d).clone();
    let source = work.hypotheses[0].id.clone();
    for (i, sp) in work.specimens.iter_mut().enumerate() {
        sp.attributes.insert(
            basecraft_docs::delimitation::ROSTER_ID.into(),
            if i < 2 { "roster-A" } else { "roster-B" }.into(),
        );
    }
    work.specimens[2].attributes.insert(
        basecraft_docs::delimitation::ROSTER_ID.into(),
        "roster-A".into(),
    );
    assert!(
        basecraft_docs::metadata_consumers::project_partition(s.metadata(m), &work, &source, "p")
            .unwrap_err()
            .0
            .contains("assigned and unassigned")
    );
    work.specimens[2].attributes.insert(
        basecraft_docs::delimitation::ROSTER_ID.into(),
        "roster-B".into(),
    );
    let mut second = work.hypotheses[0].groups[0].clone();
    let moved = second.members.iter().next().unwrap().clone();
    second.id = "second".into();
    second.members = [moved.clone()].into();
    work.hypotheses[0].groups[0].members.remove(&moved);
    work.hypotheses[0].groups.push(second);
    assert!(
        basecraft_docs::metadata_consumers::project_partition(s.metadata(m), &work, &source, "p")
            .unwrap_err()
            .0
            .contains("conflicting source groups")
    );
    work.specimens[2].attributes.insert(
        basecraft_docs::delimitation::ROSTER_ID.into(),
        "unknown".into(),
    );
    assert!(
        basecraft_docs::metadata_consumers::project_partition(s.metadata(m), &work, &source, "p")
            .unwrap_err()
            .0
            .contains("unknown roster ID")
    );
}

#[test]
fn composed_labels_skip_empty_fields_and_keep_scientific_ids() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, m, _) = setup(dir.path());
    std::fs::write(dir.path().join("tree.nwk"), "(r1:1,r2:2,unknown:3);").unwrap();
    let t = s.open(&dir.path().join("tree.nwk")).unwrap();
    let records = vec!["r1".into(), "r2".into(), "unknown".into()];
    let labels = basecraft_docs::metadata_consumers::compose_labels(
        s.metadata(m),
        "tree.nwk",
        &records,
        &[
            LabelField::Genus,
            LabelField::Species,
            LabelField::Specimen,
            LabelField::Accession,
        ],
    );
    assert_eq!(labels[0].label, "Caulerpa roster-A");
    assert_eq!(labels[1].specimen.as_deref(), Some("roster-A"));
    assert_eq!(labels[2].label, "unknown");
    assert!(labels[2].issue.is_some());
    let duplicates = basecraft_docs::metadata_consumers::compose_labels(
        s.metadata(m),
        "tree.nwk",
        &["r1".into(), "r1".into()],
        &[LabelField::Specimen],
    );
    assert!(
        duplicates
            .iter()
            .all(|l| l.issue.is_some() && l.label == "r1")
    );
    let mut style = s.tree(t).style().clone();
    style
        .tip_labels
        .insert("r1".into(), labels[0].label.clone());
    s.apply(
        t,
        Op::TreeStyle {
            style,
            remove_supports: vec![],
        },
    )
    .unwrap();
    s.save(t, None).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("tree.nwk"))
            .unwrap()
            .contains("r1:")
    );
    assert!(s.tree(t).nodes().iter().any(|n| n.name == "r1"));
    s.undo(t).unwrap();
    assert!(s.tree(t).style().tip_labels.is_empty());
    s.redo(t).unwrap();
    assert_eq!(s.tree(t).style().tip_labels["r1"], "Caulerpa roster-A");
    let mut check = Session::unlogged(dir.path());
    let tree = check.open(&dir.path().join("tree.nwk")).unwrap();
    assert_eq!(
        check.tree(tree).style().tip_labels["r1"],
        "Caulerpa roster-A"
    );
}

#[test]
fn copied_consumers_reject_another_metadata_owner_with_identical_ids() {
    let first = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let (mut a, am, aw) = setup(first.path());
    let (mut b, bm, _) = setup(other.path());
    a.save(am, None).unwrap();
    b.save(bm, None).unwrap();
    let links = BTreeMap::from([
        ("r1".into(), "roster-A".into()),
        ("r2".into(), "roster-A".into()),
        ("r3".into(), "roster-B".into()),
    ]);
    let plan = metadata::consumer_plan(
        &a,
        am,
        &metadata::ConsumerRequest::LinkWorkspace {
            workspace: "result.spart".into(),
            links,
        },
    )
    .unwrap();
    a.apply(
        aw,
        serde_json::from_value(plan["operation"].clone()).unwrap(),
    )
    .unwrap();
    a.save(aw, None).unwrap();
    let source = first.path().join("result.spart");
    let copied = other.path().join("copied.spart");
    std::fs::copy(&source, &copied).unwrap();
    std::fs::copy(
        basecraft_ops::files::tree_sidecar(&source),
        basecraft_ops::files::tree_sidecar(&copied),
    )
    .unwrap();
    let promote = metadata::ConsumerRequest::PromotePartition {
        workspace: "copied.spart".into(),
        hypothesis: a.delimitation(aw).hypotheses[0].id.clone(),
        id: "p".into(),
    };
    assert!(
        metadata::consumer_plan(&b, bm, &promote)
            .unwrap_err()
            .to_string()
            .contains("different project metadata")
    );

    let tree_path = first.path().join("tree.nwk");
    std::fs::write(&tree_path, "(r1:1,r2:2);").unwrap();
    let tree = a.open(&tree_path).unwrap();
    let request = metadata::ConsumerRequest::Labels {
        document: "tree.nwk".into(),
        records: vec!["r1".into(), "r2".into()],
        fields: vec![LabelField::Genus, LabelField::Specimen],
    };
    let snapshot = metadata::consumer_plan(&a, am, &request).unwrap();
    let mut style = a.tree(tree).style().clone();
    style.tip_labels.insert(
        "r1".into(),
        snapshot["labels"][0]["label"].as_str().unwrap().into(),
    );
    style
        .extra
        .insert("metadata_labels".into(), snapshot.clone());
    a.apply(
        tree,
        Op::TreeStyle {
            style,
            remove_supports: vec![],
        },
    )
    .unwrap();
    a.save(tree, None).unwrap();
    let copied_tree = other.path().join("tree.nwk");
    std::fs::copy(&tree_path, &copied_tree).unwrap();
    std::fs::copy(
        basecraft_ops::files::tree_sidecar(&tree_path),
        basecraft_ops::files::tree_sidecar(&copied_tree),
    )
    .unwrap();
    let t = b.open(&copied_tree).unwrap();
    // Passive display preserves the saved snapshot even in a different project.
    assert_eq!(b.tree(t).style().tip_labels["r1"], "Caulerpa roster-A");
    // Explicit refresh refuses equal IDs from a different owner.
    assert!(
        metadata::consumer_plan(&b, bm, &request)
            .unwrap_err()
            .to_string()
            .contains("different project metadata")
    );
    // Normal metadata edits preserve ownership; plans refresh but do not mutate labels.
    metadata::apply_plan(
        &mut a,
        am,
        ChangeSet {
            cells: vec![CellChange {
                specimen: "roster-A".into(),
                column: "genus".into(),
                value: Some(Cell::Text("Codium".into())),
            }],
            ..ChangeSet::default()
        },
    )
    .unwrap();
    a.save(am, None).unwrap();
    let refresh = metadata::consumer_plan(&a, am, &request).unwrap();
    assert_eq!(refresh["labels"][0]["label"], "Codium roster-A");
    assert_ne!(refresh["metadata_hash"], snapshot["metadata_hash"]);
    assert_eq!(a.tree(tree).style().tip_labels["r1"], "Caulerpa roster-A");
    // Accepted typed external TSV edits do not silently change the saved snapshot.
    let tsv = first.path().join("metadata/specimens.tsv");
    let external = std::fs::read_to_string(&tsv)
        .unwrap()
        .replace("Codium", "Ulva");
    std::fs::write(&tsv, external).unwrap();
    let mut reader = Session::unlogged(first.path());
    let m = metadata::open(&mut reader, false).unwrap().unwrap();
    assert_ne!(reader.metadata_notices(m), [] as [String; 0]);
    let external_plan = metadata::consumer_plan(&reader, m, &request).unwrap();
    assert_eq!(external_plan["labels"][0]["label"], "Ulva roster-A");
    assert_eq!(a.tree(tree).style().tip_labels["r1"], "Caulerpa roster-A");
}
