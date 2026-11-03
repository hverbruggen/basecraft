//! Counting kernels chosen for the running CPU. A distributed binary can't assume hardware
//! popcount, and without it counting is much slower. Every path returns identical integers.
// Bodies must inline to compile with each kernel's CPU features.
#![allow(clippy::inline_always, clippy::many_single_char_names)]

/// Packed DNA columns of one sequence: low bit, high bit and valid-site mask.
#[derive(Clone, Copy)]
pub struct Words<'a> {
    pub lo: &'a [u64],
    pub hi: &'a [u64],
    pub valid: &'a [u64],
}

/// Counts valid sites, transitions and transversions, optionally only within a site mask.
pub type PairCounts = fn(Words<'_>, Words<'_>) -> (u64, u64, u64);
pub type MaskedPairCounts = fn(Words<'_>, Words<'_>, &[u64]) -> (u64, u64, u64);

#[inline(always)]
fn counts(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
    let n = a.lo.len();
    let (a_hi, a_valid, b_lo, b_hi, b_valid) = (
        &a.hi[..n],
        &a.valid[..n],
        &b.lo[..n],
        &b.hi[..n],
        &b.valid[..n],
    );
    let (mut valid, mut ts, mut tv) = (0, 0, 0);
    for k in 0..n {
        let mask = a_valid[k] & b_valid[k];
        let h = (a_hi[k] ^ b_hi[k]) & mask;
        let l = (a.lo[k] ^ b_lo[k]) & mask;
        valid += u64::from(mask.count_ones());
        tv += u64::from(h.count_ones());
        ts += u64::from((l & !h).count_ones());
    }
    (valid, ts, tv)
}

#[inline(always)]
fn masked_counts(a: Words<'_>, b: Words<'_>, sites: &[u64]) -> (u64, u64, u64) {
    let n = a.lo.len();
    let (a_hi, a_valid, b_lo, b_hi, b_valid, sites) = (
        &a.hi[..n],
        &a.valid[..n],
        &b.lo[..n],
        &b.hi[..n],
        &b.valid[..n],
        &sites[..n],
    );
    let (mut valid, mut ts, mut tv) = (0, 0, 0);
    for k in 0..n {
        let mask = a_valid[k] & b_valid[k] & sites[k];
        let h = (a_hi[k] ^ b_hi[k]) & mask;
        let l = (a.lo[k] ^ b_lo[k]) & mask;
        valid += u64::from(mask.count_ones());
        tv += u64::from(h.count_ones());
        ts += u64::from((l & !h).count_ones());
    }
    (valid, ts, tv)
}

fn counts_baseline(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
    counts(a, b)
}

fn masked_baseline(a: Words<'_>, b: Words<'_>, sites: &[u64]) -> (u64, u64, u64) {
    masked_counts(a, b, sites)
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{Words, counts, masked_counts};

    #[target_feature(enable = "popcnt")]
    fn counts_popcnt_body(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
        counts(a, b)
    }
    #[target_feature(enable = "avx2,popcnt")]
    fn counts_avx2_body(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
        counts(a, b)
    }
    #[target_feature(enable = "popcnt")]
    fn masked_popcnt_body(a: Words<'_>, b: Words<'_>, sites: &[u64]) -> (u64, u64, u64) {
        masked_counts(a, b, sites)
    }
    #[target_feature(enable = "avx2,popcnt")]
    fn masked_avx2_body(a: Words<'_>, b: Words<'_>, sites: &[u64]) -> (u64, u64, u64) {
        masked_counts(a, b, sites)
    }

    // SAFETY for all four: only reachable through the selectors below, after detection.
    pub(super) fn counts_popcnt(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
        unsafe { counts_popcnt_body(a, b) }
    }
    pub(super) fn counts_avx2(a: Words<'_>, b: Words<'_>) -> (u64, u64, u64) {
        unsafe { counts_avx2_body(a, b) }
    }
    pub(super) fn masked_popcnt(a: Words<'_>, b: Words<'_>, s: &[u64]) -> (u64, u64, u64) {
        unsafe { masked_popcnt_body(a, b, s) }
    }
    pub(super) fn masked_avx2(a: Words<'_>, b: Words<'_>, s: &[u64]) -> (u64, u64, u64) {
        unsafe { masked_avx2_body(a, b, s) }
    }
}

/// The fastest pair-counting kernel this CPU supports.
pub fn pair_counts() -> PairCounts {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("popcnt") {
            return x86::counts_avx2;
        }
        if is_x86_feature_detected!("popcnt") {
            return x86::counts_popcnt;
        }
    }
    counts_baseline
}

/// The fastest masked pair-counting kernel this CPU supports.
pub fn masked_pair_counts() -> MaskedPairCounts {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("popcnt") {
            return x86::masked_avx2;
        }
        if is_x86_feature_detected!("popcnt") {
            return x86::masked_popcnt;
        }
    }
    masked_baseline
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(seed: u64, n: usize) -> Vec<u64> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                state ^ (state >> 29)
            })
            .collect()
    }

    #[test]
    fn every_available_kernel_counts_the_same() {
        for n in [0, 1, 3, 4, 5, 31, 157] {
            let (al, ah, av) = (words(1, n), words(2, n), words(3, n));
            let (bl, bh, bv) = (words(4, n), words(5, n), words(6, n));
            let sites = words(7, n);
            let a = Words {
                lo: &al,
                hi: &ah,
                valid: &av,
            };
            let b = Words {
                lo: &bl,
                hi: &bh,
                valid: &bv,
            };
            // A word-at-a-time count is the reference for all kernels.
            let (mut valid, mut ts, mut tv) = (0, 0, 0);
            let (mut m_valid, mut m_ts, mut m_tv) = (0, 0, 0);
            for k in 0..n {
                for bit in 0..64 {
                    if (av[k] & bv[k]) >> bit & 1 == 1 {
                        let high = (ah[k] ^ bh[k]) >> bit & 1 == 1;
                        let low = (al[k] ^ bl[k]) >> bit & 1 == 1;
                        let selected = sites[k] >> bit & 1 == 1;
                        valid += 1;
                        m_valid += u64::from(selected);
                        if high {
                            tv += 1;
                            m_tv += u64::from(selected);
                        } else if low {
                            ts += 1;
                            m_ts += u64::from(selected);
                        }
                    }
                }
            }
            assert_eq!(counts_baseline(a, b), (valid, ts, tv));
            assert_eq!(pair_counts()(a, b), (valid, ts, tv));
            assert_eq!(masked_baseline(a, b, &sites), (m_valid, m_ts, m_tv));
            assert_eq!(masked_pair_counts()(a, b, &sites), (m_valid, m_ts, m_tv));
            #[cfg(target_arch = "x86_64")]
            {
                if is_x86_feature_detected!("popcnt") {
                    assert_eq!(x86::counts_popcnt(a, b), (valid, ts, tv));
                    assert_eq!(x86::masked_popcnt(a, b, &sites), (m_valid, m_ts, m_tv));
                }
                if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("popcnt") {
                    assert_eq!(x86::counts_avx2(a, b), (valid, ts, tv));
                    assert_eq!(x86::masked_avx2(a, b, &sites), (m_valid, m_ts, m_tv));
                }
            }
        }
    }
}
