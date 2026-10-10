//! Static interval index: items sorted by start, with the maximum end per
//! block of 64 items and per superblock of 4096. A query binary-searches the
//! last item starting before the query end, then walks backwards, skipping
//! any block whose maximum end lies before the query start. One huge feature
//! (a `source` spanning the genome) costs one extra block visit, not a scan.

const BLOCK: usize = 64;
const SUPER: usize = 64 * BLOCK;

#[derive(Clone, Debug, Default)]
pub struct IntervalIndex {
    /// `(start, end, id)`, sorted by start. Half-open.
    items: Vec<(u64, u64, u32)>,
    block_max: Vec<u64>,
    super_max: Vec<u64>,
}

impl IntervalIndex {
    pub fn new(mut items: Vec<(u64, u64, u32)>) -> Self {
        items.sort_unstable_by_key(|&(s, e, id)| (s, e, id));
        let block_max = items
            .chunks(BLOCK)
            .map(|c| c.iter().map(|i| i.1).max().unwrap_or(0))
            .collect();
        let super_max = items
            .chunks(SUPER)
            .map(|c| c.iter().map(|i| i.1).max().unwrap_or(0))
            .collect();
        Self {
            items,
            block_max,
            super_max,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Ids of items overlapping `start..end`, in order of start. Empty items
    /// (sites between bases) match when they lie inside the query.
    pub fn query(&self, start: u64, end: u64, out: &mut Vec<u32>) {
        let first = out.len();
        self.each_overlap(start, end, |item| out.push(item.2));
        out[first..].reverse();
    }

    /// Calls `f` for each overlapping item, in reverse order of start.
    fn each_overlap(&self, start: u64, end: u64, mut f: impl FnMut(&(u64, u64, u32))) {
        let hi = self.items.partition_point(|i| i.0 < end.max(start + 1));
        let mut i = hi;
        while i > 0 {
            // Blocks whose ends all lie before `start` can't overlap. Using
            // `<` rather than `<=` keeps empty items at `start` reachable.
            let sup = (i - 1) / SUPER;
            if self.super_max[sup] < start {
                i = sup * SUPER;
                continue;
            }
            let blk = (i - 1) / BLOCK;
            if self.block_max[blk] < start {
                i = blk * BLOCK;
                continue;
            }
            let item = &self.items[i - 1];
            if item.1 > start || (item.0 == item.1 && item.0 >= start) {
                f(item);
            }
            i -= 1;
        }
    }

    /// Number of items overlapping each of `bins` equal bins over
    /// `start..end`, for drawing density when features are too small to see.
    pub fn density(&self, start: u64, end: u64, bins: usize) -> Vec<u32> {
        let mut counts = vec![0u32; bins];
        if bins == 0 || end <= start {
            return counts;
        }
        let span = end - start;
        let to_bin = |x: u64| -> usize {
            let rel = x.clamp(start, end) - start;
            #[allow(clippy::cast_possible_truncation)]
            let b = (u128::from(rel) * bins as u128 / u128::from(span)) as usize;
            b.min(bins - 1)
        };
        self.each_overlap(start, end, |&(s, e, _)| {
            for c in &mut counts[to_bin(s)..=to_bin(e.saturating_sub(1).max(s))] {
                *c += 1;
            }
        });
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(items: &[(u64, u64, u32)], s: u64, e: u64) -> Vec<u32> {
        let mut v: Vec<(u64, u64, u32)> = items
            .iter()
            .copied()
            .filter(|&(a, b, _)| {
                if a == b {
                    a >= s && a < e.max(s + 1)
                } else {
                    a < e.max(s + 1) && b > s
                }
            })
            .collect();
        v.sort_unstable();
        v.into_iter().map(|i| i.2).collect()
    }

    #[test]
    fn matches_naive_on_random_data() {
        let mut x = 12345u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut items = Vec::new();
        for id in 0..20_000u32 {
            let s = rnd(1_000_000);
            let len = if id % 997 == 0 {
                rnd(500_000)
            } else {
                rnd(2_000)
            };
            items.push((s, s + len, id));
        }
        items.push((0, 1_000_000, 99_999));
        let idx = IntervalIndex::new(items.clone());
        for _ in 0..500 {
            let s = rnd(1_000_000);
            let e = s + rnd(50_000);
            let mut got = Vec::new();
            idx.query(s, e, &mut got);
            assert_eq!(got, naive(&items, s, e), "{s}..{e}");
        }
    }

    #[test]
    fn density_counts_overlaps() {
        let idx = IntervalIndex::new(vec![(0, 10, 0), (5, 25, 1), (30, 40, 2)]);
        assert_eq!(idx.density(0, 40, 4), vec![2, 1, 1, 1]);
    }
}
