//! Versions of host document serializers. Bump the affected format whenever its
//! bytes change, and add a golden for that version without deleting old goldens.
use std::{collections::BTreeMap, path::Path};

pub fn for_path(path: &Path) -> BTreeMap<String, u32> {
    use crate::files::Format;
    let formats: &[(&str, u32)] = match Format::from_path(path) {
        Ok(Format::Fasta) => &[("fasta", 1), ("gff3", 1)],
        Ok(Format::GenBank) => &[("genbank", 1)],
        Ok(Format::Tree) => &[("newick", 1), ("tree-sidecar", 1), ("alignment-link", 1)],
        Ok(Format::Delimitation) => &[("spart", 1), ("delimitation-sidecar", 1)],
        Ok(Format::Metadata) => &[("metadata-manifest", 1), ("metadata-tsv", 1)],
        Ok(Format::Submission) => &[("submission", 1)],
        Err(_) => &[],
    };
    formats
        .iter()
        .map(|(name, version)| ((*name).to_owned(), *version))
        .collect()
}
