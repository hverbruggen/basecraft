//! Frozen alignment context for one review-only Sanger placement.

use basecraft_docs::sanger::SangerPlacementExt;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use basecraft_docs::AlignmentDoc;
use basecraft_docs::alignment::ColumnStats;
use basecraft_docs::sanger::{SangerDraft, SangerPlacement};

#[derive(Clone, Debug)]
pub struct RepresentativeRow {
    pub row: usize,
    pub copies: usize,
    pub matches: usize,
    pub comparable: usize,
}

pub struct AlignmentContext {
    pub placement: SangerPlacement,
    pub alignment: AlignmentDoc,
    /// Tracy consensus alignment column -> frozen source MSA column.
    pub columns: Vec<Option<usize>>,
    /// Precomputed whole-MSA summaries, never derived from displayed rows.
    pub stats: Vec<Option<ColumnStats>>,
    pub representatives: Vec<RepresentativeRow>,
}

impl AlignmentContext {
    pub fn new(
        draft: &SangerDraft,
        placement: SangerPlacement,
        alignment: AlignmentDoc,
    ) -> Result<Self, String> {
        placement.validate()?;
        if alignment.is_protein()
            || alignment.row_count() != placement.source_rows
            || usize::try_from(alignment.width()).ok() != Some(placement.source_width)
        {
            return Err("Placement does not match a frozen DNA alignment".into());
        }
        let fragment: String = draft
            .consensus
            .chars()
            .filter(|&base| base != '-')
            .collect();
        if fragment != placement.fragment {
            return Err("Placement fragment differs from Tracy draft".into());
        }
        let mut next = 0;
        let columns: Vec<Option<usize>> = draft
            .consensus
            .bytes()
            .map(|base| {
                if base == b'-' {
                    None
                } else {
                    let mapped = placement.map[next];
                    next += 1;
                    mapped
                }
            })
            .collect();
        let stats = columns
            .iter()
            .map(|&column| column.and_then(|col| alignment.column_stats(col as u64)))
            .collect();
        let mapped: Vec<usize> = placement.map.iter().flatten().copied().collect();
        let first = *mapped.first().ok_or("No placed source columns")?;
        let last = *mapped.last().ok_or("No placed source columns")? + 1;
        if last - first > 20_000 {
            return Err("Sanger placement spans more than 20,000 alignment columns".into());
        }
        let mut candidates = Vec::new();
        for row in 0..alignment.row_count() {
            let segment = alignment.read(row, first as u64, last as u64);
            let mut comparable = 0;
            let mut matches = 0;
            for (i, &col) in placement.map.iter().enumerate() {
                if let Some(col) = col {
                    let reference = segment[col - first].to_ascii_uppercase();
                    let proposal = placement.fragment.as_bytes()[i];
                    if b"ACGT".contains(&reference) && b"ACGT".contains(&proposal) {
                        comparable += 1;
                        matches += usize::from(reference == proposal);
                    }
                }
            }
            candidates.push(RepresentativeRow {
                row,
                copies: 1,
                matches,
                comparable,
            });
        }
        // Identity over placed, unambiguous bases determines the extremes.
        // Rows without comparable evidence are eligible for the random extras.
        candidates.sort_by(|a, b| {
            (b.comparable > 0)
                .cmp(&(a.comparable > 0))
                .then_with(|| {
                    (b.matches * a.comparable.max(1)).cmp(&(a.matches * b.comparable.max(1)))
                })
                .then_with(|| b.comparable.cmp(&a.comparable))
                .then_with(|| a.row.cmp(&b.row))
        });
        let mut representatives = Vec::new();
        if !candidates.is_empty() {
            representatives.push(candidates.remove(0));
            let distant = candidates
                .iter()
                .rposition(|r| r.comparable > 0)
                .or_else(|| (!candidates.is_empty()).then_some(0));
            if let Some(i) = distant {
                representatives.push(candidates.remove(i));
            }
            // RandomState is independently seeded by the standard library.
            // Hash ranking samples distinct rows without a new RNG dependency.
            let random = RandomState::new();
            candidates.sort_by_key(|r| random.hash_one(r.row));
            representatives.extend(candidates.into_iter().take(1));
        }
        Ok(Self {
            placement,
            alignment,
            columns,
            stats,
            representatives,
        })
    }

    pub fn site_text(&self, selected: usize) -> String {
        let Some(column) = self.columns.get(selected).copied().flatten() else {
            let left = self.columns[..selected]
                .iter()
                .rev()
                .flatten()
                .next()
                .map(|x| x + 1);
            let right = self.columns[selected + 1..]
                .iter()
                .flatten()
                .next()
                .map(|x| x + 1);
            return format!(
                "{} [{}] · novel insertion or unplaced event between source columns {:?} and {:?}; no alignment frequency",
                self.placement.source_path.display(),
                &self.placement.source_sha256[..8],
                left,
                right
            );
        };
        let stats = self.stats[selected].expect("mapped column has stats");
        let entropy = stats
            .entropy_bits()
            .map_or("NA".into(), |bits| format!("{bits:.2} bits"));
        let mut text = format!(
            "{} [{}] · frozen MSA column {} of {} rows · A:{} C:{} G:{} T:{} · n={} gap={} N/?={} ambiguity={} · H={}",
            self.placement.source_path.display(),
            &self.placement.source_sha256[..8],
            column + 1,
            self.alignment.row_count(),
            stats.acgt[0],
            stats.acgt[1],
            stats.acgt[2],
            stats.acgt[3],
            stats.informative(),
            stats.gap,
            stats.missing,
            stats.ambiguous,
            entropy
        );
        for representative in &self.representatives {
            let base = self
                .alignment
                .read(representative.row, column as u64, column as u64 + 1)
                .first()
                .copied()
                .unwrap_or(b'?') as char;
            let _ = std::fmt::Write::write_fmt(
                &mut text,
                format_args!(
                    "\n{}: {} ({} similar rows; {}/{} placed matches)",
                    self.alignment.row(representative.row).name,
                    base,
                    representative.copies,
                    representative.matches,
                    representative.comparable
                ),
            );
        }
        text
    }
}
