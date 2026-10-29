//! Canonical identity boundaries: explicit links, projection, display-only labels.
use basecraft_ops::{Session, metadata, replay};
use basecraft_protocol::{
    Op,
    delimitation::{
        DelimitationEdit, DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin, Specimen,
    },
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
            ..Default::default()
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
                color: "".into(),
                members: ["r1".into(), "r2".into()].into(),
                scores: vec![],
                individual_scores: BTreeMap::new(),
                attributes: BTreeMap::new(),
            }],
            unassigned: ["r3".into()].into(),
            scores: vec![],
            provenance: Default::default(),
            attributes: BTreeMap::new(),
        }],
        view: Default::default(),
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
        s.apply(
            d,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::SetRosterLinks { links }),
            },
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
        assert!(s.metadata(m).partitions.is_empty());
        s.redo(m).unwrap();
        s.save(m, None).unwrap();
        assert_eq!(
            basecraft_docs::metadata_consumers::partitions(s.metadata(m)).len(),
            2
        );
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
