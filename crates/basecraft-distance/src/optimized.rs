//! Exact NJ search with cached row bounds, on one triangular matrix.
//!
//! Dynamic NJ (Clausen 2023, Bioinformatics 39:btac774) caches each row's smallest Q and
//! rescans only rows that could hold the next minimum. Its proof needs the triangle
//! inequality, which corrected and pairwise-deleted distances can break. This guard is our
//! addition, and README.md lists every source. After joining x and y, every surviving
//! pair changes by exactly the first line below, so for any finite matrix row r's old
//! pairs change by at least the second:
//!
//! ```text
//! Q'_ij - Q_ij = a_i + a_j + D_xy - D_ij, where a_i = (D_ix + D_iy) / 2
//! a_r + min(a_j, j < r) + D_xy - max(D_rj, j < r)
//! ```
//!
//! Lowering the cached bound by that amount when it is negative keeps it valid without
//! assuming a metric. Results are the same joins as an exhaustive search over every
//! pair, including ties, which break on (Q, smaller slot, larger slot).
use super::{Best, Instant, Packed, Stats, index};
use rayon::prelude::*;
const NO_PARTNER: usize = usize::MAX;
const BLOCK: usize = 64;
const LANES: usize = 4;
// Enough rows to pay for a parallel batch.
const PARALLEL_ENTRIES: usize = 1 << 17;
struct RowScan {
    q: f64,
    j: usize,
    d: f64,
    max_d: f64,
}
/// Smallest Q in one row, lowest slot on a tie. Dead partners have s = -inf, so Q = +inf.
// Comparing Q exactly is the point: it finds the entry that produced the minimum.
#[allow(clippy::float_cmp)]
fn scan_row(row: &[f64], s: &[f64], c: f64, sr: f64) -> RowScan {
    let mut best = f64::INFINITY;
    let mut best_block = usize::MAX;
    let mut max_d = f64::NEG_INFINITY;
    for (k, (ds, ss)) in row
        .chunks(BLOCK)
        .zip(s[..row.len()].chunks(BLOCK))
        .enumerate()
    {
        let mut low = [f64::INFINITY; LANES];
        let mut high = [f64::NEG_INFINITY; LANES];
        let (d_lanes, d_tail) = ds.as_chunks::<LANES>();
        let (s_lanes, s_tail) = ss.as_chunks::<LANES>();
        for (d, si) in d_lanes.iter().zip(s_lanes) {
            for l in 0..LANES {
                let q = c * d[l] - si[l] - sr;
                low[l] = if q < low[l] { q } else { low[l] };
                high[l] = if d[l] > high[l] { d[l] } else { high[l] };
            }
        }
        let mut block = low.into_iter().fold(f64::INFINITY, f64::min);
        max_d = high.into_iter().fold(max_d, f64::max);
        for (&d, &si) in d_tail.iter().zip(s_tail) {
            block = block.min(c * d - si - sr);
            max_d = max_d.max(d);
        }
        if block < best {
            best = block;
            best_block = k;
        }
    }
    if best_block == usize::MAX {
        return RowScan {
            q: f64::INFINITY,
            j: NO_PARTNER,
            d: f64::NAN,
            max_d,
        };
    }
    let first = best_block * BLOCK;
    let end = (first + BLOCK).min(row.len());
    let j = (first..end)
        .find(|&j| c * row[j] - s[j] - sr == best)
        .unwrap_or(first);
    RowScan {
        q: best,
        j,
        d: row[j],
        max_d,
    }
}
// Arena avoids copying a growing Newick subtree after every ladder join.
struct Node {
    text: Option<String>,
    edges: Vec<(usize, f64)>,
}
fn emit(nodes: &[Node], root: usize) -> String {
    enum Event {
        Node(usize),
        Text(String),
    }
    let mut stack = vec![Event::Node(root)];
    let mut out = String::new();
    while let Some(e) = stack.pop() {
        match e {
            Event::Text(s) => out.push_str(&s),
            Event::Node(i) => {
                if let Some(s) = &nodes[i].text {
                    out.push_str(s);
                } else {
                    stack.push(Event::Text(")".into()));
                    for (k, &(j, len)) in nodes[i].edges.iter().enumerate().rev() {
                        stack.push(Event::Text(format!(":{len:.12}")));
                        stack.push(Event::Node(j));
                        if k > 0 {
                            stack.push(Event::Text(",".into()));
                        }
                    }
                    stack.push(Event::Text("(".into()));
                }
            }
        }
    }
    out.push_str(";\n");
    out
}
// Row sums accumulated in slot order, so rounding is independent of the matrix layout.
fn row_sums(d: &[f64], n: usize) -> Vec<f64> {
    let mut s = vec![0.; n];
    for r in 1..n {
        let row = &d[index(r, 0)..index(r, 0) + r];
        let (before, rest) = s.split_at_mut(r);
        let mut own = rest[0];
        for (sum, &v) in before.iter_mut().zip(row) {
            own += v;
            *sum += v;
        }
        rest[0] = own;
    }
    s
}
// Slot casts are guarded by n < u32::MAX; preserve reference arithmetic order.
#[allow(clippy::manual_midpoint)]
pub(super) fn tree(
    mut d: Vec<f64>,
    names: &[String],
    parallel: bool,
    cutoff: usize,
    verify: bool,
) -> (String, Stats) {
    let n = names.len();
    assert!(n >= 3 && n < u32::MAX as usize && d.len() == n * (n - 1) / 2);
    let init = Instant::now();
    let mut st = Stats::default();
    let mut s = row_sums(&d, n);
    let mut nodes: Vec<_> = names
        .iter()
        .map(|s| Node {
            text: Some(format!("'{}'", s.replace('\'', "''"))),
            edges: vec![],
        })
        .collect();
    let mut roots: Vec<_> = (0..n).collect();
    // Lower bound on row r's smallest Q. Each pair lives in its larger slot's row.
    let mut lower = vec![f64::NEG_INFINITY; n];
    lower[0] = f64::INFINITY;
    let mut partner = vec![NO_PARTNER; n];
    let mut partner_d = vec![0.; n];
    let mut max_d = vec![f64::INFINITY; n];
    // The cached partner is known to hold the row minimum this round.
    let mut exact = vec![false; n];
    let mut new_d: Vec<(usize, f64)> = Vec::with_capacity(n);
    let mut wanted: Vec<(f64, usize)> = Vec::with_capacity(n);
    let mut d_limit = d.iter().fold(0_f64, |a, &b| a.max(b.abs()));
    let mut slots = n;
    let mut m = n;
    let batch = rayon::current_num_threads() * 2;
    st.init_s = init.elapsed().as_secs_f64();
    while m > 3 {
        let start = Instant::now();
        let c = (m - 2) as f64;
        // Rounding margin: each Q and bound step is within a few ulps of the largest Q.
        let s_limit = s
            .iter()
            .filter(|v| v.is_finite())
            .fold(0_f64, |a, &b| a.max(b.abs()));
        let pad = 8. * f64::EPSILON * (1. + c * d_limit + 2. * s_limit);
        let mut best = Best::none();
        for r in 0..slots {
            let p = partner[r];
            if p != NO_PARTNER {
                let q = c * partner_d[r] - s[p] - s[r];
                if q <= best.q {
                    best = best.min(Best { q, i: p, j: r });
                }
            }
        }
        wanted.clear();
        for r in 0..slots {
            let bound = lower[r];
            if bound <= best.q + pad && bound < f64::INFINITY && !exact[r] {
                wanted.push((bound, r));
            }
        }
        wanted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let view = &d;
        let sums = &s;
        let scan = |r: usize| scan_row(&view[index(r, 0)..index(r, 0) + r], sums, c, sums[r]);
        let mut next = 0;
        while next < wanted.len() && wanted[next].0 <= best.q + pad {
            let entries: usize = wanted[next..(next + batch).min(wanted.len())]
                .iter()
                .map(|w| w.1)
                .sum();
            let found: Vec<(usize, RowScan)> =
                if parallel && m >= cutoff && (cutoff == 0 || entries >= PARALLEL_ENTRIES) {
                    wanted[next..(next + batch).min(wanted.len())]
                        .par_iter()
                        .map(|&(_, r)| (r, scan(r)))
                        .collect()
                } else {
                    vec![(wanted[next].1, scan(wanted[next].1))]
                };
            next += found.len();
            for (r, row) in found {
                st.evaluated += r as u64;
                st.rows += 1;
                lower[r] = row.q;
                partner[r] = row.j;
                partner_d[r] = row.d;
                max_d[r] = row.max_d;
                if row.j != NO_PARTNER {
                    best = best.min(Best {
                        q: row.q,
                        i: row.j,
                        j: r,
                    });
                }
            }
        }
        st.search_s += start.elapsed().as_secs_f64();
        if verify {
            let mut reference = Best::none();
            for j in 0..slots {
                for i in 0..j {
                    if s[i].is_finite() && s[j].is_finite() {
                        let q = c * d[index(j, i)] - s[i] - s[j];
                        reference = reference.min(Best { q, i, j });
                    }
                }
            }
            st.max_q_error = st.max_q_error.max((best.q - reference.q).abs());
            if best.q.to_bits() != reference.q.to_bits()
                || best.i != reference.i
                || best.j != reference.j
            {
                st.mismatches += 1;
            }
        }
        let start = Instant::now();
        let (x, y) = (best.i, best.j);
        let xy = d[index(y, x)];
        let limb = 0.5 * xy + (s[x] - s[y]) / (2. * (m - 2) as f64);
        nodes.push(Node {
            text: None,
            edges: vec![(roots[x], limb), (roots[y], xy - limb)],
        });
        roots[x] = nodes.len() - 1;
        new_d.clear();
        let mut a_min = f64::INFINITY;
        for r in 0..slots {
            if r == x || r == y || s[r] == f64::NEG_INFINITY {
                continue;
            }
            let at_x = index(r, x);
            let dx = d[at_x];
            let dy = d[index(r, y)];
            let a = 0.5 * (dx + dy);
            let v = a - 0.5 * xy;
            s[r] += v - dx - dy;
            d[at_x] = v;
            d_limit = d_limit.max(v.abs());
            new_d.push((r, v));
            // Old pairs of row r are its partners below r, other than x and y.
            let step = a + a_min + xy - max_d[r];
            if step < 0. {
                lower[r] += step;
            }
            lower[r] -= pad;
            exact[r] = false;
            if partner[r] == x || partner[r] == y {
                partner[r] = NO_PARTNER;
            }
            if r > x {
                max_d[r] = max_d[r].max(v);
            }
            a_min = a_min.min(a);
        }
        s[x] = new_d.iter().map(|v| v.1).sum();
        s[y] = f64::NEG_INFINITY;
        lower[y] = f64::INFINITY;
        partner[y] = NO_PARTNER;
        exact[y] = false;
        lower[x] = f64::NEG_INFINITY;
        partner[x] = NO_PARTNER;
        exact[x] = false;
        m -= 1;
        // Pairs with the new cluster are known exactly, so they can settle a row outright.
        let c = (m - 2) as f64;
        for &(r, v) in new_d.iter().filter(|e| e.0 > x) {
            let q = c * v - s[x] - s[r];
            if q < lower[r] - pad {
                exact[r] = true;
                partner[r] = x;
                partner_d[r] = v;
            } else if partner[r] == NO_PARTNER {
                partner[r] = x;
                partner_d[r] = v;
            }
            lower[r] = lower[r].min(q);
        }
        if m * 4 <= slots * 3 && slots >= 64 {
            let keep: Vec<_> = (0..slots).filter(|&r| s[r] != f64::NEG_INFINITY).collect();
            let mut position = vec![NO_PARTNER; slots];
            for (new, &old) in keep.iter().enumerate() {
                position[old] = new;
            }
            let mut w = 0;
            for &r in &keep {
                let offset = index(r, 0);
                for &j in keep.iter().take_while(|&&j| j < r) {
                    d[w] = d[offset + j];
                    w += 1;
                }
            }
            d.truncate(w);
            for (new, &old) in keep.iter().enumerate() {
                s[new] = s[old];
                roots[new] = roots[old];
                lower[new] = lower[old];
                partner[new] = if partner[old] == NO_PARTNER {
                    NO_PARTNER
                } else {
                    position[partner[old]]
                };
                partner_d[new] = partner_d[old];
                max_d[new] = max_d[old];
                exact[new] = exact[old];
            }
            slots = keep.len();
            s.truncate(slots);
        }
        st.update_s += start.elapsed().as_secs_f64();
    }
    let ids: Vec<_> = (0..slots).filter(|&r| s[r] != f64::NEG_INFINITY).collect();
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let ab = d[index(b, a)];
    let ac = d[index(c, a)];
    let bc = d[index(c, b)];
    let la = (ab + ac - bc) / 2.;
    nodes.push(Node {
        text: None,
        edges: vec![(roots[a], la), (roots[b], ab - la), (roots[c], ac - la)],
    });
    (emit(&nodes, nodes.len() - 1), st)
}
pub(super) fn pack_fast(s: &[u8]) -> Packed {
    let mut lut = [4u8; 256];
    for (c, v) in [(b'A', 0), (b'G', 1), (b'C', 2), (b'T', 3), (b'U', 3)] {
        lut[c as usize] = v;
    }
    let mut p = Packed {
        lo: Vec::with_capacity(s.len().div_ceil(64)),
        hi: Vec::with_capacity(s.len().div_ceil(64)),
        valid: Vec::with_capacity(s.len().div_ceil(64)),
    };
    for chunk in s.chunks(64) {
        let (mut lo, mut hi, mut valid) = (0, 0, 0);
        for (k, &c) in chunk.iter().enumerate() {
            let v = lut[c as usize];
            lo |= u64::from(v & 1) << k;
            hi |= u64::from((v >> 1) & 1) << k;
            valid |= u64::from(v < 4) << k;
        }
        p.lo.push(lo);
        p.hi.push(hi);
        p.valid.push(valid);
    }
    p
}
