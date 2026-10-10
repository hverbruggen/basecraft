//! The `basecraft` command-line tool.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "basecraft", version, about = "Basecraft command-line tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Index a FASTA file (or load its cached index) and print a summary.
    Index { file: PathBuf },
    /// Print residues `start..end` (0-based, half-open) of one record.
    Fetch {
        file: PathBuf,
        record: String,
        start: u64,
        end: u64,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("basecraft: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Index { file } => {
            let t = Instant::now();
            let f = basecraft_io::cache::open_fasta(&file)?;
            let elapsed = t.elapsed();
            let residues: u64 = f.index.records.iter().map(|r| r.len).sum();
            let ragged = f
                .index
                .records
                .iter()
                .filter(|r| matches!(r.layout, basecraft_io::fasta::Layout::Ragged(_)))
                .count();
            println!(
                "{} records, {residues} residues, {ragged} ragged, {} in {:.1} ms",
                f.index.records.len(),
                if f.cached {
                    "index from cache"
                } else {
                    "index built"
                },
                elapsed.as_secs_f64() * 1e3
            );
        }
        Command::Fetch {
            file,
            record,
            start,
            end,
        } => {
            let f = basecraft_io::cache::open_fasta(&file)?;
            let r = f
                .index
                .get(&record)
                .ok_or_else(|| basecraft_io::Error::NoSuchRecord(record.clone()))?;
            let seq = r.read(f.bytes(), start, end)?;
            println!("{}", String::from_utf8_lossy(&seq));
        }
    }
    Ok(())
}
