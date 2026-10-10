//! Deterministic synthetic test data. The same seed and scale give
//! byte-identical files on every machine.

mod fasta;
mod genome;
mod other;
mod rng;

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};

use genome::GenomeSpec;
use rng::Rng;

#[derive(Parser)]
#[command(about = "Generate synthetic test data for Basecraft benchmarks and spikes")]
struct Args {
    /// Output directory.
    #[arg(long)]
    out: PathBuf,
    /// small: seconds, for tests. full: the benchmark sizes, several GB.
    #[arg(long, value_enum, default_value_t = Scale::Small)]
    scale: Scale,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Regenerate files that already exist.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Scale {
    Small,
    Full,
}

struct Sizes {
    genome_len: u64,
    genome_genes: u64,
    fasta_bytes: u64,
    aln_rows: u64,
    aln_cols: u64,
    tree_tips: u64,
}

impl Scale {
    fn sizes(self) -> Sizes {
        match self {
            Scale::Small => Sizes {
                genome_len: 1_000_000,
                genome_genes: 250,
                fasta_bytes: 10_000_000,
                aln_rows: 100,
                aln_cols: 5_000,
                tree_tips: 1_000,
            },
            Scale::Full => Sizes {
                genome_len: 50_000_000,
                genome_genes: 12_000,
                fasta_bytes: 1_000_000_000,
                aln_rows: 10_000,
                aln_cols: 100_000,
                tree_tips: 50_000,
            },
        }
    }

    fn name(self) -> &'static str {
        match self {
            Scale::Small => "small",
            Scale::Full => "full",
        }
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let dir = args.out.join(args.scale.name());
    fs::create_dir_all(&dir)?;
    let s = args.scale.sizes();
    // Each output gets its own stream, so changing one leaves the others identical.
    let mut root = Rng::new(args.seed);
    let mut streams: Vec<Rng> = (0..6).map(|i| root.fork(i)).collect();
    let mut next = || streams.remove(0);

    let mut genome_rng = next();
    if wanted(&dir, "genome.fasta", args.force) {
        let spec = GenomeSpec {
            id: "chr1",
            length: s.genome_len,
            genes: s.genome_genes,
            circular: false,
            gc: 0.42,
        };
        let g = genome::generate(&spec, &mut genome_rng);
        write(&dir, "genome.fasta", |w| genome::write_fasta(&g, w))?;
        write(&dir, "genome.gff3", |w| genome::write_gff3(&g, w))?;
        write(&dir, "genome.gb", |w| genome::write_genbank(&g, w))?;
    }

    let mut plastid_rng = next();
    if wanted(&dir, "plastid.gb", args.force) {
        let spec = GenomeSpec {
            id: "plastid1",
            length: 150_000,
            genes: 130,
            circular: true,
            gc: 0.37,
        };
        let g = genome::generate(&spec, &mut plastid_rng);
        write(&dir, "plastid.gb", |w| genome::write_genbank(&g, w))?;
        write(&dir, "plastid.fasta", |w| genome::write_fasta(&g, w))?;
        write(&dir, "plastid.gff3", |w| genome::write_gff3(&g, w))?;
    }

    let mut fasta_rng = next();
    if wanted(&dir, "big.fasta", args.force) {
        write(&dir, "big.fasta", |w| {
            other::write_big_fasta(w, s.fasta_bytes, &mut fasta_rng)
        })?;
    }

    let mut ragged_rng = next();
    if wanted(&dir, "ragged.fasta", args.force) {
        write(&dir, "ragged.fasta", |w| {
            other::write_ragged_fasta(w, &mut ragged_rng)
        })?;
    }

    let mut aln_rng = next();
    if wanted(&dir, "alignment.fasta", args.force) {
        write(&dir, "alignment.fasta", |w| {
            other::write_alignment(w, s.aln_rows, s.aln_cols, &mut aln_rng)
        })?;
    }

    let mut tree_rng = next();
    if wanted(&dir, "tree.nwk", args.force) {
        write(&dir, "tree.nwk", |w| {
            other::write_tree(w, s.tree_tips, &mut tree_rng)
        })?;
    }
    Ok(())
}

fn wanted(dir: &Path, name: &str, force: bool) -> bool {
    let exists = dir.join(name).exists();
    if exists && !force {
        eprintln!("skip   {name} (exists)");
    }
    force || !exists
}

/// Write to a temporary file and rename, so an interrupted run never leaves
/// a truncated file that a later run would skip.
fn write(
    dir: &Path,
    name: &str,
    f: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut w = BufWriter::with_capacity(1 << 20, File::create(&tmp)?);
    f(&mut w)?;
    w.flush()?;
    drop(w);
    fs::rename(&tmp, dir.join(name))?;
    let size = fs::metadata(dir.join(name))?.len();
    #[allow(clippy::cast_precision_loss)]
    let mb = size as f64 / 1e6;
    eprintln!("wrote  {name} ({mb:.1} MB)");
    Ok(())
}
