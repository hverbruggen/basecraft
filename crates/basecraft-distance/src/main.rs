use basecraft_distance::{
    Alignment, BootstrapOptions, DistanceMatrix, Method, Model, Result, bootstrap, write_json,
};
use clap::Parser;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf, time::Instant};
#[derive(Parser)]
#[command(
    version,
    about = "Parallel DNA distances, reusable matrices, grouping summaries, NJ and UPGMA trees"
)]
struct Args {
    /// Aligned DNA FASTA; omit to use an existing --cache.
    #[arg(long)]
    input: Option<PathBuf>,
    /// Observed-distance JSON cache. Reused only when input, model and overlap agree.
    #[arg(long)]
    cache: Option<PathBuf>,
    /// DNA distance model (ignored with cache-only input).
    #[arg(long, value_enum, default_value = "k2p")]
    model: Model,
    #[arg(long, default_value_t = 1)]
    min_overlap: u64,
    /// Distance worker count; defaults to available CPUs, capped at 16.
    #[arg(long)]
    threads: Option<usize>,
    /// Tree worker count; defaults to at most four distance workers.
    #[arg(long)]
    tree_threads: Option<usize>,
    /// Optional observed-distance TSV matrix; missing values are NA.
    #[arg(long)]
    matrix: Option<PathBuf>,
    /// Optional Newick output (one tree per disconnected/unresolved group).
    #[arg(long)]
    tree: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "sorted")]
    method: Method,
    /// Ordinary site bootstrap replicates (off by default; alignment and tree required).
    #[arg(long, default_value_t = 0)]
    bootstrap: usize,
    /// Bootstrap seed, independent of worker counts.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Atomic bootstrap progress JSON snapshot.
    #[arg(long)]
    progress: Option<PathBuf>,
    /// Cancel between replicates when this file exists.
    #[arg(long)]
    cancel_file: Option<PathBuf>,
    /// JSON object mapping sequence names to group names. Unlisted names are unassigned.
    #[arg(long, requires = "summary")]
    groups: Option<PathBuf>,
    /// Group statistics as a JSON table, with null for unavailable values.
    #[arg(long, requires = "groups")]
    summary: Option<PathBuf>,
    /// Optional run metadata JSON; also printed to stdout.
    #[arg(long)]
    report: Option<PathBuf>,
}
fn run(a: &Args) -> Result<()> {
    let start = Instant::now();
    let threads = a
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(16)));
    let tt = a.tree_threads.unwrap_or(threads.min(4));
    if threads == 0 || tt == 0 {
        return Err("worker counts must be positive".into());
    }
    if a.bootstrap > 0
        && (a.input.is_none()
            || a.tree.is_none()
            || !matches!(a.method, Method::Sorted | Method::Upgma))
    {
        return Err("bootstrap requires --input, --tree and --method sorted or upgma; a cache alone is insufficient".into());
    }
    if a.cache.is_none() && a.matrix.is_none() && a.tree.is_none() && a.summary.is_none() {
        return Err("choose --cache, --matrix, --tree, or --summary output".into());
    }
    // Prevent accidental input destruction and conflicting output paths, including symlinks.
    let canonical = |p: &PathBuf| -> Result<PathBuf> {
        if p.exists() {
            Ok(p.canonicalize()?)
        } else {
            Ok(p.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."))
                .canonicalize()?
                .join(p.file_name().ok_or("invalid output path")?))
        }
    };
    let mut outputs = std::collections::HashSet::new();
    for path in [
        &a.cache,
        &a.matrix,
        &a.tree,
        &a.summary,
        &a.report,
        &a.progress,
    ]
    .into_iter()
    .flatten()
    {
        let path = canonical(path)?;
        if !outputs.insert(path.clone()) {
            return Err("output paths must be distinct".into());
        }
        for input in [&a.input, &a.groups, &a.cancel_file].into_iter().flatten() {
            if canonical(input)? == path {
                return Err("an output would overwrite an input".into());
            }
        }
    }
    let mut cached = false;
    let alignment = a
        .input
        .as_ref()
        .map(|input| Alignment::read(input))
        .transpose()?;
    let matrix = if let Some(alignment) = &alignment {
        let saved = a
            .cache
            .as_ref()
            .filter(|p| p.exists())
            .map(|p| DistanceMatrix::read(p))
            .transpose()?;
        if let Some(m) = saved.filter(|m| m.matches(alignment, a.model, a.min_overlap)) {
            cached = true;
            m
        } else {
            DistanceMatrix::calculate(alignment, a.model, a.min_overlap, threads)?
        }
    } else {
        cached = true;
        DistanceMatrix::read(
            a.cache
                .as_ref()
                .ok_or("provide --input or an existing --cache")?,
        )?
    };
    let distance_s = start.elapsed().as_secs_f64();
    // Finish calculations before writing any requested artifacts.
    let (tree, bootstrap_report) = if a.bootstrap > 0 {
        let result = bootstrap(
            alignment.as_ref().unwrap(),
            &matrix,
            &BootstrapOptions {
                replicates: a.bootstrap,
                seed: a.seed,
                method: a.method,
                distance_threads: threads,
                tree_threads: tt,
                progress: a.progress.clone(),
                cancel_file: a.cancel_file.clone(),
            },
        )?;
        (result.tree, Some(result.report))
    } else {
        (
            a.tree
                .as_ref()
                .map(|_| matrix.tree(a.method, tt))
                .transpose()?,
            None,
        )
    };
    let summary = a
        .groups
        .as_ref()
        .map(|p| -> Result<_> {
            let groups: BTreeMap<String, String> = serde_json::from_reader(fs::File::open(p)?)?;
            matrix.groups(&groups)
        })
        .transpose()?;
    if let Some(path) = &a.cache {
        if !cached {
            matrix.write(path)?;
        }
    }
    if let Some(path) = &a.matrix {
        matrix.export_tsv(path)?;
    }
    if let (Some(path), Some(tree)) = (&a.tree, &tree) {
        fs::write(path, tree.trees.concat())?;
    }
    if let (Some(path), Some(summary)) = (&a.summary, &summary) {
        write_json(
            path,
            &json!({"model":matrix.model,"min_overlap":matrix.min_overlap,"gap_policy":matrix.gap_policy,"input_hash":matrix.input_hash,"unassigned":matrix.names.len()-summary.iter().map(|g|g.specimens).sum::<usize>(),"groups":summary}),
        )?;
    }
    let result = json!({"tool":"basecraft-distance","version":env!("CARGO_PKG_VERSION"),"model":matrix.model,"method":a.method,"min_overlap":matrix.min_overlap,"gap_policy":matrix.gap_policy,"input_hash":matrix.input_hash,"cache_reused":cached,"sequences":matrix.names.len(),"columns":matrix.columns,"distance_threads":threads,"tree_threads":tt,"distance_s":distance_s,"total_s":start.elapsed().as_secs_f64(),"zero_overlap_pairs":matrix.shared_sites.iter().filter(|&&n|n==0).count(),"short_overlap_pairs":matrix.shared_sites.iter().filter(|&&n|n>0&&n<matrix.min_overlap).count(),"saturated_pairs":matrix.distances.iter().zip(&matrix.shared_sites).filter(|(d,s)|!d.is_finite()&&**s>=matrix.min_overlap).count(),"tree":tree,"bootstrap":bootstrap_report});
    if let Some(path) = &a.report {
        write_json(path, &result)?;
    }
    println!("{result}");
    if let Some(report) = &bootstrap_report {
        if report.support_withheld {
            eprintln!("Bootstrap support withheld: {}", report.failures.join("; "));
        }
        if tree.is_none() {
            return Err(
                "original distances do not define a complete tree; see bootstrap failure report"
                    .into(),
            );
        }
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run(&Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("basecraft-distance: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
