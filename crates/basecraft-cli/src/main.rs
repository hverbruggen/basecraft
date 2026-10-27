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
        /// Client name recorded with each edit and operation run: the
        /// assistant's model ID, or the script's name with `--client-kind script`.
        #[arg(long, alias = "client", default_value = "mcp-client")]
        model_id: String,
        /// Record edits as made by an assistant (`agent`) or a script (`script`).
        #[arg(long, value_enum, default_value_t)]
        client_kind: mcp::ClientKind,
    },
    /// Compatibility route: run a bundled tool's registered operation and write one
    /// result file, as the GUI does. Parameters are validated against the
    /// operation descriptor; prefer `operations run` for new scripts.
    Tool {
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(tools::NAMES))]
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
    /// Calculate DNA distances or infer a tree with the registered
    /// basecraft.distance operations. Omitted settings take the descriptor
    /// defaults shown by `operations describe basecraft.distance/tree@1`.
    Distance {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// p, jc69 or k2p.
        #[arg(long)]
        model: Option<String>,
        /// sorted, fourpoint, njstar, mvrstar or upgma.
        #[arg(long)]
        method: Option<String>,
        #[arg(long)]
        min_overlap: Option<u64>,
        #[arg(long)]
        threads: Option<usize>,
        #[arg(long)]
        tree_threads: Option<usize>,
        /// Exact ordinary site bootstrap for sorted NJ/UPGMA (off by default).
        #[arg(long)]
        bootstrap: Option<usize>,
        /// Where to retain bootstrap replicate trees: metadata or working.
        #[arg(long)]
        bootstrap_destination: Option<String>,
        #[arg(long)]
        seed: Option<u64>,
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
    /// Run a plugin's sequence operation on a file and apply what it proposes.
    /// Schema-2 packages must be installed or registered and run through the
    /// registered operation; schema-1 directories keep the legacy direct call.
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

/// A recoverable CLI session. Its edits are recorded as made by the CLI
/// script client, not by a person or an assistant.
pub(crate) fn script_session(
    root: impl Into<PathBuf>,
) -> Result<basecraft_ops::Session, basecraft_ops::Error> {
    let mut session = basecraft_ops::Session::recoverable(root)?;
    session.set_actor(basecraft_protocol::Actor::Script {
        client: "CLI".into(),
    });
    Ok(session)
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Plugins { action } => operations::plugins(action)?,
        Command::Operations { action } => operations::operations(action)?,
        Command::History { file, json } => {
            let file = file.canonicalize()?;
            let mut session =
                script_session(file.parent().ok_or("File has no parent")?)?;
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
                script_session(file.parent().ok_or("File has no parent")?)?;
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
            client_kind,
        } => mcp::serve(&project, allow_writes, client_kind, &model_id)?,
        Command::Tool {
            tool,
            file,
            output,
            params,
        } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = script_session(root)?;
            let doc = session.open(&file)?;
            let params = serde_json::from_str(&params)?;
            let registry = basecraft_plugin_host::registry::Registry::load(
                &basecraft_plugin_host::registry::RegistryConfig::discover()?,
            )?;
            let result = tools::run(&registry, &mut session, doc, &tool, output, params)?;
            println!("{result}");
        }
        Command::Edit { file, operation } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = script_session(root)?;
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
            let mut session = script_session(root)?;
            let doc = session.open(&file)?;
            let operation = if groups.is_some() {
                "groups"
            } else if matrix {
                "matrix"
            } else {
                "tree"
            };
            // Only explicit flags; the operation descriptor supplies defaults.
            let mut params = serde_json::Map::new();
            for (name, value) in [
                ("model", model.map(serde_json::Value::from)),
                ("method", method.map(serde_json::Value::from)),
                ("min_overlap", min_overlap.map(serde_json::Value::from)),
                ("threads", threads.map(serde_json::Value::from)),
                ("tree_threads", tree_threads.map(serde_json::Value::from)),
                ("bootstrap", bootstrap.map(serde_json::Value::from)),
                (
                    "bootstrap_destination",
                    bootstrap_destination.map(serde_json::Value::from),
                ),
                ("seed", seed.map(serde_json::Value::from)),
                (
                    "groups_data",
                    groups
                        .map(
                            |path| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                                Ok(serde_json::from_reader(std::fs::File::open(path)?)?)
                            },
                        )
                        .transpose()?,
                ),
            ] {
                if let Some(value) = value {
                    params.insert(name.into(), value);
                }
            }
            let mut host = serde_json::Map::new();
            if let Some(path) = cache {
                host.insert("cache".into(), serde_json::json!(path));
            }
            let registry = basecraft_plugin_host::registry::Registry::load(
                &basecraft_plugin_host::registry::RegistryConfig::discover()?,
            )?;
            let result = tools::run_operation(
                &registry,
                &mut session,
                doc,
                &format!("basecraft.distance/{operation}@1"),
                output,
                params,
                host,
            )?;
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
    if manifest.schema >= 2 {
        return run_registered_plugin(&manifest, file, operation, map, save);
    }
    // Schema-1 packages have no typed contract: the explicit legacy route.
    let t = Instant::now();
    let mut plugin = basecraft_plugin_host::PluginProcess::spawn(&manifest)?;
    println!(
        "{} {} started in {:.0} ms",
        plugin.info.id,
        plugin.info.version,
        plugin.startup.as_secs_f64() * 1e3
    );

    let root = file.parent().unwrap_or(std::path::Path::new("."));
    let mut session = script_session(root)?;
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

/// `plugin` for a schema-2 package: the registered sequence operation through the
/// same job runner as `operations run`, against the file opened in this session.
fn run_registered_plugin(
    manifest: &basecraft_plugin_host::Manifest,
    file: &std::path::Path,
    operation: &str,
    parameters: serde_json::Map<String, serde_json::Value>,
    save: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use basecraft_plugin_host::registry::{Registry, RegistryConfig};
    use basecraft_protocol::operation::{ExecutionFamily, InputReference, OperationRequest};
    let registry = Registry::load(&RegistryConfig::discover()?)?;
    let directory = manifest.dir.canonicalize()?;
    let registered = registry
        .operations
        .values()
        .find(|op| {
            op.package.manifest.id == manifest.id
                && op.descriptor.name == operation
                && op.package.manifest.dir.canonicalize().ok().as_ref() == Some(&directory)
        })
        .ok_or("This plugin directory is not an installed or registered package. Register it with `basecraft plugins register-editable DIR`, then run it again or use `basecraft operations run`")?;
    if registered.descriptor.family != ExecutionFamily::SequenceProposal {
        return Err("`plugin` applies sequence proposals only; run file analyses with `basecraft operations run`".into());
    }
    let binding = registered
        .descriptor
        .inputs
        .keys()
        .next()
        .ok_or("operation declares no snapshot input")?;
    let file = file.canonicalize()?;
    let root = file.parent().ok_or("File has no parent")?;
    let request = OperationRequest {
        operation: registered.id.clone(),
        inputs: [(
            binding.clone(),
            vec![InputReference::File {
                path: file.display().to_string(),
            }],
        )]
        .into(),
        parameters: serde_json::Value::Object(parameters),
        destinations: std::collections::BTreeMap::default(),
        pins: None,
        request_key: None,
    };
    let policy = basecraft_plugin_host::policy::SessionPolicy::new(root, true)?;
    let mut session = script_session(root)?;
    basecraft_platform::interrupt::install()?;
    let (outcome, target) =
        operations::run_foreground(&registry, &request, policy, "CLI", &mut session)?;
    println!("{outcome}");
    if outcome["state"] != "completed" {
        return Err(format!("{} did not complete", registered.id).into());
    }
    if save {
        session.save(target.ok_or("operation returned no target document")?, None)?;
    }
    Ok(())
}
