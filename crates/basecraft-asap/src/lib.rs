//! ASAP (Puillandre, Brouillet and Achaz 2021, doi:10.1111/1755-0998.13281).
//!
//! Port of the reference command-line ASAP core (G. Achaz and S. Brouillet,
//! `asap_core.c`/`asap_common.c`). Clustering, coalescent simulations, p-values,
//! barcode-gap slopes and ranks reproduce that code's arithmetic. With
//! [`Stream::Reference`] the random stream is glibc `rand()` and sorts follow
//! glibc's merge-sort tie order, so a fixed seed gives the reference results on
//! the same distance matrix. [`Stream::Parallel`] runs the same logic on
//! independent per-simulation streams. Distances come from the caller.
//!
//! The reference ignores its replicate setting and doubles simulations per node
//! up to a fixed 100000; [`Settings::max_simulations`] is that cap. Two
//! reference defects are not reproduced: specimens that are singletons at a
//! zero merge distance stay in the partition, and each subset carries its own
//! node's p-value (the reference SPART-XML attaches them by position).
//! [`Analysis::diagnostics`] reports when either case, or an ambiguous tied
//! rank, occurs. See `methods_research/asap_native/README.md`.
// Counts are bounded by the specimen count and match the reference C int/long
// conversions; the casts mirror its implicit numeric promotions.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::float_cmp
)]
use serde::Serialize;
use std::fmt::Write as _;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;

const RAND_MAX: i32 = 2_147_483_647;
const EPSILON: f64 = 0.0001;
const MIN_SUCCESS: f64 = 50.0;
const INITIAL_SIMULATIONS: usize = 500;

/// Reference parameters. `p_threshold`, `slope_weight` and `score_weight` are
/// single precision in the reference, so they are kept as `f32` here.
#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    /// Alignment length used to scale distances to differences.
    pub sequence_length: u32,
    /// glibc `srand` seed.
    pub seed: u32,
    /// Upper limit of coalescent simulations per node (reference: 100000).
    pub max_simulations: usize,
    /// Node p-value threshold for recursive splitting (reference: 0.001).
    pub p_threshold: f32,
    /// Relative window for the barcode-gap slope (reference: 0.1).
    pub slope_weight: f32,
    /// Weight of the p-value rank in the ASAP score (reference: 0.5).
    pub score_weight: f32,
    /// Best partitions to describe in full (reference: 10).
    pub reported: usize,
    /// Random stream for the coalescent simulations.
    pub stream: Stream,
}

/// `Reference` reproduces the reference program bit for bit and runs the
/// simulations on one thread. `Parallel` gives every simulation its own
/// stream, keyed by seed, node and replicate, and runs them on the current
/// rayon pool. Its results do not depend on the thread count but differ from
/// the reference by Monte Carlo error.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    #[default]
    Reference,
    Parallel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sequence_length: 600,
            seed: 1,
            max_simulations: 100_000,
            p_threshold: 0.001,
            slope_weight: 0.1,
            score_weight: 0.5,
            reported: 10,
            stream: Stream::Reference,
        }
    }
}

/// Complete symmetric distances in Basecraft's triangular row order
/// `(1,0), (2,0), (2,1), ...`.
pub struct Distances<'a> {
    pub n: usize,
    pub values: &'a [f64],
}

impl Distances<'_> {
    fn get(&self, i: u32, j: u32) -> f64 {
        let (x, y) = if i > j { (i, j) } else { (j, i) };
        let x = x as usize;
        self.values[x * (x - 1) / 2 + y as usize]
    }
}

/// One subset of a described partition.
#[derive(Clone, Debug, Serialize)]
pub struct Subset {
    /// Specimen indices, in display (cladogram) order.
    pub members: Vec<usize>,
    /// Node p-value; -1 for a single specimen and 1 when untestable, as upstream.
    pub p_value: f64,
}

/// One candidate partition (one distinct merge distance).
#[derive(Clone, Debug, Serialize)]
pub struct Partition {
    /// Merge distance.
    pub distance: f64,
    /// Threshold: midpoint between this and the previous merge distance.
    pub threshold: f64,
    /// Groups after merging at `distance`.
    pub groups: usize,
    /// Groups just below `distance` (upstream `#species`).
    pub species: usize,
    /// Subsets after recursive splitting of significant nodes (upstream `#spec w/rec`).
    pub subsets: usize,
    pub probability: f64,
    pub probability_rank: usize,
    /// Barcode-gap slope `W`.
    pub slope: f64,
    pub slope_rank: usize,
    /// ASAP score; smaller is better.
    pub score: f64,
    /// Rank from the first score sort (upstream `rank_general`).
    pub rank_general: usize,
    /// Present for the best `reported` partitions.
    pub subsets_detail: Option<Vec<Subset>>,
    /// Agglomeration round that produced this partition.
    pub round: usize,
}

/// Node of the agglomeration tree.
#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub children: Vec<u32>,
    pub distance: f64,
    pub leaves: u32,
    pub p_value: f64,
    /// Creation round; -1 for specimens.
    pub round: i32,
    /// Parent node, or `u32::MAX` for the root.
    pub parent: u32,
    /// Smallest specimen index below (the reference's component ID).
    pub first: u32,
    #[serde(skip)]
    sum_all: f64,
    #[serde(skip)]
    sum_inter: f64,
    #[serde(skip)]
    sum_intra: f64,
    #[serde(skip)]
    nb_inter: i64,
    #[serde(skip)]
    nb_intra: i64,
    #[serde(skip)]
    to_be_checked: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub settings: Settings,
    /// All partitions in final rank order.
    pub partitions: Vec<Partition>,
    /// Agglomeration tree; leaves are `0..n`, the root is last.
    pub nodes: Vec<Node>,
    /// Specimen indices in cladogram order (upstream SPART individual order).
    pub display_order: Vec<usize>,
    /// Smallest and largest pairwise distance.
    pub distance_range: (f64, f64),
    pub diagnostics: Vec<String>,
}

/// glibc `random_r` with the default `TYPE_3` state used by `srand`/`rand`.
///
/// glibc updates `s[f] += s[r]` with `r = f - 3 (mod 31)`, so its outputs follow
/// `x[i] = x[i-31] + x[i-3] (mod 2^32)`, returned as `x[i] >> 1`. Values are
/// generated in blocks and converted to upstream `unirandom` doubles in bulk.
pub struct GlibcRand {
    /// The 31 most recent recurrence values followed by one block.
    history: Vec<u32>,
    /// `x / RAND_MAX` for the current block; exactly 1.0 marks `x == RAND_MAX`.
    uniforms: Vec<f64>,
    raw: Vec<i32>,
    next: usize,
}

const BLOCK: usize = 4096;

impl GlibcRand {
    pub fn new(seed: u32) -> Self {
        let seed = if seed == 0 { 1 } else { seed };
        let mut state = [0u32; 31];
        state[0] = seed;
        // glibc stores the unsigned seed in an int32_t before seeding.
        let mut word = i64::from(seed as i32);
        for value in state.iter_mut().skip(1) {
            let hi = word / 127_773;
            let lo = word % 127_773;
            word = 16_807 * lo - 2_836 * hi;
            if word < 0 {
                word += 2_147_483_647;
            }
            *value = word as u32;
        }
        // Oldest first: the first update reads s[3] (31 back) and s[0] (3 back).
        let mut history = Vec::with_capacity(31 + BLOCK);
        history.extend_from_slice(&state[3..]);
        history.extend_from_slice(&state[..3]);
        let mut rand = Self {
            history,
            uniforms: Vec::with_capacity(BLOCK),
            raw: Vec::with_capacity(BLOCK),
            next: 0,
        };
        rand.refill();
        // glibc discards the first 310 outputs.
        rand.next = 310;
        rand
    }

    fn refill(&mut self) {
        let keep = self.history.len() - 31;
        self.history.drain(..keep);
        self.history.resize(31 + BLOCK, 0);
        for i in 31..31 + BLOCK {
            self.history[i] = self.history[i - 31].wrapping_add(self.history[i - 3]);
        }
        self.raw.clear();
        self.raw
            .extend(self.history[31..].iter().map(|&x| (x >> 1) as i32));
        self.uniforms.clear();
        self.uniforms
            .extend(self.raw.iter().map(|&x| f64::from(x) / f64::from(RAND_MAX)));
        self.next = 0;
    }

    pub fn next_i32(&mut self) -> i32 {
        if self.next == BLOCK {
            self.refill();
        }
        self.next += 1;
        self.raw[self.next - 1]
    }
}

/// Uniform draws plus the reference `exponentialdev` and `poissondev`.
trait Draws {
    fn uniform(&mut self) -> f64;

    fn exponential(&mut self) -> f64 {
        let mut u = self.uniform();
        while u == 0.0 {
            u = self.uniform();
        }
        -u.ln()
    }

    fn poisson(&mut self, mean: f64) -> f64 {
        // exp(-0) is exactly 1; skipping the call keeps the same stream.
        let g = if mean == 0.0 { 1.0 } else { (-mean).exp() };
        let mut events = -1i32;
        let mut t = 1.0;
        loop {
            events += 1;
            t *= self.uniform();
            if t <= g {
                return f64::from(events);
            }
        }
    }
}

impl Draws for GlibcRand {
    /// Upstream `unirandom`: uniform in [0, 1), redrawing `RAND_MAX`.
    #[inline]
    fn uniform(&mut self) -> f64 {
        loop {
            if self.next == BLOCK {
                self.refill();
            }
            let u = self.uniforms[self.next];
            self.next += 1;
            if u != 1.0 {
                return u;
            }
        }
    }
}

/// xoshiro256++ for [`Stream::Parallel`], one generator per simulation.
struct Xoshiro([u64; 4]);

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Xoshiro {
    fn keyed(seed: u32, node: usize, replicate: usize) -> Self {
        let mut state = u64::from(seed);
        let a = splitmix(&mut state);
        let mut state = a ^ (node as u64).wrapping_mul(0xd1b5_4a32_d192_ed03);
        let b = splitmix(&mut state);
        let mut state = b ^ (replicate as u64).wrapping_mul(0x8cb9_2ba7_2f3d_8dd7);
        Self([
            splitmix(&mut state),
            splitmix(&mut state),
            splitmix(&mut state),
            splitmix(&mut state),
        ])
    }
}

impl Draws for Xoshiro {
    #[inline]
    fn uniform(&mut self) -> f64 {
        let s = &mut self.0;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        (result >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// glibc `qsort` merge order (msort.c): left half n/2, right half n - n/2,
/// take the left element when `compare <= 0`. Upstream comparators are not
/// consistent orders, so this order decides ties.
pub fn glibc_merge_sort<T: Copy>(values: &mut [T], compare: &impl Fn(&T, &T) -> i32) {
    let mut scratch = Vec::with_capacity(values.len());
    merge_sort(values, &mut scratch, compare);
}

fn merge_sort<T: Copy>(values: &mut [T], scratch: &mut Vec<T>, compare: &impl Fn(&T, &T) -> i32) {
    let n = values.len();
    if n <= 1 {
        return;
    }
    let left = n / 2;
    merge_sort(&mut values[..left], scratch, compare);
    merge_sort(&mut values[left..], scratch, compare);
    scratch.clear();
    let (mut i, mut j) = (0, left);
    while i < left && j < n {
        if compare(&values[i], &values[j]) <= 0 {
            scratch.push(values[i]);
            i += 1;
        } else {
            scratch.push(values[j]);
            j += 1;
        }
    }
    scratch.extend_from_slice(&values[i..left]);
    values[..scratch.len()].copy_from_slice(scratch);
}

#[derive(Clone, Copy)]
struct Pair {
    d: f64,
    a: u32,
    b: u32,
}

/// Mutable state of one simulated top-down coalescent (upstream `RandomPi`).
struct Coalescent {
    left: Vec<i32>,
    right: Vec<i32>,
    length_left: Vec<f64>,
    length_right: Vec<f64>,
    /// Bitset of size classes with a nonzero lineage count on either side.
    active: Vec<u64>,
}

impl Coalescent {
    fn new(n: usize) -> Self {
        Self {
            left: vec![0; n + 1],
            right: vec![0; n + 1],
            length_left: vec![0.0; n + 1],
            length_right: vec![0.0; n + 1],
            active: vec![0; (n + 1).div_ceil(64)],
        }
    }

    fn mark(&mut self, i: usize) {
        if self.left[i] != 0 || self.right[i] != 0 {
            self.active[i / 64] |= 1 << (i % 64);
        } else {
            self.active[i / 64] &= !(1 << (i % 64));
        }
    }

    /// Upstream `compute_length` over `min..=max`. Classes with no lineage add
    /// exactly zero there, so only active classes are visited.
    fn add_time(&mut self, t: f64) {
        for (w, &word) in self.active.iter().enumerate() {
            let mut bits = word;
            while bits != 0 {
                let i = w * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                self.length_left[i] += f64::from(self.left[i]) * t;
                self.length_right[i] += f64::from(self.right[i]) * t;
            }
        }
    }

    /// Upstream `SplitSize`: choose a lineage by its number of possible splits.
    fn split(&mut self, total: i32, rand: &mut impl Draws) -> Option<()> {
        let u1 = (rand.uniform() * f64::from(total)).floor() as i32 + 1;
        let mut c = 0i32;
        let mut chosen = None;
        'scan: for (w, &word) in self.active.iter().enumerate() {
            let mut bits = word;
            while bits != 0 {
                let i = w * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                c += self.left[i] * i as i32;
                if c >= u1 {
                    chosen = Some((i, true));
                    break 'scan;
                }
                c += self.right[i] * i as i32;
                if c >= u1 {
                    chosen = Some((i, false));
                    break 'scan;
                }
            }
        }
        // Weights sum to `total`, so a class is always chosen.
        let (i, on_left) = chosen?;
        let u2 = (rand.uniform() * i as f64).floor() as usize + 1;
        let side = if on_left {
            &mut self.left
        } else {
            &mut self.right
        };
        side[i] -= 1;
        side[u2 - 1] += 1;
        side[i - u2] += 1;
        self.mark(i);
        self.mark(u2 - 1);
        self.mark(i - u2);
        Some(())
    }

    /// Returns intra-group differences and comparisons.
    fn simulate(
        &mut self,
        n: usize,
        mutation_rate: f64,
        k: usize,
        rand: &mut impl Draws,
    ) -> Option<(f64, i64)> {
        self.left[k - 1] = 1;
        self.right[n - k - 1] = 1;
        self.mark(k - 1);
        self.mark(n - k - 1);
        let t = rand.exponential();
        self.add_time(t);
        for i in 3..=n {
            self.split((n - i + 1) as i32, rand)?;
            let t = rand.exponential() * (2.0 / (i as f64 * (i as f64 - 1.0)));
            self.add_time(t);
        }
        let (kf, nf) = (k as f64, n as f64);
        // Upstream also sums all differences; no result uses them.
        let mut intra = 0.0;
        for i in 0..=k {
            let m = rand.poisson(mutation_rate * self.length_left[i]);
            intra += m * (i as f64 + 1.0) * (kf - i as f64 - 1.0);
        }
        for i in 0..=n - k {
            let m = rand.poisson(mutation_rate * self.length_right[i]);
            intra += m * (i as f64 + 1.0) * ((n - k) as f64 - i as f64 - 1.0);
        }
        let comparisons = ((kf * (kf - 1.0) / 2.0) + ((nf - kf) * (nf - kf - 1.0) / 2.0)) as i64;
        // All lineages end as single specimens in class 0.
        self.length_left[..k].fill(0.0);
        self.length_right[..n - k].fill(0.0);
        self.left[0] = 0;
        self.right[0] = 0;
        self.active[0] &= !1;
        debug_assert!(self.active.iter().all(|&w| w == 0));
        Some((intra, comparisons))
    }
}

struct Components {
    of: Vec<u32>,
    members: Vec<Vec<u32>>,
    sum_all: Vec<f64>,
    altered: Vec<u32>,
    is_altered: Vec<bool>,
    /// For each component: sizes and component IDs of the parts merged this round.
    parts: Vec<Vec<(i64, u32)>>,
    count: usize,
}

impl Components {
    fn new(n: usize) -> Self {
        Self {
            of: (0..n as u32).collect(),
            members: (0..n as u32).map(|i| vec![i]).collect(),
            sum_all: vec![0.0; n],
            altered: Vec::new(),
            is_altered: vec![false; n],
            parts: (0..n as u32).map(|i| vec![(1, i)]).collect(),
            count: n,
        }
    }

    /// Upstream `agglutine`: merge the larger component ID into the smaller.
    fn merge(
        &mut self,
        a: u32,
        b: u32,
        d: &Distances<'_>,
        s_inter: &mut f64,
        n_inter: &mut i64,
    ) -> bool {
        let (mut aa, mut bb) = (self.of[a as usize] as usize, self.of[b as usize] as usize);
        if aa == bb {
            return false;
        }
        if bb < aa {
            std::mem::swap(&mut aa, &mut bb);
        }
        self.sum_all[aa] += self.sum_all[bb];
        let moved = std::mem::take(&mut self.members[bb]);
        for &x in &self.members[aa] {
            for &y in &moved {
                let value = d.get(x, y);
                *s_inter += value;
                *n_inter += 1;
                self.sum_all[aa] += value;
            }
        }
        for &y in &moved {
            self.of[y as usize] = aa as u32;
        }
        self.members[aa].extend(moved);
        if !self.is_altered[aa] {
            self.altered.push(aa as u32);
            self.is_altered[aa] = true;
        }
        if self.is_altered[bb] {
            if let Some(at) = self.altered.iter().position(|&c| c as usize == bb) {
                self.altered.remove(at);
            }
        }
        let moved = std::mem::take(&mut self.parts[bb]);
        self.parts[aa].extend(moved);
        self.is_altered[bb] = false;
        self.count -= 1;
        true
    }

    /// Upstream `clearalltab`.
    fn clear_round(&mut self) {
        for &c in &self.altered {
            let c = c as usize;
            self.parts[c].clear();
            self.parts[c].push((self.members[c].len() as i64, c as u32));
            self.is_altered[c] = false;
        }
        self.altered.clear();
    }
}

struct Run<'a> {
    settings: &'a Settings,
    nodes: Vec<Node>,
    /// Current node of each specimen.
    node_of: Vec<u32>,
    comps: Components,
    coalescent: Coalescent,
    rand: GlibcRand,
}

/// Upstream `calcul_pente` on the sorted pair list, starting at the first pair
/// at this distance. Searches replace its linear scans with identical bounds.
fn slope(pairs: &[Pair], start: usize, weight: f32) -> f64 {
    let weight = f64::from(weight);
    let d = pairs[start].d;
    let d_min = d * (1.0 + weight);
    let d_max = if start > 0 {
        pairs[start - 1].d * (1.0 - weight)
    } else {
        d * (1.0 - weight)
    };
    let high = start + pairs[start..].partition_point(|p| p.d <= d_min);
    // Upstream reads one element past the list here; that memory is heap
    // padding or a zero-filled page, which reads as 0 or a subnormal value.
    let d_h = pairs.get(high).map_or(0.0, |p| p.d);
    let first = pairs[..=start].partition_point(|p| p.d < d_max);
    let low = first.saturating_sub(1).min(start);
    let d_l = pairs[low].d;
    let steps = (high - start) + (start - low);
    if steps == 0 {
        0.0
    } else {
        (d_h - d_l) / ((1.0 + d_h + d_l) * steps as f64)
    }
}

struct Target {
    node: usize,
    leaves: usize,
    k_obs: usize,
    mutation_rate: f64,
    pi_intra_scaled: f64,
    testable: bool,
}

impl Run<'_> {
    fn new_node(&mut self, component: usize, round: usize, round_distance: f64) -> Result<()> {
        let size = self.comps.members[component].len();
        if size < 2 {
            return Err(Error(
                "ASAP internal error: merged component has one member".into(),
            ));
        }
        let id = self.nodes.len() as u32;
        let children: Vec<u32> = self.comps.parts[component]
            .iter()
            .map(|&(_, c)| self.node_of[c as usize])
            .collect();
        let leaves = children
            .iter()
            .map(|&c| self.nodes[c as usize].leaves)
            .sum();
        for &c in &children {
            self.nodes[c as usize].parent = id;
        }
        for &m in &self.comps.members[component] {
            self.node_of[m as usize] = id;
        }
        let sum_all = self.comps.sum_all[component];
        let size = size as i64;
        let nb_all = size * (size - 1) / 2;
        let mut sum_intra = 0.0;
        for &c in &children {
            sum_intra += self.nodes[c as usize].sum_all;
        }
        let sum_inter = sum_all - sum_intra;
        let parts = &self.comps.parts[component];
        let mut nb_inter = 0i64;
        for i in 0..parts.len() {
            for j in i + 1..parts.len() {
                nb_inter += parts[i].0 * parts[j].0;
            }
        }
        self.nodes.push(Node {
            children,
            distance: round_distance,
            leaves,
            p_value: -1.0,
            round: round as i32,
            parent: u32::MAX,
            first: component as u32,
            sum_all,
            sum_inter,
            sum_intra: sum_all - sum_inter,
            nb_inter,
            nb_intra: nb_all - nb_inter,
            to_be_checked: true,
        });
        Ok(())
    }

    /// Upstream `newStatCoal`: node p-values and the partition probability.
    fn test_round(
        &mut self,
        first: bool,
        pi_intra_obs: f64,
        s_intra_total: f64,
        nb_intra_total: i64,
    ) -> Result<f64> {
        let length = f64::from(self.settings.sequence_length);
        let altered: Vec<usize> = self
            .comps
            .altered
            .iter()
            .map(|&c| self.node_of[self.comps.members[c as usize][0] as usize] as usize)
            .collect();
        let (mut s_intra_altered, mut nb_intra_altered) = (0.0, 0i64);
        for &node in &altered {
            let node = &mut self.nodes[node];
            nb_intra_altered += node.nb_intra;
            s_intra_altered += node.sum_intra;
            node.p_value = if node.nb_intra == 0 { 1.0 } else { 0.0 };
        }
        if first {
            return Ok(1.0);
        }
        let targets: Vec<Target> = altered
            .iter()
            .map(|&node| {
                let v = &self.nodes[node];
                let pi_inter = if v.nb_inter > 0 {
                    v.sum_inter / v.nb_inter as f64
                } else {
                    0.0
                };
                let pi_intra = if v.nb_intra > 0 {
                    v.sum_intra / v.nb_intra as f64
                } else {
                    0.0
                };
                let leaves = v.leaves as usize;
                let k_obs = v
                    .children
                    .iter()
                    .map(|&c| self.nodes[c as usize].leaves)
                    .max()
                    .unwrap_or(0) as usize;
                let theta = length * pi_inter * leaves as f64 / (2.0 * (leaves as f64 - 1.0));
                Target {
                    node,
                    leaves,
                    k_obs,
                    mutation_rate: theta / 2.0,
                    pi_intra_scaled: pi_intra * length,
                    testable: v.nb_intra > 0,
                }
            })
            .collect();
        let mut successes = 0.0;
        let mut replicates = INITIAL_SIMULATIONS;
        let mut skipped = 0usize;
        let mut part_replicates = 0i64;
        let mut k = 0usize;
        let effort = self.settings.max_simulations;
        let parallel = self.settings.stream == Stream::Parallel;
        // Parallel blocks: replicates [start, start + len) for every target.
        let (mut block_start, mut block_len) = (0usize, 0usize);
        let mut block: Vec<Option<(f64, i64)>> = Vec::new();
        while k < replicates {
            if parallel && k == block_start + block_len {
                block_start = k;
                // Skipping first becomes possible at replicate 500; small blocks
                // there waste little work on nodes that stop early.
                block_len = match k.cmp(&INITIAL_SIMULATIONS) {
                    std::cmp::Ordering::Less => INITIAL_SIMULATIONS - k,
                    std::cmp::Ordering::Equal => 64,
                    std::cmp::Ordering::Greater => (2 * block_len).min(8192),
                }
                .min(effort.max(k + 1) - k);
                block = self.simulate_block(&targets, block_start, block_len);
            }
            let (mut s_intra_partition, mut nb_intra_partition) = (0.0, 0i64);
            for (t, target) in targets.iter().enumerate() {
                if !self.nodes[target.node].to_be_checked {
                    continue;
                }
                let simulated = if parallel {
                    block[t * block_len + (k - block_start)]
                } else {
                    self.coalescent.simulate(
                        target.leaves,
                        target.mutation_rate,
                        target.k_obs,
                        &mut self.rand,
                    )
                };
                let (s_intra, nb_intra) = simulated.ok_or_else(|| {
                    Error("ASAP internal error: inconsistent simulated lineages".into())
                })?;
                let pi_intra_theo = if nb_intra == 0 {
                    0.0
                } else {
                    s_intra / nb_intra as f64
                };
                let node = &mut self.nodes[target.node];
                if target.testable && pi_intra_theo - EPSILON <= target.pi_intra_scaled {
                    node.p_value += 1.0;
                }
                nb_intra_partition += nb_intra;
                s_intra_partition += s_intra;
                if k + 1 > INITIAL_SIMULATIONS
                    && successes >= MIN_SUCCESS
                    && node.p_value >= MIN_SUCCESS
                {
                    skipped += 1;
                    node.to_be_checked = false;
                    node.p_value = (node.p_value + 1.0) / ((k + 1) as f64 + 1.0);
                }
            }
            if skipped == 0 {
                s_intra_partition += (s_intra_total - s_intra_altered) * length;
                nb_intra_partition += nb_intra_total - nb_intra_altered;
                let pi = s_intra_partition / nb_intra_partition as f64;
                part_replicates = k as i64 + 1;
                if pi - EPSILON <= pi_intra_obs * length {
                    successes += 1.0;
                }
            }
            if skipped < targets.len() {
                replicates = (replicates * 2).min(effort);
            } else {
                // Every node is final; further iterations change nothing.
                break;
            }
            k += 1;
        }
        for target in &targets {
            let node = &mut self.nodes[target.node];
            if node.to_be_checked && target.testable {
                node.p_value = (node.p_value + 1.0) / (k as f64 + 1.0);
            }
        }
        Ok((successes + 1.0) / (part_replicates as f64 + 1.0))
    }

    /// Simulations for `targets` x replicates `start..start + len`, each on its
    /// own keyed stream. Targets already final are skipped.
    fn simulate_block(
        &self,
        targets: &[Target],
        start: usize,
        len: usize,
    ) -> Vec<Option<(f64, i64)>> {
        use rayon::prelude::*;
        let active: Vec<usize> = (0..targets.len())
            .filter(|&t| self.nodes[targets[t].node].to_be_checked)
            .collect();
        let largest = active.iter().map(|&t| targets[t].leaves).max().unwrap_or(2);
        let seed = self.settings.seed;
        let m = active.len().max(1);
        // Replicate-major order spreads expensive nodes across workers.
        let results: Vec<Option<(f64, i64)>> = (0..active.len() * len)
            .into_par_iter()
            .with_min_len(16)
            .map_init(
                || Coalescent::new(largest),
                |coalescent, index| {
                    let target = &targets[active[index % m]];
                    let mut rand = Xoshiro::keyed(seed, target.node, start + index / m);
                    coalescent.simulate(
                        target.leaves,
                        target.mutation_rate,
                        target.k_obs,
                        &mut rand,
                    )
                },
            )
            .collect();
        let mut block = vec![Some((0.0, 0)); targets.len() * len];
        for (index, value) in results.into_iter().enumerate() {
            block[active[index % m] * len + index / m] = value;
        }
        block
    }

    /// Upstream `specie_node_recurse`, summed over `roots`.
    fn recursive_count(&self, roots: &[u32], threshold: f64) -> usize {
        let mut stack = roots.to_vec();
        let mut count = 0;
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            if !node.children.is_empty() && node.p_value <= threshold {
                stack.extend(&node.children);
            } else {
                count += 1;
            }
        }
        count
    }
}

fn nothing_good_under(nodes: &[Node], node: usize, threshold: f64) -> bool {
    !nodes[node]
        .children
        .iter()
        .any(|&c| nodes[c as usize].p_value < threshold && nodes[c as usize].leaves > 1)
}

fn leaves_under(nodes: &[Node], node: usize, out: &mut Vec<usize>) {
    let mut stack = vec![node as u32];
    while let Some(n) = stack.pop() {
        let v = &nodes[n as usize];
        if v.children.is_empty() {
            out.push(n as usize);
        } else {
            stack.extend(v.children.iter().rev());
        }
    }
}

/// Upstream `ecrit_rec_esp_sous_node`/`ecrit_rec_esp_cvs`, which share one rule.
fn recursive_subsets(nodes: &[Node], node: usize, threshold: f64, out: &mut Vec<Subset>) {
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let v = &nodes[n];
        if (v.p_value > threshold || v.p_value < 0.0)
            && (v.leaves <= 1 || nothing_good_under(nodes, n, threshold))
        {
            let mut members = Vec::new();
            leaves_under(nodes, n, &mut members);
            out.push(Subset {
                members,
                p_value: v.p_value,
            });
        } else {
            stack.extend(v.children.iter().rev().map(|&c| c as usize));
        }
    }
}

fn subset(nodes: &[Node], node: usize) -> Subset {
    let mut members = Vec::new();
    leaves_under(nodes, node, &mut members);
    Subset {
        members,
        p_value: nodes[node].p_value,
    }
}

/// Upstream `ecrit_fichier_texte` for one partition. Returns the subsets and
/// whether upstream's zero-distance branch would have dropped a specimen.
fn describe(nodes: &[Node], partition: &Partition, threshold: f64) -> (Vec<Subset>, bool) {
    // The partition's components: the highest nodes existing at its round, in
    // component ID order (upstream `listNodes`).
    let round = partition.round as i32;
    let mut current: Vec<usize> = (0..nodes.len())
        .filter(|&v| {
            nodes[v].round <= round
                && nodes
                    .get(nodes[v].parent as usize)
                    .is_none_or(|p| p.round > round)
        })
        .collect();
    current.sort_by_key(|&v| nodes[v].first);
    let mut out = Vec::new();
    let mut dropped = false;
    for n in current {
        let v = &nodes[n];
        if v.p_value == 1.0 || v.p_value > threshold || v.leaves == 1 {
            if v.distance == partition.distance {
                if v.children.is_empty() {
                    // Upstream iterates the (absent) children of a specimen
                    // at a zero merge distance and loses it; keep it here.
                    dropped = true;
                    out.push(subset(nodes, n));
                } else if nothing_good_under(nodes, n, threshold) {
                    out.extend(v.children.iter().map(|&c| subset(nodes, c as usize)));
                } else {
                    for &c in &v.children {
                        recursive_subsets(nodes, c as usize, threshold, &mut out);
                    }
                }
            } else {
                out.push(subset(nodes, n));
            }
        } else {
            for &c in &v.children {
                recursive_subsets(nodes, c as usize, threshold, &mut out);
            }
        }
    }
    (out, dropped)
}

fn compare_rank(a: &Partition, b: &Partition) -> i32 {
    if a.score < b.score {
        return -1;
    }
    if a.score > b.score {
        return 1;
    }
    if a.probability < b.probability {
        return -1;
    }
    if a.probability > b.probability {
        return 1;
    }
    if a.slope > b.slope {
        return -1;
    }
    1
}

fn sort_partitions(
    partitions: &mut Vec<Partition>,
    compare: impl Fn(&Partition, &Partition) -> i32,
) {
    let mut order: Vec<usize> = (0..partitions.len()).collect();
    glibc_merge_sort(&mut order, &|&a, &b| {
        compare(&partitions[a], &partitions[b])
    });
    *partitions = order.into_iter().map(|i| partitions[i].clone()).collect();
}

/// Run ASAP on a complete distance matrix. `progress(done, total)` is called
/// after each tested partition; returning `false` cancels the run.
pub fn analyse(
    distances: &Distances<'_>,
    settings: &Settings,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<Analysis> {
    let n = distances.n;
    if n < 2 {
        return Err(Error("ASAP needs at least two specimens".into()));
    }
    if n > u32::MAX as usize / 2 || distances.values.len() != n * (n - 1) / 2 {
        return Err(Error("ASAP distance matrix has the wrong size".into()));
    }
    if let Some(bad) = distances
        .values
        .iter()
        .position(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(Error(format!(
            "ASAP needs a finite nonnegative distance for every pair; pair {bad} has {}",
            distances.values[bad]
        )));
    }
    if settings.sequence_length == 0 || settings.max_simulations == 0 || settings.reported == 0 {
        return Err(Error(
            "ASAP sequence length, simulation limit and reported count must be positive".into(),
        ));
    }
    // Upstream `compareCase` is a total order, so any construction order and
    // sort give its result. Adding 0.0 turns -0.0 into 0.0, which upstream
    // compares equal; `total_cmp` would otherwise order them.
    let mut pairs: Vec<Pair> = {
        use rayon::prelude::*;
        let values = distances.values;
        (1..n as u32)
            .into_par_iter()
            .flat_map_iter(|b| {
                let row = b as usize * (b as usize - 1) / 2;
                (0..b).map(move |a| Pair {
                    d: values[row + a as usize] + 0.0,
                    a,
                    b,
                })
            })
            .collect()
    };
    {
        use rayon::slice::ParallelSliceMut;
        pairs.par_sort_unstable_by(|x, y| {
            x.d.total_cmp(&y.d).then(x.a.cmp(&y.a)).then(x.b.cmp(&y.b))
        });
    }
    let leaves = (0..n)
        .map(|i| Node {
            children: Vec::new(),
            distance: 0.0,
            leaves: 1,
            p_value: -1.0,
            round: -1,
            parent: u32::MAX,
            first: i as u32,
            sum_all: 0.0,
            sum_inter: 0.0,
            sum_intra: 0.0,
            nb_inter: 0,
            nb_intra: 0,
            to_be_checked: true,
        })
        .collect::<Vec<_>>();
    let mut run = Run {
        settings,
        nodes: leaves,
        node_of: (0..n as u32).collect(),
        comps: Components::new(n),
        coalescent: Coalescent::new(n),
        rand: GlibcRand::new(settings.seed),
    };
    run.nodes.reserve(n - 1);
    let threshold = f64::from(settings.p_threshold);
    let mut partitions: Vec<Partition> = Vec::new();
    let (mut s_intra, mut nb_intra) = (0.0f64, 0i64);
    let mut i = 0;
    while i < pairs.len() && run.comps.count > 1 {
        let start = i;
        let dist = pairs[i].d;
        let (mut s_inter, mut nb_inter) = (0.0f64, 0i64);
        let mut merged = false;
        while i < pairs.len() && pairs[i].d == dist {
            merged |= run.comps.merge(
                pairs[i].a,
                pairs[i].b,
                distances,
                &mut s_inter,
                &mut nb_inter,
            );
            i += 1;
        }
        if merged {
            let round = partitions.len();
            let previous = partitions.last().map_or(0.0, |p| p.distance);
            for c in run.comps.altered.clone() {
                run.new_node(c as usize, round, dist)?;
            }
            // Upstream computes these in single precision.
            let pi_intra = if nb_intra != 0 {
                (s_intra as f32) / (nb_intra as f32)
            } else {
                0.0
            };
            let probability = run.test_round(round == 0, f64::from(pi_intra), s_intra, nb_intra)?;
            let species = partitions.last().map_or(n, |p| p.groups);
            let mut roots = Vec::new();
            for c in 0..n {
                if let Some(&first) = run.comps.members[c].first() {
                    let current = run.node_of[first as usize];
                    let node = &run.nodes[current as usize];
                    if node.distance == dist {
                        roots.extend(&node.children);
                    } else {
                        roots.push(current);
                    }
                }
            }
            let subsets = run.recursive_count(&roots, threshold).max(species);
            partitions.push(Partition {
                distance: dist,
                threshold: f64::midpoint(dist, previous),
                groups: run.comps.count,
                species,
                subsets,
                probability,
                probability_rank: 0,
                slope: slope(&pairs, start, settings.slope_weight),
                slope_rank: 0,
                score: 0.0,
                rank_general: 0,
                subsets_detail: None,
                round,
            });
            s_intra += s_inter;
            nb_intra += nb_inter;
            if !progress(partitions.len(), n - 1) {
                return Err(Error("ASAP cancelled; no completed hypotheses".into()));
            }
        }
        run.comps.clear_round();
    }
    let mut diagnostics = Vec::new();
    sort_partitions(&mut partitions, |a, b| {
        if a.probability < b.probability { -1 } else { 1 }
    });
    for (rank, p) in partitions.iter_mut().enumerate() {
        p.probability_rank = rank + 1;
    }
    sort_partitions(
        &mut partitions,
        |a, b| if a.slope > b.slope { -1 } else { 1 },
    );
    let weight = f64::from(settings.score_weight);
    for (rank, p) in partitions.iter_mut().enumerate() {
        p.slope_rank = rank + 1;
        p.score = p.slope_rank as f64 * (1.0 - weight) + p.probability_rank as f64 * weight;
    }
    sort_partitions(&mut partitions, compare_rank);
    for (rank, p) in partitions.iter_mut().enumerate() {
        p.rank_general = rank + 1;
    }
    // Upstream sorts by group count for its figures, then by score again; with
    // full ties the second score sort can differ from the first.
    sort_partitions(
        &mut partitions,
        |a, b| if a.groups > b.groups { -1 } else { 1 },
    );
    sort_partitions(&mut partitions, compare_rank);
    if partitions
        .iter()
        .enumerate()
        .any(|(i, p)| p.rank_general != i + 1)
    {
        diagnostics.push(
            "Fully tied ASAP scores were reordered by the reference's second sort; its SPART rank labels are ambiguous for them"
                .into(),
        );
    }
    let reported = settings.reported.min(partitions.len());
    for p in partitions.iter_mut().take(reported) {
        let (subsets, dropped) = describe(&run.nodes, p, threshold);
        if dropped {
            diagnostics.push(format!(
                "ASAP partition at distance {} keeps specimens the reference omits at zero distance",
                p.distance
            ));
        }
        if subsets.len() != p.subsets {
            diagnostics.push(format!(
                "ASAP partition at distance {} has {} described subsets but a recursive count of {}",
                p.distance,
                subsets.len(),
                p.subsets
            ));
        }
        p.subsets_detail = Some(subsets);
    }
    let root = run.nodes.len() - 1;
    let mut display_order = Vec::with_capacity(n);
    leaves_under(&run.nodes, root, &mut display_order);
    let range = pairs
        .first()
        .zip(pairs.last())
        .map_or((0.0, 0.0), |(a, b)| (a.d, b.d));
    Ok(Analysis {
        settings: settings.clone(),
        partitions,
        nodes: run.nodes,
        display_order,
        distance_range: range,
        diagnostics,
    })
}

/// C `%e`-style exponent: at least two digits with an explicit sign.
fn c_exp(value: f64, precision: usize) -> String {
    let text = format!("{value:.precision$e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            let (sign, digits) = exponent
                .strip_prefix('-')
                .map_or(("+", exponent), |d| ("-", d));
            format!("{mantissa}e{sign}{digits:0>2}")
        }
        None => text,
    }
}

impl Analysis {
    /// Reference `.res.cvs` table: every partition in rank order.
    pub fn results_table(&self) -> String {
        let mut out = "Partition rank\tNbSubset\tAsap score\tp-val\tpval-rank\tW\tW rank\tTreshold distance\n".to_string();
        for (i, p) in self.partitions.iter().enumerate() {
            writeln!(
                out,
                "{}\t{}\t{:2.2}\t{}\t{}\t{:.6}\t{}\t{:.6}",
                i + 1,
                p.subsets,
                p.score,
                c_exp(p.probability, 6),
                p.probability_rank,
                p.slope,
                p.slope_rank,
                p.threshold
            )
            .expect("write to string");
        }
        out
    }

    /// Full-precision table for exact comparisons.
    pub fn exact_table(&self) -> String {
        let mut out = "position\trank_general\tdistance\tthreshold\tgroups\tspecies\tsubsets\tprobability\tprobability_rank\tslope\tslope_rank\tscore\n".to_string();
        for (i, p) in self.partitions.iter().enumerate() {
            writeln!(
                out,
                "{}\t{}\t{:e}\t{:e}\t{}\t{}\t{}\t{:e}\t{}\t{:e}\t{}\t{:e}",
                i + 1,
                p.rank_general,
                p.distance,
                p.threshold,
                p.groups,
                p.species,
                p.subsets,
                p.probability,
                p.probability_rank,
                p.slope,
                p.slope_rank,
                p.score
            )
            .expect("write to string");
        }
        out
    }

    /// Node p-values in creation order, for exact comparisons.
    pub fn node_table(&self, n: usize) -> String {
        let mut out = "node\tleaves\tdistance\tp_value\n".to_string();
        for (i, v) in self.nodes.iter().enumerate().skip(n) {
            writeln!(out, "{i}\t{}\t{:e}\t{:e}", v.leaves, v.distance, v.p_value)
                .expect("write to string");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glibc_rand_stream() {
        // glibc srand(1)/rand() reference values.
        let mut rand = GlibcRand::new(1);
        let first: Vec<i32> = (0..5).map(|_| rand.next_i32()).collect();
        assert_eq!(
            first,
            [
                1_804_289_383,
                846_930_886,
                1_681_692_777,
                1_714_636_915,
                1_957_747_793
            ]
        );
        let mut zero = GlibcRand::new(0);
        assert_eq!(zero.next_i32(), 1_804_289_383);
    }

    #[test]
    fn glibc_high_unsigned_seeds() {
        // Captured from glibc srand(unsigned int)/rand(), not our recurrence.
        for (seed, expected) in [
            (
                2_147_483_648,
                [
                    1_336_741_213,
                    1_210_407_648,
                    1_447_044_896,
                    337_392_383,
                    82_502_902,
                ],
            ),
            (
                u32::MAX,
                [
                    254_925_627,
                    1_205_188_300,
                    366_127_624,
                    1_401_405_153,
                    76_053_476,
                ],
            ),
        ] {
            let mut rand = GlibcRand::new(seed);
            assert_eq!(
                std::array::from_fn::<_, 5, _>(|_| rand.next_i32()),
                expected
            );
        }
    }

    /// glibc `random_r` written as its pointer form, for cross-checking blocks.
    fn random_r(seed: u32, count: usize) -> Vec<i32> {
        let mut s = [0u32; 31];
        s[0] = seed;
        // glibc stores the unsigned seed in an int32_t before seeding.
        let mut word = i64::from(seed as i32);
        for v in s.iter_mut().skip(1) {
            let (hi, lo) = (word / 127_773, word % 127_773);
            word = 16_807 * lo - 2_836 * hi;
            if word < 0 {
                word += 2_147_483_647;
            }
            *v = word as u32;
        }
        let (mut f, mut r) = (3, 0);
        let mut step = || {
            s[f] = s[f].wrapping_add(s[r]);
            let out = (s[f] >> 1) as i32;
            f += 1;
            if f >= 31 {
                f = 0;
                r += 1;
            } else {
                r += 1;
                if r >= 31 {
                    r = 0;
                }
            }
            out
        };
        for _ in 0..310 {
            step();
        }
        (0..count).map(|_| step()).collect()
    }

    #[test]
    fn batched_stream_matches_random_r() {
        for seed in [1, 2, 12_345, 2_147_483_647] {
            let mut rand = GlibcRand::new(seed);
            let batched: Vec<i32> = (0..3 * BLOCK + 17).map(|_| rand.next_i32()).collect();
            assert_eq!(batched, random_r(seed, 3 * BLOCK + 17));
        }
    }

    #[test]
    fn glibc_merge_order_for_inconsistent_comparators() {
        let mut values: Vec<usize> = (0..5).collect();
        glibc_merge_sort(&mut values, &|_, _| 1);
        assert_eq!(values, [4, 3, 2, 1, 0]);
        let mut values = vec![3, 1, 2, 1, 0];
        glibc_merge_sort(&mut values, &|a: &i32, b: &i32| if a < b { -1 } else { 1 });
        assert_eq!(values, [0, 1, 1, 2, 3]);
    }

    fn condensed(full: &[&[f64]]) -> Vec<f64> {
        let mut out = Vec::new();
        for i in 1..full.len() {
            out.extend_from_slice(&full[i][..i]);
        }
        out
    }

    #[test]
    fn partitions_cover_every_specimen() {
        let rows: [&[f64]; 6] = [
            &[0.0],
            &[0.01, 0.0],
            &[0.02, 0.015, 0.0],
            &[0.2, 0.21, 0.19, 0.0],
            &[0.22, 0.2, 0.21, 0.01, 0.0],
            &[0.23, 0.2, 0.2, 0.02, 0.0, 0.0],
        ];
        let values = condensed(&rows);
        let distances = Distances {
            n: 6,
            values: &values,
        };
        let settings = Settings {
            sequence_length: 500,
            max_simulations: 2_000,
            ..Settings::default()
        };
        let run = analyse(&distances, &settings, |_, _| true).unwrap();
        let again = analyse(&distances, &settings, |_, _| true).unwrap();
        assert_eq!(run.exact_table(), again.exact_table());
        assert_eq!(run.nodes.len(), 11);
        let mut order = run.display_order.clone();
        order.sort_unstable();
        assert_eq!(order, (0..6).collect::<Vec<_>>());
        for partition in run
            .partitions
            .iter()
            .filter_map(|p| p.subsets_detail.as_ref())
        {
            let mut members: Vec<usize> =
                partition.iter().flat_map(|s| s.members.clone()).collect();
            members.sort_unstable();
            assert_eq!(members, (0..6).collect::<Vec<_>>());
        }
        let ranks: Vec<usize> = run.partitions.iter().map(|p| p.rank_general).collect();
        assert_eq!(ranks, (1..=run.partitions.len()).collect::<Vec<_>>());
    }

    #[test]
    fn parallel_stream_does_not_depend_on_thread_count() {
        // Two loose clusters of eight with distinct distances.
        let n = 16;
        let mut values = Vec::new();
        for i in 1..n {
            for j in 0..i {
                let same = (i < 8) == (j < 8);
                let base = if same { 0.01 } else { 0.12 };
                values.push(base + (i * 31 + j * 7) as f64 * 1e-5);
            }
        }
        let distances = Distances { n, values: &values };
        let settings = Settings {
            sequence_length: 600,
            max_simulations: 3_000,
            stream: Stream::Parallel,
            ..Settings::default()
        };
        let run = |threads| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| analyse(&distances, &settings, |_, _| true).unwrap())
        };
        let (one, four) = (run(1), run(4));
        assert_eq!(one.exact_table(), four.exact_table());
        assert_eq!(one.node_table(n), four.node_table(n));
        let reference = analyse(
            &distances,
            &Settings {
                stream: Stream::Reference,
                ..settings.clone()
            },
            |_, _| true,
        )
        .unwrap();
        // Same clustering and slopes; only simulated p-values may differ.
        assert_eq!(one.partitions.len(), reference.partitions.len());
        assert_eq!(one.nodes.len(), reference.nodes.len());
    }

    #[test]
    fn slope_past_the_last_pair_reads_zero() {
        let pairs = [
            Pair { d: 0.1, a: 0, b: 1 },
            Pair {
                d: 0.105,
                a: 0,
                b: 2,
            },
        ];
        // Every remaining distance is within the window above 0.105.
        let w = slope(&pairs, 1, 0.1);
        assert_eq!(w, (0.0 - 0.1) / ((1.0 + 0.0 + 0.1) * 2.0));
    }

    #[test]
    fn rejects_missing_distances() {
        let values = [0.1, f64::NAN, 0.2];
        let distances = Distances {
            n: 3,
            values: &values,
        };
        assert!(analyse(&distances, &Settings::default(), |_, _| true).is_err());
    }

    #[test]
    fn cancellation_stops_the_run() {
        let values = [0.1, 0.2, 0.3];
        let distances = Distances {
            n: 3,
            values: &values,
        };
        let error = analyse(&distances, &Settings::default(), |_, _| false).unwrap_err();
        assert!(error.0.contains("cancelled"));
    }
}
