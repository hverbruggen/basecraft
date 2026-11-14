//! FASTA classification the way it was decided before streaming, from the
//! index. Tests compare the production classifier against it.
use basecraft_io::fasta;
use basecraft_ops::files::{self, Kind, LONGEST_ALIGNMENT_ROW};
use std::path::Path;

pub fn indexed_kind(path: &Path) -> basecraft_ops::Result<Kind> {
    let file = fasta::IndexedFasta::open(path)?;
    let named = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("afa"))
        || path
            .file_stem()
            .and_then(|s| Path::new(s).extension())
            .is_some_and(|e| e.eq_ignore_ascii_case("aln"));
    let rows = file.index.records.len() >= 2
        && file
            .index
            .records
            .iter()
            .all(|r| r.len <= LONGEST_ALIGNMENT_ROW)
        && !files::gff_sidecar(path).exists();
    Ok(if named || rows {
        Kind::Alignment
    } else {
        Kind::Sequence
    })
}
