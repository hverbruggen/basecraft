//! Exact sorted-row NJ; stable slots, timestamp-owned matrix rows, compact sorted entries.
use super::{Best, Instant, Packed, Stats, map};
use rayon::prelude::*;
#[derive(Clone, Copy)]
struct Item {
    d: f64,
    j: u32,
    generation: u32,
}
fn value(d: &[f64], g: &[u32], n: usize, i: usize, j: usize) -> f64 {
    if g[i] >= g[j] {
        d[i * n + j]
    } else {
        d[j * n + i]
    }
}
fn candidate(v: f64, s: &[f64], m: usize, i: usize, j: usize) -> Best {
    let (i, j) = (i.min(j), i.max(j));
    Best {
        q: (m - 2) as f64 * v - s[i] - s[j],
        i,
        j,
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
// Slot casts are guarded by n < u32::MAX; preserve reference arithmetic order.
#[allow(clippy::cast_possible_truncation, clippy::manual_midpoint)]
pub(super) fn tree(
    mut d: Vec<f64>,
    names: &[String],
    parallel: bool,
    cutoff: usize,
    verify: bool,
) -> (String, Stats) {
    let n = names.len();
    assert!(n >= 3 && n < u32::MAX as usize);
    let init = Instant::now();
    let mut st = Stats::default();
    let mut ids: Vec<_> = (0..n).collect();
    let mut active = vec![true; n];
    let mut g = vec![0u32; n];
    let mut s = map(n, parallel, |i| d[i * n..(i + 1) * n].iter().sum::<f64>());
    let mut rows = map(n, parallel, |i| {
        let mut r: Vec<_> = (0..i)
            .map(|j| Item {
                d: d[i * n + j],
                j: j as u32,
                generation: 0,
            })
            .collect();
        r.sort_unstable_by(|a, b| a.d.total_cmp(&b.d).then(a.j.cmp(&b.j)));
        r
    });
    let mut nodes: Vec<_> = names
        .iter()
        .map(|s| Node {
            text: Some(format!("'{}'", s.replace('\'', "''"))),
            edges: vec![],
        })
        .collect();
    let mut roots: Vec<_> = (0..n).collect();
    let mut starts = vec![0; n];
    let mut dense = false;
    let mut symmetric = false;
    let mut spare = Vec::with_capacity(n);
    let mut updates = Vec::with_capacity(n);
    st.init_s = init.elapsed().as_secs_f64();
    while ids.len() > 3 {
        let m = ids.len();
        let start = Instant::now();
        let max_s = ids.iter().map(|&i| s[i]).fold(f64::NEG_INFINITY, f64::max);
        // One first live entry per row gives a stronger inexpensive incumbent than a fixed full row.
        let mut seed = Best::none();
        for &i in &ids {
            while starts[i] < rows[i].len() {
                let e = &rows[i][starts[i]];
                if active[e.j as usize] && g[e.j as usize] == e.generation {
                    break;
                }
                starts[i] += 1;
            }
            if let Some(e) = rows[i].get(starts[i]) {
                seed = seed.min(candidate(e.d, &s, m, i, e.j as usize));
                st.evaluated += 1;
            }
        }
        let scan = |chunk: &[usize]| {
            let mut b = seed;
            let mut count = 0u64;
            for &i in chunk {
                for e in &rows[i][starts[i]..] {
                    // Bound valid even for stale entries; check it before chasing metadata.
                    if (m - 2) as f64 * e.d - s[i] - max_s > b.q + 1e-11 * (1. + b.q.abs()) {
                        break;
                    }
                    let j = e.j as usize;
                    if active[j] && g[j] == e.generation {
                        b = b.min(candidate(e.d, &s, m, i, j));
                        count += 1;
                    }
                }
            }
            (b, count)
        };
        // Poor pruning (e.g. a star) is cheaper as a contiguous exhaustive scan.
        // Probe sorted search every eighth join so this decision can reverse.
        let dense_round = dense && !(n - m).is_multiple_of(8);
        let dense_scan = |chunk: &[usize]| {
            let mut b = seed;
            let mut count = 0;
            for &i in chunk {
                for &j in &ids {
                    if j >= i {
                        break;
                    }
                    b = b.min(candidate(d[i * n + j], &s, m, i, j));
                    count += 1;
                }
            }
            (b, count)
        };
        let use_parallel = parallel && (m >= cutoff || (dense_round && m >= 128));
        let chunk = (m / (rayon::current_num_threads() * 4)).max(16);
        let (best, count) = if dense_round {
            if use_parallel {
                ids.par_chunks(chunk)
                    .map(dense_scan)
                    .reduce(|| (Best::none(), 0), |a, b| (a.0.min(b.0), a.1 + b.1))
            } else {
                dense_scan(&ids)
            }
        } else if use_parallel {
            ids.par_chunks(chunk)
                .map(scan)
                .reduce(|| (Best::none(), 0), |a, b| (a.0.min(b.0), a.1 + b.1))
        } else {
            scan(&ids)
        };
        if !dense_round {
            dense = count as f64 > 0.15 * (m * (m - 1) / 2) as f64;
        }
        if dense_round {
            st.skipped += 1;
        }
        if dense && !symmetric {
            for (k, &i) in ids.iter().enumerate() {
                for &j in &ids[..k] {
                    let v = value(&d, &g, n, i, j);
                    d[i * n + j] = v;
                    d[j * n + i] = v;
                }
            }
            symmetric = true;
        }
        st.evaluated += count;
        st.rows += m as u64;
        st.search_s += start.elapsed().as_secs_f64();
        if verify {
            let mut reference = Best::none();
            for (k, &i) in ids.iter().enumerate() {
                for &j in &ids[..k] {
                    reference = reference.min(candidate(value(&d, &g, n, i, j), &s, m, i, j));
                }
            }
            let error = best.q - reference.q;
            st.max_q_error = st.max_q_error.max(error);
            if error > 1e-10 * (1. + reference.q.abs()) {
                st.mismatches += 1;
            }
        }
        let start = Instant::now();
        let (x, y) = (best.i, best.j);
        let xy = value(&d, &g, n, x, y);
        let limb = 0.5 * xy + (s[x] - s[y]) / (2. * (m - 2) as f64);
        nodes.push(Node {
            text: None,
            edges: vec![(roots[x], limb), (roots[y], xy - limb)],
        });
        roots[x] = nodes.len() - 1;
        updates.clear();
        spare.clear();
        for &i in &ids {
            if i != x && i != y {
                let dx = value(&d, &g, n, i, x);
                let dy = value(&d, &g, n, i, y);
                let v = 0.5 * (dx + dy) - 0.5 * xy;
                s[i] += v - dx - dy;
                updates.push((i, v));
                spare.push(Item {
                    d: v,
                    j: i as u32,
                    generation: g[i],
                });
            }
        }
        // Only the newest row owns these distances. No strided symmetric writes.
        for &(i, v) in &updates {
            d[x * n + i] = v;
            if symmetric {
                d[i * n + x] = v;
            }
        }
        s[x] = updates.iter().map(|v| v.1).sum();
        g[x] = (n - m + 1) as u32;
        active[y] = false;
        ids.retain(|&i| i != y);
        if parallel && spare.len() >= 4096 {
            spare.par_sort_unstable_by(|a, b| a.d.total_cmp(&b.d).then(a.j.cmp(&b.j)));
        } else {
            spare.sort_unstable_by(|a, b| a.d.total_cmp(&b.d).then(a.j.cmp(&b.j)));
        }
        std::mem::swap(&mut rows[x], &mut spare);
        rows[y] = Vec::new();
        starts[x] = 0;
        starts[y] = 0;
        st.update_s += start.elapsed().as_secs_f64();
    }
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let ab = value(&d, &g, n, a, b);
    let ac = value(&d, &g, n, a, c);
    let bc = value(&d, &g, n, b, c);
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
