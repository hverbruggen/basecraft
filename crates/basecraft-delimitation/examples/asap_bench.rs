//! Benchmark and reference-comparison driver for native ASAP.
//!
//! ```sh
//! asap_bench --fasta in.fasta --model jc69 --write-matrix in.phy --seed 1 --out DIR
//! asap_bench --matrix in.phy --length 650 --seed 1 --out DIR
//! ```
//! Writes `exact.tsv`, `nodes.tsv`, `subsets.tsv`, `res.cvs` and `timing.json`.
use basecraft_delimitation::asap;
use clap::Parser;
use std::{fmt::Write as _, path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    fasta: Option<PathBuf>,
    #[arg(long, default_value = "jc69")]
    model: String,
    #[arg(long)]
    write_matrix: Option<PathBuf>,
    #[arg(long)]
    matrix: Option<PathBuf>,
    #[arg(long)]
    length: Option<u32>,
    #[arg(long, default_value_t = 1)]
    seed: u32,
    #[arg(long, default_value_t = 100_000)]
    max_simulations: usize,
    #[arg(long, default_value_t = 1)]
    threads: usize,
    /// Independent per-simulation streams on `threads` workers.
    #[arg(long)]
    parallel: bool,
    #[arg(long)]
    out: PathBuf,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn read_phylip(path: &PathBuf) -> Result<(Vec<String>, Vec<f64>)> {
    let text = std::fs::read_to_string(path)?;
    let mut words = text.split_whitespace();
    let n: usize = words.next().ok_or("empty matrix")?.parse()?;
    let mut names = Vec::with_capacity(n);
    let mut full = Vec::with_capacity(n * n);
    for _ in 0..n {
        names.push(words.next().ok_or("missing name")?.to_string());
        for _ in 0..n {
            full.push(words.next().ok_or("missing distance")?.parse::<f64>()?);
        }
    }
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            // The reference reads the upper triangle (row < column).
            condensed.push(full[j * n + i]);
        }
    }
    Ok((names, condensed))
}

fn write_phylip(path: &PathBuf, names: &[String], values: &[f64]) -> Result<()> {
    let n = names.len();
    let get = |i: usize, j: usize| -> f64 {
        if i == j {
            0.0
        } else {
            let (x, y) = (i.max(j), i.min(j));
            values[x * (x - 1) / 2 + y]
        }
    };
    let mut out = format!("{n}\n");
    for (i, name) in names.iter().enumerate() {
        out.push_str(name);
        for j in 0..n {
            // Shortest round-trip form: the C reader recovers the same double.
            write!(out, " {:e}", get(i, j))?;
        }
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.out)?;
    let started = Instant::now();
    let (names, values, length) = if let Some(fasta) = &args.fasta {
        let alignment = basecraft_distance::Alignment::read(fasta)?;
        let model = match args.model.as_str() {
            "p" => basecraft_distance::Model::P,
            "jc69" => basecraft_distance::Model::Jc69,
            "k2p" => basecraft_distance::Model::K2p,
            _ => return Err("model must be p, jc69 or k2p".into()),
        };
        let matrix =
            basecraft_distance::DistanceMatrix::calculate(&alignment, model, 1, args.threads)?;
        // Basecraft's JC69 of identical sequences is -0.0; ASAP treats it as 0.
        let values: Vec<f64> = matrix.distances.iter().map(|&d| d + 0.0).collect();
        let length = u32::try_from(alignment.sequences[0].len())?;
        (alignment.names, values, length)
    } else {
        let (names, values) =
            read_phylip(args.matrix.as_ref().ok_or("--fasta or --matrix required")?)?;
        (
            names,
            values,
            args.length.ok_or("--length required with --matrix")?,
        )
    };
    let distance_seconds = started.elapsed().as_secs_f64();
    if let Some(path) = &args.write_matrix {
        write_phylip(path, &names, &values)?;
    }
    let settings = asap::Settings {
        sequence_length: length,
        seed: args.seed,
        max_simulations: args.max_simulations,
        stream: if args.parallel {
            asap::Stream::Parallel
        } else {
            asap::Stream::Reference
        },
        ..asap::Settings::default()
    };
    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;
    let analysis = pool.install(|| {
        asap::analyse(
            &asap::Distances {
                n: names.len(),
                values: &values,
            },
            &settings,
            |_, _| true,
        )
    })?;
    let analysis_seconds = started.elapsed().as_secs_f64();
    std::fs::write(args.out.join("exact.tsv"), analysis.exact_table())?;
    std::fs::write(args.out.join("nodes.tsv"), analysis.node_table(names.len()))?;
    std::fs::write(args.out.join("res.cvs"), analysis.results_table())?;
    let mut subsets = "position\tsubset\tp_value\tmembers\n".to_string();
    for (i, p) in analysis.partitions.iter().enumerate() {
        for (j, s) in p.subsets_detail.iter().flatten().enumerate() {
            let members: Vec<&str> = s.members.iter().map(|&m| names[m].as_str()).collect();
            writeln!(
                subsets,
                "{}\t{}\t{:e}\t{}",
                i + 1,
                j + 1,
                s.p_value,
                members.join(",")
            )?;
        }
    }
    std::fs::write(args.out.join("subsets.tsv"), subsets)?;
    let order: Vec<&str> = analysis
        .display_order
        .iter()
        .map(|&m| names[m].as_str())
        .collect();
    std::fs::write(
        args.out.join("timing.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "specimens": names.len(),
            "sequence_length": length,
            "partitions": analysis.partitions.len(),
            "distance_seconds": distance_seconds,
            "analysis_seconds": analysis_seconds,
            "diagnostics": analysis.diagnostics,
            "display_order": order,
        }))?,
    )?;
    Ok(())
}
