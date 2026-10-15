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
