//! Reusable DNA distances and exact sorted-row neighbor joining.
//! Missing estimates are retained separately from inferred tree distances.
// Counts and matrix indices fit exactly at practical in-memory dataset sizes.
// Mathematical subscripts follow the published NJ formulas.
#![allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
use basecraft_platform::cpu::Words;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
    time::Instant,
};
mod bootstrap;
mod incomplete;
mod optimized;
mod upgma;
// Distinct count triples in one block of barcode data are few, and a logarithm is slow.
const MEMO: usize = 128;
pub use bootstrap::{
    BootstrapOptions, BootstrapPublicationError, BootstrapReport, BootstrapResult,
    ReplicateCollection, bootstrap, partial_collection_path,
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    P,
    Jc69,
    K2p,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Upgma,
    Sorted,
    Fourpoint,
    Njstar,
    Mvrstar,
}
#[derive(Clone, Debug)]
pub struct Alignment {
    pub names: Vec<String>,
    pub sequences: Vec<Vec<u8>>,
}
impl Alignment {
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let mut names = vec![];
        let mut sequences: Vec<Vec<u8>> = vec![];
        for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(name) = line.strip_prefix('>') {
                names.push(name.split_whitespace().next().unwrap_or("").to_string());
                let width = sequences.first().map_or(0, Vec::len);
                sequences.push(Vec::with_capacity(width));
            } else {
                let sequence = sequences
                    .last_mut()
                    .ok_or("sequence before FASTA header")?;
                let bytes = line.as_bytes();
                let start = sequence.len();
                if bytes.iter().any(u8::is_ascii_whitespace) {
                    sequence.extend(bytes.iter().filter(|c| !c.is_ascii_whitespace()));
                } else {
                    sequence.extend_from_slice(bytes);
                }
                sequence[start..].make_ascii_uppercase();
            }
        }
        let a = Self { names, sequences };
        a.validate()?;
        Ok(a)
    }
    pub fn validate(&self) -> Result<()> {
        self.check(false)
    }
    // Parallel checks must run inside a caller's pool, so --threads holds.
    fn check(&self, parallel: bool) -> Result<()> {
        if self.names.is_empty() || self.names.len() != self.sequences.len() {
            return Err("alignment must have named sequences".into());
        }
        let width = self.sequences[0].len();
        let mut seen = HashSet::new();
        if width == 0 || self.sequences.iter().any(|s| s.len() != width) {
            return Err("FASTA must be a nonempty aligned set of sequences".into());
        }
        for name in &self.names {
            if name.is_empty() || name.chars().any(char::is_control) || !seen.insert(name) {
                return Err(
                    "sequence names must be unique, nonempty, and contain no control characters"
                        .into(),
                );
            }
        }
        let mut allowed = [false; 256];
        for &c in b"ACGTUNRYKMSWBDHVX?- ." {
            allowed[usize::from(c)] = true;
        }
        let bad = |s: &Vec<u8>| s.iter().any(|&c| !allowed[usize::from(c)]);
        let found = if parallel {
            self.sequences.par_iter().any(bad)
        } else {
            self.sequences.iter().any(bad)
        };
        if found {
            return Err("DNA/RNA input required; unsupported residue".into());
        }
        Ok(())
    }
    pub fn hash(&self) -> String {
        let mut h = blake3::Hasher::new();
        for (name, seq) in self.names.iter().zip(&self.sequences) {
            h.update(&(name.len() as u64).to_le_bytes());
            h.update(name.as_bytes());
            h.update(&(seq.len() as u64).to_le_bytes());
            h.update(seq);
        }
        h.finalize().to_hex().to_string()
    }
}
#[derive(Clone)]
struct Packed {
    lo: Vec<u64>,
    hi: Vec<u64>,
    valid: Vec<u64>,
}
impl Packed {
    fn words(&self) -> Words<'_> {
        Words {
            lo: &self.lo,
            hi: &self.hi,
            valid: &self.valid,
        }
    }
}
/// Writes every pair of a triangular matrix from pair counts. Row blocks run as tasks and
/// visit partner rows in short runs, which keeps both sides of each comparison in cache.
fn fill_pairs(
    packed: &[Packed],
    model: Model,
    min_overlap: u64,
    (distances, variances, shared_sites): (&mut [f64], &mut [f64], &mut [u64]),
    counts: impl Fn(&Packed, &Packed) -> (u64, u64, u64) + Sync + Send,
) {
    let n = packed.len();
    let parallel = rayon::current_num_threads() > 1;
    let row_bytes = packed.first().map_or(8, |p| p.lo.len() * 24).max(8);
    let run = (128 * 1024 / row_bytes).clamp(8, 512);
    let rows = run.min((n / (rayon::current_num_threads() * 8)).max(4));
    let keep_variances = !variances.is_empty();
    let mut tasks = vec![];
    let (mut ds, mut vs, mut ss) = (distances, variances, shared_sites);
    let mut first = 1;
    while first < n {
        let last = (first + rows).min(n);
        let pairs = index(last, 0) - index(first, 0);
        let (d_head, d_tail) = std::mem::take(&mut ds).split_at_mut(pairs);
        let (v_head, v_tail) = std::mem::take(&mut vs).split_at_mut(if keep_variances { pairs } else { 0 });
        let (s_head, s_tail) = std::mem::take(&mut ss).split_at_mut(pairs);
        (ds, vs, ss) = (d_tail, v_tail, s_tail);
        tasks.push((first, last, d_head, v_head, s_head));
        first = last;
    }
    let fill = |(first, last, ds, vs, ss): (usize, usize, &mut [f64], &mut [f64], &mut [u64])| {
        let base = index(first, 0);
        let mut memo = vec![(u64::MAX, 0, 0, f64::NAN, f64::NAN); MEMO];
        for start in (0..last).step_by(run) {
            for i in first..last {
                let row = index(i, 0) - base;
                for j in start..(start + run).min(i) {
                    let (valid, ts, tv) = counts(&packed[i], &packed[j]);
                    let k = row + j;
                    ss[k] = valid;
                    let slot = &mut memo[(valid as usize)
                        .wrapping_mul(31)
                        .wrapping_add((ts as usize).wrapping_mul(7))
                        .wrapping_add(tv as usize)
                        & (MEMO - 1)];
                    if slot.0 != valid || slot.1 != ts || slot.2 != tv {
                        let (d, v) = if valid >= min_overlap {
                            estimate(model, valid, ts, tv).unwrap_or((f64::NAN, f64::NAN))
                        } else {
                            (f64::NAN, f64::NAN)
                        };
                        *slot = (valid, ts, tv, d, v);
                    }
                    ds[k] = slot.3;
                    if keep_variances {
                        vs[k] = slot.4;
                    }
                }
            }
        }
    };
    if parallel {
        tasks.into_par_iter().for_each(fill);
    } else {
        tasks.into_iter().for_each(fill);
    }
}
fn counts(a: &Packed, b: &Packed) -> (u64, u64, u64) {
    basecraft_platform::cpu::pair_counts()(a.words(), b.words())
}
fn map<T: Send, F: Fn(usize) -> T + Sync + Send>(n: usize, parallel: bool, f: F) -> Vec<T> {
    if parallel {
        (0..n).into_par_iter().map(f).collect()
    } else {
        (0..n).map(f).collect()
    }
}
#[derive(Clone, Copy, Debug)]
struct Best {
    q: f64,
    i: usize,
    j: usize,
}
impl Best {
    fn none() -> Self {
        Self {
            q: f64::INFINITY,
            i: usize::MAX,
            j: usize::MAX,
        }
    }
    fn min(self, b: Self) -> Self {
        if self
            .q
            .total_cmp(&b.q)
            .then(self.i.cmp(&b.i))
            .then(self.j.cmp(&b.j))
            .is_le()
        {
            self
        } else {
            b
        }
    }
}
#[derive(Default)]
struct Stats {
    evaluated: u64,
    rows: u64,
    mismatches: usize,
    max_q_error: f64,
    init_s: f64,
    search_s: f64,
    update_s: f64,
}
/// Triangular observed distances, in row order: (1,0), (2,0), (2,1), ... .
/// JSON null denotes a missing value. This cache is never changed by tree inference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistanceMatrix {
    pub schema: u32,
    pub input_hash: String,
    pub names: Vec<String>,
    pub columns: usize,
    pub model: Model,
    pub min_overlap: u64,
    pub gap_policy: String,
    #[serde(with = "nullable")]
    pub distances: Vec<f64>,
    pub shared_sites: Vec<u64>,
    #[serde(with = "nullable")]
    pub variances: Vec<f64>,
}
mod nullable {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[f64], s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(v.len()))?;
        for x in v {
            seq.serialize_element(&x.is_finite().then_some(*x))?;
        }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<f64>, D::Error> {
        Ok(Vec::<Option<f64>>::deserialize(d)?
            .into_iter()
            .map(|x| x.unwrap_or(f64::NAN))
            .collect())
    }
}
fn index(i: usize, j: usize) -> usize {
    let (x, y) = (i.max(j), i.min(j));
    x * (x - 1) / 2 + y
}
/// Distance and delta-method variance; None means saturation or no comparable sites.
pub fn estimate(
    model: Model,
    valid: u64,
    transitions: u64,
    transversions: u64,
) -> Option<(f64, f64)> {
    if valid == 0 || transitions > valid || transversions > valid - transitions {
        return None;
    }
    let p = transitions as f64 / valid as f64;
    let q = transversions as f64 / valid as f64;
    let mismatch = p + q;
    let (d, var) = match model {
        Model::P => (mismatch, mismatch * (1. - mismatch) / valid as f64),
        Model::Jc69 => {
            let a = 1. - 4. * mismatch / 3.;
            if a <= 0. {
                return None;
            }
            (
                -0.75 * a.ln(),
                mismatch * (1. - mismatch) / (valid as f64 * a * a),
            )
        }
        Model::K2p => {
            let a = 1. - 2. * p - q;
            let b = 1. - 2. * q;
            if a <= 0. || b <= 0. {
                return None;
            }
            let dp = 1. / a;
            let dq = 0.5 / a + 0.5 / b;
            (
                -0.5 * a.ln() - 0.25 * b.ln(),
                (dp * dp * p + dq * dq * q - (dp * p + dq * q).powi(2)) / valid as f64,
            )
        }
    };
    Some((d, var.max(0.25 / (valid as f64).powi(2))))
}
fn pool(threads: usize) -> Result<rayon::ThreadPool> {
    if threads == 0 {
        return Err("threads must be at least one".into());
    }
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?)
}
impl DistanceMatrix {
    pub fn calculate(
        a: &Alignment,
        model: Model,
        min_overlap: u64,
        threads: usize,
    ) -> Result<Self> {
        if min_overlap == 0 {
            return Err("minimum overlap must be at least one".into());
        }
        let workers = pool(threads)?;
        workers.install(|| a.check(threads > 1))?;
        let n = a.names.len();
        let len = n.checked_mul(n - 1).ok_or("too many sequences")? / 2;
        let (distances, variances, shared_sites) = workers.install(|| {
            let packed = map(n, threads > 1, |i| optimized::pack_fast(&a.sequences[i]));
            // Zeroed pages are touched first by the workers that fill them.
            let mut distances = vec![0.; len];
            let mut variances = vec![0.; len];
            let mut shared_sites = vec![0; len];
            let kernel = basecraft_platform::cpu::pair_counts();
            fill_pairs(
                &packed,
                model,
                min_overlap,
                (&mut distances, &mut variances, &mut shared_sites),
                |x, y| kernel(x.words(), y.words()),
            );
            (distances, variances, shared_sites)
        });
        Ok(Self {
            schema: 1,
            input_hash: a.hash(),
            names: a.names.clone(),
            columns: a.sequences[0].len(),
            model,
            min_overlap,
            gap_policy: "pairwise deletion; ACGTU valid; U=T".into(),
            distances,
            variances,
            shared_sites,
        })
    }
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        if i == j {
            return Some(0.);
        }
        let d = self.distances[index(i, j)];
        d.is_finite().then_some(d)
    }
    pub fn validate(&self) -> Result<()> {
        let n = self.names.len();
        let len = n
            .checked_mul(n.saturating_sub(1))
            .ok_or("invalid dimensions")?
            / 2;
        if self.schema != 1
            || n == 0
            || self.columns == 0
            || self.min_overlap == 0
            || self.distances.len() != len
            || self.variances.len() != len
            || self.shared_sites.len() != len
        {
            return Err("invalid distance cache schema or dimensions".into());
        }
        let mut seen = HashSet::new();
        if self
            .names
            .iter()
            .any(|s| s.is_empty() || s.chars().any(char::is_control) || !seen.insert(s))
        {
            return Err("invalid cache labels".into());
        }
        for k in 0..len {
            let d = self.distances[k];
            let v = self.variances[k];
            if self.shared_sites[k] > self.columns as u64
                || d.is_infinite()
                || v.is_infinite()
                || d.is_finite()
                    && (d < 0.
                        || !v.is_finite()
                        || v <= 0.
                        || self.shared_sites[k] < self.min_overlap)
                || d.is_nan() && v.is_finite()
            {
                return Err("invalid cache estimate or overlap".into());
            }
        }
        Ok(())
    }
    pub fn read(path: &Path) -> Result<Self> {
        let m: Self = serde_json::from_reader(std::io::BufReader::new(fs::File::open(path)?))?;
        m.validate()?;
        Ok(m)
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        write_json(path, self)
    }
    pub fn matches(&self, a: &Alignment, model: Model, min_overlap: u64) -> bool {
        self.input_hash == a.hash()
            && self.model == model
            && self.min_overlap == min_overlap
            && self.gap_policy == "pairwise deletion; ACGTU valid; U=T"
    }
    pub fn export_tsv(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let mut out = std::io::BufWriter::new(fs::File::create(path)?);
        write!(out, "sequence")?;
        for name in &self.names {
            write!(out, "\t{name}")?;
        }
        writeln!(out)?;
        for (i, name) in self.names.iter().enumerate() {
            write!(out, "{name}")?;
            for j in 0..self.names.len() {
                match self.get(i, j) {
                    Some(d) => write!(out, "\t{d:.12}")?,
                    None => write!(out, "\tNA")?,
                }
            }
            writeln!(out)?;
        }
        out.flush()?;
        Ok(())
    }
    pub fn tree(&self, method: Method, threads: usize) -> Result<TreeResult> {
        self.validate()?;
        let n = self.names.len();
        let pool = pool(threads)?;
        // A complete matrix is already the triangle the search needs.
        if n >= 3
            && matches!(method, Method::Sorted | Method::Njstar)
            && self.distances.iter().all(|x| x.is_finite())
        {
            let (t, st) = pool.install(|| {
                optimized::tree(self.distances.clone(), &self.names, threads > 1, 512, false)
            });
            return Ok(TreeResult {
                trees: vec![t],
                components: 1,
                unresolved: false,
                details: vec![
                    json!({"completion":{},"search_s":st.search_s,"update_s":st.update_s,"q_evaluations":st.evaluated}),
                ],
            });
        }
        let full = |values: &[f64]| {
            pool.install(|| {
                let mut d = vec![0.; n * n];
                d.par_chunks_mut(n).enumerate().for_each(|(i, row)| {
                    for (j, v) in row.iter_mut().enumerate() {
                        if i != j {
                            *v = values[index(i, j)];
                        }
                    }
                });
                d
            })
        };
        let mut d = full(&self.distances);
        let groups = incomplete::components(&d, n);
        let mut result = TreeResult {
            trees: vec![],
            components: groups.len(),
            unresolved: false,
            details: vec![],
        };
        let one_component = groups.len() == 1;
        for group in groups {
            let size = group.len();
            let names: Vec<_> = group.iter().map(|&i| self.names[i].clone()).collect();
            if size == 1 {
                result.trees.push(format!("{};\n", quote(&names[0])));
                continue;
            }
            let mut dm = if one_component {
                std::mem::take(&mut d)
            } else {
                let values = &d;
                group
                    .iter()
                    .flat_map(|&i| group.iter().map(move |&j| values[i * n + j]))
                    .collect()
            };
            if size == 2 {
                result.trees.push(format!(
                    "({}:{:.12},{}:{:.12});\n",
                    quote(&names[0]),
                    dm[1] / 2.,
                    quote(&names[1]),
                    dm[1] / 2.
                ));
                continue;
            }
            let mut info = json!({});
            if method == Method::Fourpoint {
                info = pool.install(|| incomplete::completion(&mut dm, size, threads > 1));
            }
            let complete = dm.iter().all(|x| x.is_finite());
            if method == Method::Upgma {
                if !complete {
                    return Err("UPGMA requires complete distances within each connected component; choose another model or improve overlap".into());
                }
                result.trees.push(upgma::tree(dm, &names));
                result.details.push(json!({"algorithm":"nearest-neighbor chain", "weighting":"cluster size", "rooted":true}));
            } else if complete && method != Method::Mvrstar {
                let triangle: Vec<_> = (1..size)
                    .flat_map(|i| dm[i * size..i * size + i].iter().copied())
                    .collect();
                let (t, st) =
                    pool.install(|| optimized::tree(triangle, &names, threads > 1, 512, false));
                result.trees.push(t);
                result.details.push(json!({"completion":info,"search_s":st.search_s,"update_s":st.update_s,"q_evaluations":st.evaluated}));
            } else if matches!(method, Method::Njstar | Method::Mvrstar) {
                let vm = group
                    .iter()
                    .flat_map(|&i| {
                        group.iter().map(move |&j| {
                            if i == j {
                                0.
                            } else {
                                self.variances[index(i, j)]
                            }
                        })
                    })
                    .collect();
                let (trees, st) = pool.install(|| {
                    incomplete::direct(
                        dm,
                        vm,
                        &names,
                        if method == Method::Mvrstar {
                            "mvrstar"
                        } else {
                            "njstar"
                        },
                        threads > 1,
                        false,
                    )
                });
                result.unresolved |= trees.len() > 1;
                result.trees.extend(trees);
                result.details.push(st);
            } else {
                return Err("missing distances remain within a connected component; use --method fourpoint, njstar, or mvrstar".into());
            }
        }
        Ok(result)
    }
    /// Statistics use only observed finite distances. Missing comparisons are counted.
    pub fn groups(&self, assignments: &BTreeMap<String, String>) -> Result<Vec<GroupSummary>> {
        self.validate()?;
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for name in assignments.keys() {
            if !self.names.contains(name) {
                return Err(format!("unknown specimen: {name}").into());
            }
        }
        for (i, name) in self.names.iter().enumerate() {
            if let Some(group) = assignments.get(name) {
                if group.is_empty() {
                    return Err("empty group name".into());
                }
                groups.entry(group).or_default().push(i);
            }
        }
        let mut out = vec![];
        for (&group, ids) in &groups {
            let mut values = vec![];
            let mut missing = 0;
            for (k, &i) in ids.iter().enumerate() {
                for &j in &ids[..k] {
                    if let Some(d) = self.get(i, j) {
                        values.push(d);
                    } else {
                        missing += 1;
                    }
                }
            }
            let max = values.iter().copied().reduce(f64::max);
            let mean =
                (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64);
            let mut nearest: Option<(f64, String)> = None;
            let mut nearest_groups = std::collections::BTreeSet::new();
            let mut observed_between = 0;
            let mut missing_between = 0;
            for (&other, other_ids) in &groups {
                if other == group {
                    continue;
                }
                for &i in ids {
                    for &j in other_ids {
                        if let Some(d) = self.get(i, j) {
                            observed_between += 1;
                            if nearest.as_ref().is_none_or(|(v, _)| d < *v) {
                                nearest_groups.clear();
                                nearest_groups.insert(other.to_string());
                                nearest = Some((d, other.to_string()));
                            } else if nearest
                                .as_ref()
                                .is_some_and(|(v, _)| d.total_cmp(v).is_eq())
                            {
                                nearest_groups.insert(other.to_string());
                            }
                        } else {
                            missing_between += 1;
                        }
                    }
                }
            }
            let gap = if missing == 0 && missing_between == 0 {
                max.zip(nearest.as_ref().map(|x| x.0)).map(|(a, b)| a < b)
            } else {
                None
            };
            out.push(GroupSummary {
                group: group.into(),
                specimens: ids.len(),
                mean_within: mean,
                max_within: max,
                min_between: nearest.as_ref().map(|x| x.0),
                nearest_group: nearest.map(|x| x.1),
                nearest_groups: nearest_groups.into_iter().collect(),
                observed_between,
                barcode_gap: gap,
                observed_within: values.len(),
                missing_within: missing,
                missing_between,
            });
        }
        Ok(out)
    }
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
#[derive(Debug, Serialize)]
pub struct TreeResult {
    pub trees: Vec<String>,
    pub components: usize,
    pub unresolved: bool,
    pub details: Vec<serde_json::Value>,
}
#[derive(Debug, Serialize)]
pub struct GroupSummary {
    pub group: String,
    pub specimens: usize,
    pub mean_within: Option<f64>,
    pub max_within: Option<f64>,
    pub min_between: Option<f64>,
    pub nearest_group: Option<String>,
    pub nearest_groups: Vec<String>,
    pub observed_between: usize,
    pub barcode_gap: Option<bool>,
    pub observed_within: usize,
    pub missing_within: usize,
    pub missing_between: usize,
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    let mut f = std::io::BufWriter::new(temporary.as_file_mut());
    serde_json::to_writer(&mut f, value)?;
    writeln!(f)?;
    f.flush()?;
    drop(f);
    temporary.persist(path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn alignment() -> Alignment {
        Alignment {
            names: vec!["a 'one".into(), "b".into(), "c".into(), "d".into()],
            sequences: vec![
                b"AAAAAAAACC".to_vec(),
                b"AAAAAAAATC".to_vec(),
                b"CCAAAAAACC".to_vec(),
                b"CCAAAAAATC".to_vec(),
            ],
        }
    }
    #[test]
    fn analytic_models_and_saturation() {
        let p = estimate(Model::P, 100, 6, 4).unwrap().0;
        assert!((p - 0.1).abs() < 1e-14);
        let jc = estimate(Model::Jc69, 100, 6, 4).unwrap().0;
        assert!((jc + 0.75 * (1.0_f64 - 4. * 0.1 / 3.).ln()).abs() < 1e-14);
        let k = estimate(Model::K2p, 100, 6, 4).unwrap().0;
        assert!((k + 0.5 * 0.84_f64.ln() + 0.25 * 0.92_f64.ln()).abs() < 1e-14);
        assert!(estimate(Model::Jc69, 100, 25, 50).is_none());
        assert!(estimate(Model::K2p, 100, 50, 0).is_none());
        assert!(estimate(Model::P, 0, 0, 0).is_none());
    }
    #[test]
    fn nearest_group_ties_and_singleton_unknowns_remain_explicit() {
        let a = Alignment {
            names: vec!["a".into(), "b".into(), "c".into()],
            sequences: vec![b"AAAA".to_vec(), b"AAAC".to_vec(), b"AAAG".to_vec()],
        };
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let summary = matrix
            .groups(
                &[
                    ("a".into(), "A".into()),
                    ("b".into(), "B".into()),
                    ("c".into(), "C".into()),
                ]
                .into(),
            )
            .unwrap();
        assert_eq!(summary[0].nearest_groups, vec!["B", "C"]);
        assert_eq!(summary[0].observed_between, 2);
        assert!(summary[0].mean_within.is_none());
        assert!(summary[0].max_within.is_none());
        assert!(summary[0].barcode_gap.is_none());
    }
    #[test]
    fn cached_observations_survive_tree_and_group_changes() {
        let a = alignment();
        let m = DistanceMatrix::calculate(&a, Model::Jc69, 1, 4).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("matrix.json");
        m.write(&p).unwrap();
        let saved = DistanceMatrix::read(&p).unwrap();
        assert!(saved.matches(&a, Model::Jc69, 1));
        assert!(!saved.matches(&a, Model::K2p, 1));
        assert!(!saved.matches(&a, Model::Jc69, 2));
        let before = saved.distances.clone();
        let tree = saved.tree(Method::Sorted, 4).unwrap();
        assert_eq!(tree.trees.len(), 1);
        assert!(tree.trees[0].contains("'a ''one'"));
        assert_eq!(before, saved.distances);
        let mut assignments = BTreeMap::from([
            ("a 'one".into(), "A".into()),
            ("b".into(), "A".into()),
            ("c".into(), "B".into()),
        ]);
        let summaries = saved.groups(&assignments).unwrap();
        assert_eq!(summaries[0].specimens, 2);
        assert!(summaries[1].mean_within.is_none());
        assignments.insert("d".into(), "B".into());
        assert_eq!(saved.groups(&assignments).unwrap()[1].specimens, 2);
    }
    #[test]
    fn gaps_forests_and_missing_summary() {
        let a = Alignment {
            names: vec!["a".into(), "b".into(), "c".into()],
            sequences: vec![b"AA--".to_vec(), b"--AA".to_vec(), b"AAAA".to_vec()],
        };
        let m = DistanceMatrix::calculate(&a, Model::Jc69, 1, 2).unwrap();
        assert_eq!(m.get(0, 1), None);
        assert_eq!(m.shared_sites, vec![0, 2, 2]);
        assert!(m.tree(Method::Sorted, 1).is_err());
        assert!(m.tree(Method::Upgma, 1).is_err());
        let t = m.tree(Method::Njstar, 2).unwrap();
        assert!(t.unresolved);
        assert_eq!(t.trees.len(), 3);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        m.write(&path).unwrap();
        assert!(DistanceMatrix::read(&path).unwrap().get(0, 1).is_none());
        let groups = BTreeMap::from([
            ("a".into(), "A".into()),
            ("b".into(), "A".into()),
            ("c".into(), "B".into()),
        ]);
        let s = m.groups(&groups).unwrap();
        assert_eq!(s[0].missing_within, 1);
        assert!(s[0].mean_within.is_none());
        assert!(s[0].barcode_gap.is_none());
    }
    #[test]
    fn scalar_counts_and_parallel_reproducibility() {
        let a = Alignment {
            names: vec!["a".into(), "b".into()],
            sequences: vec![b"AGCTN-U?".repeat(17), b"GATT-ANT".repeat(17)],
        };
        let code = |c| match c {
            b'A' => Some(0),
            b'G' => Some(1),
            b'C' => Some(2),
            b'T' | b'U' => Some(3),
            _ => None,
        };
        let (mut valid, mut ts, mut tv) = (0, 0, 0);
        for (&x, &y) in a.sequences[0].iter().zip(&a.sequences[1]) {
            if let (Some(x), Some(y)) = (code(x), code(y)) {
                valid += 1;
                if (x ^ y) & 2 != 0 {
                    tv += 1;
                } else if x != y {
                    ts += 1;
                }
            }
        }
        assert_eq!(
            counts(
                &optimized::pack_fast(&a.sequences[0]),
                &optimized::pack_fast(&a.sequences[1])
            ),
            (valid, ts, tv)
        );
        for model in [Model::P, Model::Jc69, Model::K2p] {
            let a = alignment();
            let x = DistanceMatrix::calculate(&a, model, 1, 1).unwrap();
            let y = DistanceMatrix::calculate(&a, model, 1, 4).unwrap();
            assert_eq!(x.distances, y.distances);
            assert_eq!(
                x.tree(Method::Sorted, 1).unwrap().trees,
                y.tree(Method::Sorted, 4).unwrap().trees
            );
        }
    }
    #[test]
    fn exact_search_on_nonmetric_matrices() {
        let mut state = 719_u64;
        for n in [5, 19, 63] {
            for _ in 0..10 {
                let mut d = vec![0.; n * n];
                for i in 0..n {
                    for j in 0..i {
                        state = state
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1);
                        let v = (state >> 32) as f64 / f64::from(u32::MAX);
                        d[i * n + j] = v;
                        d[j * n + i] = v;
                    }
                }
                let names = (0..n).map(|i| format!("s{i}")).collect::<Vec<_>>();
                let pool = pool(4).unwrap();
                let d: Vec<_> = (1..n)
                    .flat_map(|i| d[i * n..i * n + i].iter().copied())
                    .collect();
                let a = optimized::tree(d.clone(), &names, false, 0, true);
                let b = pool.install(|| optimized::tree(d, &names, true, 0, true));
                assert_eq!(a.1.mismatches, 0);
                assert_eq!(b.1.mismatches, 0);
                assert_eq!(a.0, b.0);
            }
        }
    }
}
