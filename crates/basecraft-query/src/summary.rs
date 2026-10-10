//! Per-zoom sequence summaries: GC and N counts per bin, with levels that
//! double the bin size up to one bin per sequence. Used when residues are
//! too small to draw.

#[derive(Clone, Debug)]
pub struct SeqSummary {
    pub len: u64,
    /// Bin size of level 0.
    pub base_bin: u64,
    /// `levels[k]` holds `(gc, acgt)` counts for bins of `base_bin << k`.
    levels: Vec<Vec<(u32, u32)>>,
}

impl SeqSummary {
    /// Build from residues streamed in order by `read(start, end, buf)`.
    pub fn build(len: u64, base_bin: u64, mut read: impl FnMut(u64, u64, &mut Vec<u8>)) -> Self {
        let bins = len.div_ceil(base_bin);
        #[allow(clippy::cast_possible_truncation)]
        let mut level0 = Vec::with_capacity(bins as usize);
        let mut buf = Vec::new();
        let chunk = base_bin * 1024;
        let mut pos = 0;
        while pos < len {
            let end = (pos + chunk).min(len);
            buf.clear();
            read(pos, end, &mut buf);
            #[allow(clippy::cast_possible_truncation)]
            for bin in buf.chunks(base_bin as usize) {
                let (mut gc, mut acgt) = (0u32, 0u32);
                for &b in bin {
                    match b | 0x20 {
                        b'g' | b'c' | b's' => {
                            gc += 1;
                            acgt += 1;
                        }
                        b'a' | b't' | b'u' | b'w' => acgt += 1,
                        _ => {}
                    }
                }
                level0.push((gc, acgt));
            }
            pos = end;
        }
        let mut levels = vec![level0];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let prev = levels.last().map(Vec::as_slice).unwrap_or_default();
            let next = prev
                .chunks(2)
                .map(|p| p.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)))
                .collect();
            levels.push(next);
        }
        Self {
            len,
            base_bin,
            levels,
        }
    }

    pub fn from_bytes(seq: &[u8], base_bin: u64) -> Self {
        Self::build(seq.len() as u64, base_bin, |s, e, buf| {
            #[allow(clippy::cast_possible_truncation)]
            buf.extend_from_slice(&seq[s as usize..e as usize]);
        })
    }

    /// GC fraction for `bins` equal bins over `start..end`, using the
    /// coarsest level that still gives at least one summary bin per output
    /// bin. `NaN` where a bin has no A, C, G or T.
    pub fn gc(&self, start: u64, end: u64, bins: usize) -> Vec<f32> {
        let mut out = vec![f32::NAN; bins];
        if bins == 0 || end <= start || self.levels[0].is_empty() {
            return out;
        }
        let span = end - start;
        let want = (span / bins as u64).max(1);
        let mut level = 0;
        while level + 1 < self.levels.len() && (self.base_bin << (level + 1)) <= want {
            level += 1;
        }
        let size = self.base_bin << level;
        let data = &self.levels[level];
        let mut acc = vec![(0u64, 0u64); bins];
        #[allow(clippy::cast_possible_truncation)]
        let first = (start / size) as usize;
        #[allow(clippy::cast_possible_truncation)]
        let last = (end.div_ceil(size) as usize).min(data.len());
        for (i, &(gc, acgt)) in data.iter().enumerate().take(last).skip(first) {
            let mid = (i as u64 * size + size / 2).clamp(start, end - 1);
            #[allow(clippy::cast_possible_truncation)]
            let b = ((u128::from(mid - start) * bins as u128) / u128::from(span)) as usize;
            let slot = &mut acc[b.min(bins - 1)];
            slot.0 += u64::from(gc);
            slot.1 += u64::from(acgt);
        }
        for (o, (gc, acgt)) in out.iter_mut().zip(acc) {
            if acgt > 0 {
                #[allow(clippy::cast_precision_loss)]
                let f = gc as f32 / acgt as f32;
                *o = f;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_at_several_zooms() {
        let mut seq = vec![b'A'; 4096];
        seq.extend(vec![b'G'; 4096]);
        let s = SeqSummary::from_bytes(&seq, 64);
        assert_eq!(s.gc(0, 8192, 2), vec![0.0, 1.0]);
        let fine = s.gc(4000, 4200, 2);
        assert!(fine[0] < 0.5 && fine[1] > 0.5, "{fine:?}");
        assert_eq!(s.gc(0, 8192, 1), vec![0.5]);
    }

    #[test]
    fn n_only_bins_are_nan() {
        let s = SeqSummary::from_bytes(&[b'N'; 1000], 64);
        assert!(s.gc(0, 1000, 3).iter().all(|x| x.is_nan()));
    }
}
