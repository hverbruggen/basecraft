//! The `basecraft` command-line tool.

mod mcp;
mod operations;
mod tools;

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
    /// Manage trusted local plugin packages.
    Plugins {
        #[command(subcommand)]
        action: operations::PluginsCommand,
    },
    /// Inspect versioned installed operations without executing plugins.
    Operations {
        #[command(subcommand)]
        action: operations::OperationsCommand,
    },
    /// Readable history for the file's exact current contents. Does not write records.
    History {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Preview methods immediately; optionally export the frozen report.
    Methods {
        file: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Print the validated shared preferences as JSON without creating files.
    Preferences,
    /// Reconstruct an unsaved session into a new directory; never overwrites inputs.
    Recover {
        session: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inspect, verify, report, or package readable saved-result provenance.
    Provenance {
        #[command(subcommand)]
        action: ProvenanceCommand,
    },
    /// Serve a headless project session over MCP stdio (2025-11-25).
    Mcp {
        #[arg(default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        allow_writes: bool,
        #[arg(long, default_value = "mcp-client")]
        model_id: String,
    },
    /// Run a bundled alignment tool through the same public plugin as the GUI.
    Tool {
        #[arg(value_parser = ["mafft", "trimal", "iqtree", "fasttree", "distance", "delimitation"])]
        tool: String,
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Tool parameters as a JSON object.
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Apply a typed JSON operation to a document and save it.
    Edit { file: PathBuf, operation: String },
    /// Calculate DNA distances or infer a tree through the bundled distance plugin.
    Distance {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "k2p", value_parser = ["p", "jc69", "k2p"])]
        model: String,
        #[arg(long, value_parser = ["sorted", "fourpoint", "njstar", "mvrstar", "upgma"])]
        method: Option<String>,
        #[arg(long, default_value_t = 1)]
        min_overlap: u64,
        #[arg(long)]
        threads: Option<usize>,
        #[arg(long)]
        tree_threads: Option<usize>,
        /// Exact ordinary site bootstrap for sorted NJ/UPGMA (off by default).
        #[arg(long, default_value_t = 0)]
        bootstrap: usize,
        /// Where to retain bootstrap replicate trees (relative to the output directory).
        #[arg(long, default_value = "metadata", value_parser = ["metadata", "working"])]
        bootstrap_destination: String,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long)]
        progress: Option<PathBuf>,
        #[arg(long)]
        cancel_file: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
        /// Produce an observed-distance TSV instead of a tree.
        #[arg(long, conflicts_with = "groups")]
        matrix: bool,
        /// JSON specimen-to-group assignments; produce a summary table.
        #[arg(long)]
        groups: Option<PathBuf>,
    },
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

#[derive(Subcommand)]
enum ProvenanceCommand {
    Inspect {
        result: PathBuf,
    },
    Verify {
        result: PathBuf,
    },
    Methods {
        result: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Copy a verified result with retained exact inputs; executes no tools.
    Package {
        result: PathBuf,
        #[arg(long)]
        output: PathBuf,
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
        Command::Plugins { action } => operations::plugins(action)?,
        Command::Operations { action } => operations::operations(action)?,
        Command::History { file, json } => {
            let file = file.canonicalize()?;
            let mut session =
                basecraft_ops::Session::recoverable(file.parent().ok_or("File has no parent")?)?;
            let doc = session.open(&file)?;
            let report = session.document_record(doc)?.report()?;
            println!(
                "{}",
                if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.history_text
                }
            );
        }
        Command::Methods { file, json, output } => {
            let file = file.canonicalize()?;
            let mut session =
                basecraft_ops::Session::recoverable(file.parent().ok_or("File has no parent")?)?;
            let doc = session.open(&file)?;
            let report = session.document_record(doc)?.report()?;
            if let Some(output) = output {
                report.export(&output)?;
            }
            println!(
                "{}",
                if json {
                    serde_json::to_string_pretty(&report)?
                } else {
                    report.methods
                }
            );
        }
        Command::Preferences => {
            let paths = basecraft_platform::paths::AppPaths::discover()?;
            let preferences =
                basecraft_platform::settings::SettingsStore::new(paths.preferences()).load()?;
            println!("{}", serde_json::to_string_pretty(&preferences)?);
        }
        Command::Recover { session, output } => {
            for restored in basecraft_ops::replay::recover(&session, &output)? {
                println!("{}", restored.display());
            }
        }
        Command::Provenance { action } => match action {
            ProvenanceCommand::Inspect { result } => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&basecraft_ops::provenance::inspect(&result)?)?
                );
            }
            ProvenanceCommand::Verify { result } => {
                let issues = basecraft_ops::provenance::verify(&result)?;
                if !issues.is_empty() {
                    return Err(issues.join("\n").into());
                }
                println!("done: recorded hashes verified; replay verification is a separate check");
            }
            ProvenanceCommand::Methods { result, output } => {
                let publication = basecraft_ops::provenance::inspect(&result)?;
                basecraft_ops::provenance::write_reports(&output, &publication)?;
            }
            ProvenanceCommand::Package { result, output } => {
                basecraft_ops::provenance::package(&result, &output)?;
            }
        },
        Command::Mcp {
            project,
            allow_writes,
            model_id,
        } => mcp::serve(&project, allow_writes, &model_id)?,
        Command::Tool {
            tool,
            file,
            output,
            params,
        } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = basecraft_ops::Session::recoverable(root)?;
            let doc = session.open(&file)?;
            let params = serde_json::from_str(&params)?;
            let result = tools::run(&mut session, doc, &tool, output, params)?;
            println!("{result}");
        }
        Command::Edit { file, operation } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = basecraft_ops::Session::recoverable(root)?;
            let doc = session.open(&file)?;
            session.apply(doc, serde_json::from_str(&operation)?)?;
            session.save(doc, None)?;
        }
        Command::Distance {
            file,
            output,
            model,
            method,
            min_overlap,
            threads,
            tree_threads,
            bootstrap,
            bootstrap_destination,
            seed,
            progress,
            cancel_file,
            cache,
            matrix,
            groups,
        } => {
            if progress.is_some() || cancel_file.is_some() {
                return Err(
                    "Control paths are host-managed; use operations run and owned job cancellation"
                        .into(),
                );
            }
            // The explicit CLI cache option remains a compatibility route.
            // FileJob validates aliases, freezes reused bytes and retains the
            // managed cache evidence before publishing any secondary copy.
            let root = file
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."));
            let mut session = basecraft_ops::Session::recoverable(root)?;
            let doc = session.open(&file)?;
            if !session.is_alignment(doc) {
                return Err("distance inference requires an alignment".into());
            }
            let plugin_root = basecraft_plugin_host::installation::plugins();
            let operation = if groups.is_some() {
                "groups"
            } else if matrix {
                "matrix"
            } else {
                "tree"
            };
            let mut job = basecraft_plugin_host::FileJob::from_alignment(
                &session,
                doc,
                (&plugin_root.join("distance"), operation),
                basecraft_plugin_host::file_tool::AlignmentInput::Aligned,
                output,
            )?;
            if bootstrap > 0 && operation != "tree" {
                return Err("bootstrap is available for tree inference only".into());
            }
            job.params
                .insert("bootstrap".into(), serde_json::json!(bootstrap));
            job.params.insert("seed".into(), serde_json::json!(seed));
            if bootstrap > 0 {
                job.params.insert(
                    "bootstrap_destination".into(),
                    serde_json::json!(bootstrap_destination),
                );
            }
            if let Some(path) = progress {
                job.params
                    .insert("progress".into(), serde_json::json!(path));
            }
            if let Some(path) = cancel_file {
                job.params
                    .insert("cancel_file".into(), serde_json::json!(path));
            }
            job.params.insert("model".into(), serde_json::json!(model));
            let method =
                method.unwrap_or_else(|| if bootstrap > 0 { "sorted" } else { "njstar" }.into());
            job.params
                .insert("method".into(), serde_json::json!(method));
            job.params
                .insert("min_overlap".into(), serde_json::json!(min_overlap));
            if let Some(v) = threads {
                job.params.insert("threads".into(), serde_json::json!(v));
            }
            if let Some(v) = tree_threads {
                job.params
                    .insert("tree_threads".into(), serde_json::json!(v));
            }
            if let Some(v) = cache {
                job.params.insert("cache".into(), serde_json::json!(v));
            }
            if let Some(v) = groups {
                job.params.insert(
                    "groups_data".into(),
                    serde_json::from_reader(std::fs::File::open(v)?)?,
                );
            }
            let started = Instant::now();
            let outcome = job.run();
            job.record_terminal(&mut session, doc, &outcome, started.elapsed().as_secs_f64())?;
            let result = outcome?;
            job.record(&mut session, doc, &result)?;
            println!(
                "{}",
                result
                    .message
                    .unwrap_or_else(|| "Calculated distances".into())
            );
        }
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
                    "operation-contract",
                    schemars::schema_for!(basecraft_protocol::operation::OperationContract),
                ),
                (
                    "operation-request",
                    schemars::schema_for!(basecraft_protocol::operation::OperationRequest),
                ),
                (
                    "operation-outcome",
                    schemars::schema_for!(basecraft_protocol::operation::OperationOutcome),
                ),
                (
                    "plugin-invoke-result",
                    schemars::schema_for!(basecraft_protocol::plugin::InvokeResult),
                ),
                (
                    "provenance-publication",
                    schemars::schema_for!(basecraft_protocol::provenance::Publication),
                ),
                (
                    "delimitation-request",
                    schemars::schema_for!(basecraft_protocol::delimitation::DelimitationRequest),
                ),
                (
                    "delimitation-result",
                    schemars::schema_for!(basecraft_protocol::delimitation::DelimitationResult),
                ),
                (
                    "delimitation-workspace",
                    schemars::schema_for!(basecraft_protocol::delimitation::DelimitationWorkspace),
                ),
                (
                    "job-record",
                    schemars::schema_for!(basecraft_protocol::delimitation::JobRecord),
                ),
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
    let mut session = basecraft_ops::Session::recoverable(root)?;
    let doc = session.open(file)?;
    let t_snap = Instant::now();
    let generation = session.generation(doc);
    let snap =
        basecraft_plugin_host::Snapshot::of(session.doc_key(doc), session.checked_sequence(doc)?)?;
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
    let count = result.ops.len();
    session.apply_sequence_proposals(doc, generation, snap.version, &result.ops, &actor)?;
    println!("applied {count} proposals");
    if save {
        session.save(doc, None)?;
        println!("saved {}", file.display());
    }
    println!("total {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    Ok(())
}
