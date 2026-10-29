//! Bounded, read-only text pages for scientific canvases. Coordinates are one-based.
use std::fmt::Write as _;

use basecraft_docs::{AlignmentDoc, SequenceDoc, TreeDoc};
use basecraft_io::abif::Trace;

pub struct TextPage {
    pub page: u64,
    pub pages: u64,
    pub text: String,
}

fn bounds(requested: i32, count: u64, size: u64) -> (u64, u64, u64, u64) {
    let pages = count.div_ceil(size).max(1);
    let page = u64::try_from(requested).unwrap_or(0).min(pages - 1);
    let start = page * size;
    (page, pages, start, start.saturating_add(size).min(count))
}

pub fn sequence(doc: &SequenceDoc, requested: i32) -> TextPage {
    let (page, pages, start, end) = bounds(requested, doc.len(), 1000);
    TextPage {
        page,
        pages,
        text: format!(
            "Sequence, {} bases. Bases {}–{} (one-based).\n{}",
            doc.len(),
            if end > start { start + 1 } else { 0 },
            end,
            String::from_utf8_lossy(&doc.read(start, end))
        ),
    }
}

pub fn alignment(doc: &AlignmentDoc, requested: i32) -> TextPage {
    let chunks = doc.width().div_ceil(1000).max(1);
    let pages = (doc.row_count() as u64).saturating_mul(chunks).max(1);
    let page = u64::try_from(requested).unwrap_or(0).min(pages - 1);
    let row = usize::try_from(page / chunks).unwrap_or(usize::MAX);
    let start = page % chunks * 1000;
    let end = (start + 1000).min(doc.width());
    let text = if row < doc.row_count() {
        format!(
            "Alignment, {} rows, {} columns. Row {}: {}. Columns {}–{} (one-based).\n{}",
            doc.row_count(),
            doc.width(),
            row + 1,
            doc.row(row).name,
            if end > start { start + 1 } else { 0 },
            end,
            String::from_utf8_lossy(&doc.read(row, start, end))
        )
    } else {
        "Empty alignment".into()
    };
    TextPage { page, pages, text }
}

pub fn tree(doc: &TreeDoc, requested: i32) -> TextPage {
    let (page, pages, start, end) = bounds(requested, doc.nodes().len() as u64, 50);
    let mut text = format!(
        "Tree, {} tips. Node identifiers are one-based. Root: {}.\nNode\tName\tParent\tBranch length\tDisplay label",
        doc.tip_count(),
        doc.root() + 1
    );
    for set in doc.supports() {
        let _ = write!(text, "\t{}", set.name);
    }
    text.push('\n');
    for (i, node) in doc
        .nodes()
        .iter()
        .enumerate()
        .skip(usize::try_from(start).unwrap_or(usize::MAX))
        .take(usize::try_from(end - start).unwrap_or(0))
    {
        let _ = write!(
            text,
            "{}\t{}\t{}\t{}\t{}",
            i + 1,
            node.name,
            node.parent
                .map_or_else(|| "root".into(), |p| (p + 1).to_string()),
            node.length
                .map_or_else(|| "not recorded".into(), |v| v.to_string()),
            if node.is_tip() {
                super::tree::tip_label(doc, &node.name)
            } else {
                node.name.clone()
            }
        );
        for set in doc.supports() {
            let _ = write!(
                text,
                "\t{}",
                set.formatted(i, doc.style()).unwrap_or_else(|| "-".into())
            );
        }
        text.push('\n');
    }
    TextPage { page, pages, text }
}

pub fn trace(trace: &Trace, requested: i32) -> TextPage {
    // Empty call sets are possible in programmatically constructed fixtures.
    let Some(calls) = trace
        .call_sets
        .iter()
        .find(|c| c.number == 1)
        .or_else(|| trace.call_sets.first())
    else {
        return TextPage {
            page: 0,
            pages: 1,
            text: "No trace calls".into(),
        };
    };
    let (page, pages, start, end) = bounds(requested, calls.bases.len() as u64, 50);
    let mut text = format!(
        "Trace call set {}. Base and sample coordinates are one-based.\nBase\tCall\tSample\tQuality\n",
        calls.number
    );
    for (i, base) in calls
        .bases
        .iter()
        .enumerate()
        .take(usize::try_from(end).unwrap_or(0))
        .skip(usize::try_from(start).unwrap_or(0))
    {
        let sample = calls.positions.get(i).map_or_else(
            || "not recorded".into(),
            |p| (u64::from(*p) + 1).to_string(),
        );
        let quality = calls
            .qualities
            .as_ref()
            .and_then(|q| q.get(i))
            .map_or_else(|| "not recorded".into(), ToString::to_string);
        let _ = writeln!(
            text,
            "{}\t{}\t{sample}\t{quality}",
            i + 1,
            char::from(*base)
        );
    }
    TextPage { page, pages, text }
}

pub fn review(bases: &[u8], masked: &[bool], unresolved: &[bool], requested: i32) -> TextPage {
    let (page, pages, start, end) = bounds(requested, bases.len() as u64, 50);
    let mut text =
        "Reviewed consensus. Columns are one-based.\nColumn\tBase\tMasked\tUnresolved\n".to_owned();
    for (i, base) in bases
        .iter()
        .enumerate()
        .take(usize::try_from(end).unwrap_or(0))
        .skip(usize::try_from(start).unwrap_or(0))
    {
        let flag = |values: &[bool]| {
            values
                .get(i)
                .map_or("not recorded", |v| if *v { "yes" } else { "no" })
        };
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}",
            i + 1,
            char::from(*base),
            flag(masked),
            flag(unresolved)
        );
    }
    TextPage { page, pages, text }
}

/// Species membership is readable even when no display tree is attached.
pub fn delimitation(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    requested: i32,
) -> TextPage {
    let per_hypothesis = (work.specimens.len() as u64).div_ceil(50).max(1);
    let pages = per_hypothesis
        .saturating_mul(work.hypotheses.len().max(1) as u64)
        .max(1);
    let page = u64::try_from(requested).unwrap_or(0).min(pages - 1);
    let hypothesis = usize::try_from(page / per_hypothesis)
        .ok()
        .and_then(|i| work.hypotheses.get(i));
    let mut text = format!(
        "Species hypotheses: {}\nHypothesis: {}\nSequence\tSpecies group\n",
        work.name,
        hypothesis.map_or("None yet", |h| h.name.as_str())
    );
    for specimen in work
        .specimens
        .iter()
        .skip(usize::try_from((page % per_hypothesis) * 50).unwrap_or(usize::MAX))
        .take(50)
    {
        let group =
            hypothesis.and_then(|h| h.groups.iter().find(|g| g.members.contains(&specimen.id)));
        let _ = writeln!(
            text,
            "{}\t{}",
            specimen.label,
            group.map_or("Unassigned", |g| g.label.as_str())
        );
    }
    TextPage { page, pages, text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_docs::alignment::AlnRow;
    use std::sync::Arc;
    #[test]
    fn sequence_and_alignment_pages_cover_boundaries_without_overlap() {
        let mut bases = vec![b'A'; 1000];
        bases.push(b'C');
        let sequence_doc = SequenceDoc::new("sequence", Arc::new(bases.clone()), vec![], false);
        let first = sequence(&sequence_doc, 0);
        let last = sequence(&sequence_doc, 1);
        assert_eq!(first.pages, 2);
        assert!(first.text.contains("Bases 1–1000"));
        assert!(last.text.ends_with("\nC"));
        let alignment_doc = AlignmentDoc::new(
            "alignment",
            vec![
                AlnRow::new("one", Arc::new(bases.clone())),
                AlnRow::new("two", Arc::new(bases)),
            ],
        );
        let next_row = alignment(&alignment_doc, 2);
        assert_eq!(next_row.pages, 4);
        assert!(next_row.text.contains("Row 2: two. Columns 1–1000"));
        assert!(alignment(&alignment_doc, i32::MAX).text.ends_with("\nC"));
    }
    #[test]
    fn pages_bound_untrusted_requests_and_empty_documents() {
        assert_eq!(bounds(-5, 0, 1000), (0, 1, 0, 0));
        assert_eq!(bounds(i32::MAX, 1001, 1000), (1, 2, 1000, 1001));
        let doc = TreeDoc::from_newick("tree", "(A:1,B:2);").unwrap();
        let page = tree(&doc, i32::MAX);
        assert!(page.text.contains('A'));
        assert!(page.text.contains("root"));
        assert_eq!(page.pages, 1);
    }
    #[test]
    fn review_reports_unknown_and_distinguishes_masked_from_unresolved() {
        let page = review(b"AC", &[true, false], &[false], 0);
        assert!(page.text.contains("1\tA\tyes\tno"));
        assert!(page.text.contains("2\tC\tno\tnot recorded"));
    }
    #[test]
    fn species_pages_include_membership_and_unassigned_sequences() {
        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        let page = delimitation(&work, 0);
        assert!(page.text.contains("a\t1"));
        assert_eq!(page.pages, 3);
        work.hypotheses.clear();
        let template = work.specimens[0].clone();
        work.specimens = (0..101)
            .map(|i| {
                let mut specimen = template.clone();
                specimen.id = i.to_string();
                specimen.label = format!("Sequence {i}");
                specimen
            })
            .collect();
        let page = delimitation(&work, i32::MAX);
        assert_eq!(page.page, 2);
        assert_eq!(page.pages, 3);
        assert!(page.text.contains("Sequence 100\tUnassigned"));
        assert_eq!(delimitation(&work, -1).page, 0);
    }
}
