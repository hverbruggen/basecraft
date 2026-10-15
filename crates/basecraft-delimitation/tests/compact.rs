use basecraft_delimitation::haplotypes::{deduplicate, expand};
use std::collections::BTreeSet;

#[test]
fn compact_haplotype_mapping_expands_every_specimen() {
    let text =
        include_str!("../../../testdata/fixtures/compact/species-delimitation/specimens.afa");
    let rows: Vec<_> = text
        .split('>')
        .skip(1)
        .map(|record| {
            let mut lines = record.lines();
            (lines.next().unwrap().to_string(), lines.collect::<String>())
        })
        .collect();
    let haplotypes = deduplicate(&rows).unwrap();
    assert_eq!(haplotypes.original_count, 8);
    assert_eq!(haplotypes.unique.len(), 6);
    assert_eq!(
        haplotypes.members["specimen_01"],
        BTreeSet::from(["specimen_01".into(), "specimen_02".into()])
    );
    assert_eq!(
        haplotypes.members["specimen_07"],
        BTreeSet::from(["specimen_07".into(), "specimen_08".into()])
    );
    let groups = vec![haplotypes.members.keys().cloned().collect()];
    let expanded = expand(&groups, &haplotypes.members).unwrap();
    assert_eq!(expanded[0], rows.iter().map(|(n, _)| n.clone()).collect());
    let mut duplicated = rows.clone();
    duplicated[1].0.clone_from(&rows[0].0);
    assert!(deduplicate(&duplicated).is_err());
}

#[test]
fn compact_substitution_fixture_passes_rooted_method_validation() {
    let text =
        include_str!("../../../testdata/fixtures/compact/species-delimitation/substitutions.nwk");
    let tree = basecraft_docs::TreeDoc::from_newick("substitutions", text).unwrap();
    let tips = basecraft_delimitation::time_tree::substitution_tips(&tree, true).unwrap();
    let expected: BTreeSet<_> = (1..=8).map(|i| format!("specimen_{i:02}")).collect();
    assert_eq!(tips.keys().cloned().collect::<BTreeSet<_>>(), expected);
    // Rooting and units are assertions supplied by the workflow, not Newick metadata.
    assert!(basecraft_delimitation::time_tree::substitution_tips(&tree, false).is_err());
    assert!(basecraft_delimitation::time_tree::validate(&tree, true, 1e-12, true).is_err());
    let polytomy = basecraft_docs::TreeDoc::from_newick(
        "polytomy",
        include_str!("../../../testdata/fixtures/compact/species-delimitation/time.nwk"),
    )
    .unwrap();
    assert!(basecraft_delimitation::time_tree::substitution_tips(&polytomy, true).is_err());
}
