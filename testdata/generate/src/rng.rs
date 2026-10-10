//! SplitMix64. Hand-rolled so output never changes with a dependency update:
//! the same seed must give byte-identical files on every machine and version.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`. The modulo bias is irrelevant for test data.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }

    pub fn chance(&mut self, p: f64) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let x = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        x < p
    }

    /// A nucleotide with the given GC fraction.
    pub fn base(&mut self, gc: f64) -> u8 {
        let gc_base = self.chance(gc);
        match (gc_base, self.chance(0.5)) {
            (true, true) => b'G',
            (true, false) => b'C',
            (false, true) => b'A',
            (false, false) => b'T',
        }
    }

    /// Derive an independent stream, so adding output to one generator
    /// doesn't shift every other generator's data.
    pub fn fork(&mut self, salt: u64) -> Rng {
        Rng::new(self.next_u64() ^ salt.wrapping_mul(0xD1B5_4A32_D192_ED03))
    }
}
