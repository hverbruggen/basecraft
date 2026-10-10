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
    /// Replay a project's operation log and check every step.
    Replay {
        #[arg(default_value = ".")]
        project: PathBuf,
    },
    /// Run a plugin operation on a file and apply what it proposes.
    Plugin {
        /// Plugin directory, with a plugin.toml.
        plugin: PathBuf,
        file: PathBuf,
        operation: String,
        /// Operation parameters as key=value; values parse as JSON if they can.
        params: Vec<String>,
        /// Save the file afterwards.
        #[arg(long)]
        save: bool,
    },
    /// Write JSON Schemas for the protocol types into a directory.
    Schemas { dir: PathBuf },
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
        Command::Replay { project } => {
            let report = basecraft_ops::replay::replay(&project)?;
            for p in &report.problems {
                println!("problem: {p}");
            }
            println!(
                "{} sessions, {} entries, {} problems",
                report.sessions,
                report.entries,
                report.problems.len()
            );
            if !report.ok() {
                return Err("replay found problems".into());
            }
        }
        Command::Plugin {
            plugin,
            file,
            operation,
            params,
            save,
        } => run_plugin(&plugin, &file, &operation, &params, save)?,
        Command::Schemas { dir } => {
            std::fs::create_dir_all(&dir)?;
            let schemas = [
                (
                    "log-entry",
                    schemars::schema_for!(basecraft_protocol::LogEntry),
                ),
                (
                    "feature",
                    schemars::schema_for!(basecraft_protocol::Feature),
                ),
            ];
            for (name, schema) in schemas {
                let mut json = serde_json::to_string_pretty(&schema)?;
                json.push('\n');
                std::fs::write(dir.join(format!("{name}.schema.json")), json)?;
            }
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

fn run_plugin(
    dir: &std::path::Path,
    file: &std::path::Path,
    operation: &str,
    params: &[String],
    save: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut map = serde_json::Map::new();
    for p in params {
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| format!("parameter {p:?} is not key=value"))?;
        let value = serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.into()));
        map.insert(k.to_string(), value);
    }
    let manifest = basecraft_plugin_host::Manifest::load(dir)?;
    let t = Instant::now();
    let mut plugin = basecraft_plugin_host::PluginProcess::spawn(&manifest)?;
    println!(
        "{} {} started in {:.0} ms",
        plugin.info.id,
        plugin.info.version,
        plugin.startup.as_secs_f64() * 1e3
    );

    let root = file.parent().unwrap_or(std::path::Path::new("."));
    let mut session = basecraft_ops::Session::new(root);
    let doc = session.open(file)?;
    let t_snap = Instant::now();
    let snap = basecraft_plugin_host::Snapshot::of(session.doc_key(doc), session.doc(doc))?;
    let snap_ms = t_snap.elapsed().as_secs_f64() * 1e3;
    let t_call = Instant::now();
    let result = plugin.invoke(operation, serde_json::Value::Object(map), Some(&snap))?;
    let call_ms = t_call.elapsed().as_secs_f64() * 1e3;
    println!(
        "snapshot of {} bp in {snap_ms:.0} ms, {operation} in {call_ms:.0} ms: {} proposals{}",
        snap.len,
        result.ops.len(),
        result
            .message
            .map(|m| format!(" ({m})"))
            .unwrap_or_default()
    );
    let actor = basecraft_protocol::Actor::Plugin {
        id: plugin.info.id.clone(),
        version: plugin.info.version.clone(),
    };
    let report = basecraft_plugin_host::apply::apply_proposals(
        &mut session,
        doc,
        snap.version,
        result.ops,
        &actor,
    )?;
    println!("applied {}, dropped {}", report.applied, report.dropped);
    if save {
        session.save(doc, None)?;
        println!("saved {}", file.display());
    }
    println!("total {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    Ok(())
}
