//! The `basecraft` command-line tool.

mod mcp;
mod metadata;
mod operations;
mod submission;

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
    /// Reconstruct an unsaved session, or a Sanger review recovery checkpoint
    /// with all its call sets, into a new directory; never overwrites inputs.
    Recover {
        session: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// After a crash damaged the journal: recover only the edits before the
        /// first damaged line. The journal itself is left unchanged.
        #[arg(long)]
        verified_prefix: bool,
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
    /// Apply a typed JSON operation to a document and save it.
    Edit { file: PathBuf, operation: String },
    /// The project's specimen metadata: roster, identifications, record
    /// links and collection columns; imports and the .ods template.
    Metadata {
        #[command(subcommand)]
        action: metadata::MetadataCommand,
    },
    /// GenBank marker submissions: row decisions, batch settings, reading
    /// frames, the submission package, table2asn validation and accessions.
    Submission {
        #[command(subcommand)]
        action: submission::SubmissionCommand,
    },
    /// Add the support values of another tree file to a tree and save it. The
    /// source is referenced as the values' recorded input, not copied.
    ImportSupport {
        tree: PathBuf,
        source: PathBuf,
        /// Name for the imported support set; empty keeps the source's names.
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Extract a zero-based, half-open sequence range to a new GenBank file,
    /// retaining source revision and coordinate mapping through shared provenance.
    Extract {
        file: PathBuf,
        start: u64,
        end: u64,
        #[arg(long)]
        output: PathBuf,
        /// Ordered splice segments as JSON [[start,end],...].
        #[arg(long, default_value = "[]")]
        segments: String,
        #[arg(long)]
        reverse: bool,
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
        /// If saving would replace a file that other analyses recorded as
        /// their input: `cancel`, `preserve_copy`, `remove_analysis` or
        /// `break_evidence`. The same decision the application asks for.
        #[arg(long, requires = "save", value_parser = save_policy, default_value = "cancel")]
        input_conflict: basecraft_ops::input_evidence::SavePolicy,
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

/// Parse an input-conflict decision by its shared serialized name.
fn save_policy(name: &str) -> Result<basecraft_ops::input_evidence::SavePolicy, String> {
    serde_json::from_value(serde_json::Value::from(name)).map_err(|_| {
        "expected cancel, preserve_copy, remove_analysis or break_evidence".to_string()
    })
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Plugins { action } => operations::plugins(action)?,
        Command::Operations { action } => operations::operations(action)?,
        Command::Metadata { action } => metadata::run(action)?,
        Command::Submission { action } => submission::run(action)?,
        Command::History { file, json } => {
            let report = basecraft_ops::document_record::DocumentRecord::saved_report(&file)?;
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
            let report = basecraft_ops::document_record::DocumentRecord::saved_report(&file)?;
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
        Command::Recover {
            session,
            output,
            verified_prefix,
        } => {
            if basecraft_ops::sanger_review::evidence_kind(&session)
                == Some(basecraft_ops::sanger_review::EvidenceKind::Review)
            {
                if verified_prefix {
                    return Err(
                        "--verified-prefix applies to session journals, not Sanger reviews".into(),
                    );
                }
                let restored = basecraft_ops::sanger_review::recover(&session, &output)?;
                println!("{}", restored.display());
                return Ok(());
            }
            let restored = if verified_prefix {
                let (restored, problem) =
                    basecraft_ops::replay::recover_verified_prefix(&session, &output)?;
                if let Some(problem) = problem {
                    eprintln!("Recovered the verified edits before damage: {problem}");
                }
                restored
            } else {
                basecraft_ops::replay::recover(&session, &output)?
            };
            for restored in restored {
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
        Command::Edit { file, operation } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = script_session(root)?;
            let doc = session.open(&file)?;
            session.apply(doc, serde_json::from_str(&operation)?)?;
            session.save(doc, None)?;
        }
        Command::ImportSupport { tree, source, name } => {
            let tree = tree.canonicalize()?;
            let mut session = script_session(tree.parent().ok_or("File has no parent")?)?;
            let doc = session.open(&tree)?;
            let imported = session.import_support(doc, &source, &name)?;
            if imported.added.is_empty() {
                return Err("The source tree has no support values".into());
            }
            session.save(doc, None)?;
            println!(
                "{}",
                serde_json::json!({"added": imported.added, "matched": imported.matched})
            );
        }
        Command::Extract {
            file,
            start,
            end,
            output,
            segments,
            reverse,
        } => {
            let file = file.canonicalize()?;
            if output.try_exists()? {
                return Err("Extraction destination already exists; choose a new file".into());
            }
            let output = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."))
                .canonicalize()?
                .join(output.file_name().ok_or("Output filename missing")?);
            let segments: Vec<(u64, u64)> = serde_json::from_str(&segments)?;
            let mut session = script_session(file.parent().ok_or("File has no parent")?)?;
            let doc = session.open(&file)?;
            session.extract(doc, (start, end), &segments, reverse, &output)?;
            println!(
                "{}",
                serde_json::json!({"output":output,"hash":basecraft_io::hash_file(&output)?})
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
            input_conflict,
        } => run_plugin(
            &plugin,
            &file,
            &operation,
            &params,
            save.then_some(input_conflict),
        )?,
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
                    "project-metadata",
                    schemars::schema_for!(basecraft_protocol::metadata::ProjectMetadata),
                ),
                (
                    "metadata-edit",
                    schemars::schema_for!(basecraft_protocol::metadata::MetadataEdit),
                ),
                (
                    "submission",
                    schemars::schema_for!(basecraft_protocol::submission::Submission),
                ),
                (
                    "submission-edit",
                    schemars::schema_for!(basecraft_protocol::submission::SubmissionEdit),
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
    save: Option<basecraft_ops::input_evidence::SavePolicy>,
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
    let map_value = serde_json::Value::Object(map);
    let result = plugin.invoke(operation, map_value.clone(), Some(&snap))?;
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
    let edit_ids =
        session.apply_sequence_proposals(doc, generation, snap.version, &result.ops, &actor)?;
    // No typed contract, so no effective settings or retained run: record what
    // was submitted and the hash of the snapshot a registered run would retain.
    let params = serde_json::json!({"requested": map_value, "operation": operation,
        "contract": "schema-1", "initiator": "CLI",
        "snapshot": {"document": snap.doc, "version": snap.version,
            "hash": basecraft_io::hash_bytes(&snap.payload()?)}});
    session.log_contributing_tool_run(
        doc,
        actor,
        (&plugin.info.id, operation),
        params,
        &[],
        &edit_ids,
    )?;
    println!("applied {count} proposals");
    if let Some(policy) = save {
        session.save_with_evidence(doc, None, policy)?;
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
    save: Option<basecraft_ops::input_evidence::SavePolicy>,
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
        target: None,
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
    if let Some(policy) = save {
        session.save_with_evidence(
            target.ok_or("operation returned no target document")?,
            None,
            policy,
        )?;
    }
    Ok(())
}
