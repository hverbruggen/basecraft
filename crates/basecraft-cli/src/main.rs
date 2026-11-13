//! The `basecraft` command-line tool.

mod delimitation;
mod mcp;
mod metadata;
mod operations;
mod save;
mod submission;

use basecraft_ops::records::ReportExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "basecraft", version, about = "Basecraft command-line tool")]
struct Cli {
    /// The project folder you chose. Files inside it keep their history in the
    /// project; without it every file is a loose file. Basecraft never guesses.
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[command(flatten)]
    save_options: save::Options,
    #[command(subcommand)]
    command: Command,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum EditOperations {
    One(Box<basecraft_protocol::Op>),
    Many(Vec<basecraft_protocol::Op>),
}

#[derive(Subcommand)]
enum Command {
    /// Inspect and combine saved species hypotheses through the shared workflow.
    Delimitation {
        #[command(subcommand)]
        action: delimitation::DelimitationCommand,
    },
    /// Create an independent DNA sequence as a new GenBank file.
    NewSequence {
        #[arg(long)]
        name: String,
        /// Plain IUPAC DNA text (whitespace is ignored).
        #[arg(long)]
        sequence: String,
        #[arg(long)]
        output: PathBuf,
    },
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
    /// Reconstruct an unsaved session
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
    /// Maintain the records basecraft keeps in a project's `.basecraft` folders.
    Records {
        #[command(subcommand)]
        action: RecordsCommand,
    },
    /// Serve a headless project session over MCP stdio (2025-11-25).
    Mcp {
        /// The project folder: the session's root and access boundary.
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
    /// Inspect a save without writing. The returned token expires when this command exits.
    SaveCheck {
        file: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Apply a typed JSON operation to a document and save it.
    Edit {
        file: PathBuf,
        /// One JSON operation, or an array applied in one session before saving.
        operation: String,
    },
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
        /// Check that this is the open (first) record of a file of several.
        /// Other records can't be extracted yet.
        #[arg(long)]
        record: Option<String>,
    },
    /// What an alignment or sequence is written in: the stated alphabet or
    /// basecraft's guess, and each sequence's guess.
    Alphabet { file: PathBuf },
    /// Index a FASTA file (or load its cached index) and print a summary.
    Index { file: PathBuf },
    /// Replay a project's operation log and check every step.
    Replay {
        #[arg(default_value = ".")]
        project: PathBuf,
    },
    /// Run a plugin's sequence operation on a file and apply what it proposes.
    /// Schema-2 packages must be installed or registered and run through the
    /// registered operation. Schema-1 packages are refused until migrated.
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
        input_conflict: basecraft_ops::records::SavePolicy,
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
enum RecordsCommand {
    /// Rebuild the analysis indexes from the project's analysis records, after
    /// an index was damaged, deleted or left out of a copy.
    Reindex { project: PathBuf },
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
            if let Some(error) = e.downcast_ref::<basecraft_ops::Error>() {
                eprintln!("{}", save::error_value(error));
            } else {
                eprintln!("basecraft: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

/// A recoverable CLI session. Its edits are recorded as made by the CLI
/// script client, not by a person or an assistant.
pub(crate) fn script_session(
    root: impl Into<PathBuf>,
    project: Option<&basecraft_ops::project::Project>,
) -> Result<basecraft_ops::Session, basecraft_ops::Error> {
    let mut session = basecraft_ops::Session::recoverable(root)?.in_project(project.cloned());
    session.set_actor(basecraft_protocol::Actor::Script {
        client: "CLI".into(),
    });
    Ok(session)
}

/// A CLI session for a command whose folder argument is the project.
pub(crate) fn project_session(
    project: &std::path::Path,
) -> Result<basecraft_ops::Session, basecraft_ops::Error> {
    let project = basecraft_ops::project::Project::new(project)?;
    let mut session =
        basecraft_ops::Session::recoverable(project.root())?.in_project(Some(project));
    session.set_actor(basecraft_protocol::Actor::Script {
        client: "CLI".into(),
    });
    Ok(session)
}

/// Parse an input-conflict decision by its shared serialized name.
fn save_policy(name: &str) -> Result<basecraft_ops::records::SavePolicy, String> {
    serde_json::from_value(serde_json::Value::from(name)).map_err(|_| {
        "expected cancel, preserve_copy, remove_analysis or break_evidence".to_string()
    })
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let project = cli
        .project
        .as_deref()
        .map(basecraft_ops::project::Project::new)
        .transpose()?;
    let save_options = cli.save_options;
    match cli.command {
        Command::Delimitation { action } => delimitation::run(action)?,
        Command::Plugins { action } => operations::plugins(action)?,
        Command::Operations { action } => operations::operations(action)?,
        Command::Metadata { action } => metadata::run(action, save_options)?,
        Command::Submission { action } => submission::run(action, save_options)?,
        Command::History { file, json } => {
            let report =
                basecraft_ops::records::DocumentRecord::saved_report(&file, project.as_ref())?;
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
            let report =
                basecraft_ops::records::DocumentRecord::saved_report(&file, project.as_ref())?;
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
            let restored = if verified_prefix {
                let (restored, problem) =
                    basecraft_ops::records::recover_verified_prefix(&session, &output)?;
                if let Some(problem) = problem {
                    eprintln!("Recovered the verified edits before damage: {problem}");
                }
                restored
            } else {
                basecraft_ops::records::recover(&session, &output)?
            };
            for restored in restored {
                println!("{}", restored.display());
            }
        }
        Command::Records {
            action: RecordsCommand::Reindex { project },
        } => {
            let done = basecraft_ops::records::reindex(&project)?;
            println!("{}", serde_json::to_string_pretty(&done)?);
        }
        Command::Provenance { action } => match action {
            ProvenanceCommand::Inspect { result } => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&basecraft_ops::records::inspect(
                        &result,
                        project.as_ref()
                    )?)?
                );
            }
            ProvenanceCommand::Verify { result } => {
                let issues = basecraft_ops::records::verify(&result, project.as_ref())?;
                if !issues.is_empty() {
                    return Err(issues.join("\n").into());
                }
                println!("done: recorded hashes verified; replay verification is a separate check");
            }
            ProvenanceCommand::Methods { result, output } => {
                let publication = basecraft_ops::records::inspect(&result, project.as_ref())?;
                basecraft_ops::records::write_reports(&output, &publication)?;
            }
            ProvenanceCommand::Package { result, output } => {
                basecraft_ops::records::package(&result, &output, project.as_ref())?;
            }
        },
        Command::NewSequence {
            name,
            sequence,
            output,
        } => {
            let output = std::path::absolute(output)?;
            let mut session = script_session(
                output.parent().ok_or("Missing output folder")?,
                project.as_ref(),
            )?;
            let doc = session.create_sequence(&name, &sequence)?;
            save_options.save(&mut session, doc, Some(&output), true)?;
            println!("Created {}", output.display());
        }
        Command::Mcp {
            project,
            allow_writes,
            model_id,
            client_kind,
        } => mcp::serve(&project, allow_writes, client_kind, &model_id)?,
        Command::SaveCheck { file, output } => {
            let file = file.canonicalize()?;
            let mut session =
                script_session(file.parent().ok_or("File has no parent")?, project.as_ref())?;
            let doc = session.open(&file)?;
            let output = output.map(std::path::absolute).transpose()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&session.check_save(doc, output.as_deref())?)?
            );
        }
        Command::Edit { file, operation } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = script_session(root, project.as_ref())?;
            let doc = session.open(&file)?;
            let operations = match serde_json::from_str::<EditOperations>(&operation)? {
                EditOperations::One(op) => vec![*op],
                EditOperations::Many(ops) => ops,
            };
            for operation in operations {
                if matches!(
                    operation,
                    basecraft_protocol::Op::Undo { .. } | basecraft_protocol::Op::Redo { .. }
                ) {
                    session.apply_history(doc, &operation)?;
                } else {
                    basecraft_ops::renames::apply_op(&mut session, doc, operation)?;
                }
            }
            save_options.save(&mut session, doc, None, false)?;
            // A reroot can leave labels on other groups; place them with tree.place_label.
            if let Ok(tree) = session.tree(doc)
                && let displaced = tree.displaced_labels()
                && !displaced.is_empty()
            {
                println!("{}", serde_json::json!({ "displaced_labels": displaced }));
            }
        }
        Command::ImportSupport { tree, source, name } => {
            let tree = tree.canonicalize()?;
            let mut session =
                script_session(tree.parent().ok_or("File has no parent")?, project.as_ref())?;
            let doc = session.open(&tree)?;
            let imported = session.import_support(doc, &source, &name)?;
            if imported.added.is_empty() {
                return Err("The source tree has no support values".into());
            }
            save_options.save(&mut session, doc, None, false)?;
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
            record,
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
            let mut session =
                script_session(file.parent().ok_or("File has no parent")?, project.as_ref())?;
            let doc = session.open(&file)?;
            session.extract(
                doc,
                (start, end),
                &segments,
                reverse,
                &output,
                record.as_deref(),
            )?;
            println!(
                "{}",
                serde_json::json!({"output":output,"hash":basecraft_io::hash_file(&output)?})
            );
        }
        Command::Alphabet { file } => {
            let file = file.canonicalize()?;
            let root = file.parent().unwrap_or(std::path::Path::new("."));
            let mut session = script_session(root, project.as_ref())?;
            let doc = session.open(&file)?;
            let alphabet = session.alphabet(doc)?;
            let sequences = session.sequence_alphabets(doc)?;
            println!(
                "{}",
                serde_json::json!({ "alphabet": alphabet, "sequences": sequences })
            );
        }
        Command::Index { file } => {
            let t = Instant::now();
            let f = basecraft_io::fasta::IndexedFasta::open(&file)?;
            let elapsed = t.elapsed();
            let residues: u64 = f.index.records.iter().map(|r| r.len).sum();
            let ragged = f
                .index
                .records
                .iter()
                .filter(|r| matches!(r.layout, basecraft_io::fasta::Layout::Ragged(_)))
                .count();
            println!(
                "{} records, {residues} residues, {ragged} ragged, index built in {:.1} ms",
                f.index.records.len(),
                elapsed.as_secs_f64() * 1e3
            );
        }
        Command::Replay { project } => {
            let report = basecraft_ops::records::replay(&project)?;
            for p in &report.problems {
                println!("problem: {p}");
            }
            println!(
                "{} sessions, {} entries, {} not checkable (original replaced), {} problems",
                report.sessions,
                report.entries,
                report.unchecked,
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
            save_options,
            project.as_ref(),
        )?,
        Command::Schemas { dir } => {
            std::fs::create_dir_all(&dir)?;
            let schemas = [
                (
                    "sanger-saved-review",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SavedReview),
                ),
                (
                    "sanger-saved-call-sets",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SavedCallSets),
                ),
                (
                    "sanger-review-evidence",
                    schemars::schema_for!(basecraft_protocol::sanger_records::ReviewEvidence),
                ),
                (
                    "sanger-review-source",
                    schemars::schema_for!(basecraft_protocol::sanger_records::ReviewSource),
                ),
                (
                    "sanger-review-support",
                    schemars::schema_for!(basecraft_protocol::sanger_records::ReviewSupport),
                ),
                (
                    "sanger-review-choice",
                    schemars::schema_for!(basecraft_protocol::sanger::ReviewChoice),
                ),
                (
                    "sanger-mask-settings",
                    schemars::schema_for!(basecraft_protocol::sanger::MaskSettings),
                ),
                (
                    "sanger-review-op",
                    schemars::schema_for!(basecraft_protocol::sanger::ReviewOp),
                ),
                (
                    "sanger-review-settings",
                    schemars::schema_for!(basecraft_protocol::sanger::ReviewSettings),
                ),
                (
                    "sanger-review-reference",
                    schemars::schema_for!(basecraft_protocol::sanger::ReviewReference),
                ),
                (
                    "alignment-link",
                    schemars::schema_for!(basecraft_protocol::records::AlignmentLink),
                ),
                (
                    "alignment-link-recovery",
                    schemars::schema_for!(basecraft_protocol::link_records::AlignmentLinkRecovery),
                ),
                (
                    "alignment-link-unavailable",
                    schemars::schema_for!(
                        basecraft_protocol::link_records::AlignmentLinkUnavailable
                    ),
                ),
                (
                    "collection-intent",
                    schemars::schema_for!(
                        basecraft_protocol::publication_records::CollectionIntent
                    ),
                ),
                (
                    "collection-export-intent",
                    schemars::schema_for!(
                        basecraft_protocol::publication_records::CollectionExportIntent
                    ),
                ),
                (
                    "member-export-receipt",
                    schemars::schema_for!(
                        basecraft_protocol::publication_records::MemberExportReceipt
                    ),
                ),
                (
                    "provenance-report-manifest",
                    schemars::schema_for!(
                        basecraft_protocol::report_records::ProvenanceReportManifest
                    ),
                ),
                (
                    "report-export-manifest",
                    schemars::schema_for!(basecraft_protocol::report_records::ReportExportManifest),
                ),
                (
                    "report-source-map",
                    schemars::schema_for!(
                        Vec<basecraft_protocol::report_records::ReportSourceMapEntry>
                    ),
                ),
                (
                    "metadata-manifest",
                    schemars::schema_for!(basecraft_protocol::metadata_records::MetadataManifest),
                ),
                (
                    "query-preparation",
                    schemars::schema_for!(basecraft_protocol::query_records::QueryPlan),
                ),
                (
                    "document-record",
                    schemars::schema_for!(basecraft_protocol::report_records::DocumentRecord),
                ),
                (
                    "document-report",
                    schemars::schema_for!(basecraft_protocol::report_records::Report),
                ),
                (
                    "validated-artifacts",
                    schemars::schema_for!(Vec<(String, std::path::PathBuf, String)>),
                ),
                (
                    "workflow-stages",
                    schemars::schema_for!(Vec<basecraft_protocol::operation::OperationOutcome>),
                ),
                (
                    "submission-package",
                    schemars::schema_for!(
                        basecraft_protocol::submission_records::SubmissionPackage
                    ),
                ),
                (
                    "submission-validation",
                    schemars::schema_for!(
                        basecraft_protocol::submission_records::ValidationSummary
                    ),
                ),
                (
                    "operation-failure",
                    schemars::schema_for!(basecraft_protocol::operation::OperationFailure),
                ),
                (
                    "activity-input-run",
                    schemars::schema_for!(basecraft_protocol::records::ActivityInputRun),
                ),
                (
                    "activity-contributions",
                    schemars::schema_for!(basecraft_protocol::records::ActivityContributions),
                ),
                (
                    "extraction-record",
                    schemars::schema_for!(basecraft_protocol::records::ExtractionRecord),
                ),
                (
                    "journal-entry",
                    schemars::schema_for!(basecraft_protocol::records::JournalEntry),
                ),
                (
                    "workflow-failure",
                    schemars::schema_for!(basecraft_protocol::records::WorkflowFailure),
                ),
                (
                    "companion",
                    schemars::schema_for!(basecraft_protocol::companion_records::Companion),
                ),
                (
                    "sequence-arrow-pair",
                    schemars::schema_for!(basecraft_protocol::plugin::ArrowPair),
                ),
                (
                    "sanderson-report",
                    schemars::schema_for!(basecraft_protocol::sanderson_records::SandersonReport),
                ),
                (
                    "query-source",
                    schemars::schema_for!(Vec<basecraft_protocol::query_records::Query>),
                ),
                (
                    "haplotypes",
                    schemars::schema_for!(basecraft_protocol::producer_records::Haplotypes),
                ),
                (
                    "delimitation-progress",
                    schemars::schema_for!(
                        basecraft_protocol::producer_records::DelimitationProgress
                    ),
                ),
                (
                    "delimitation-report",
                    schemars::schema_for!(basecraft_protocol::producer_records::DelimitationReport),
                ),
                (
                    "mptp-failure",
                    schemars::schema_for!(basecraft_protocol::producer_records::MptpFailure),
                ),
                (
                    "blast-receipt",
                    schemars::schema_for!(basecraft_protocol::producer_records::BlastReceipt),
                ),
                (
                    "distance-matrix",
                    schemars::schema_for!(basecraft_protocol::distance_records::DistanceMatrix),
                ),
                (
                    "identified-distance-matrix",
                    schemars::schema_for!(basecraft_protocol::distance_records::IdentifiedMatrix),
                ),
                (
                    "distance-cache-header",
                    schemars::schema_for!(basecraft_protocol::distance_records::CacheHeader),
                ),
                (
                    "distance-bootstrap",
                    schemars::schema_for!(basecraft_protocol::distance_records::BootstrapReport),
                ),
                (
                    "distance-run-report",
                    schemars::schema_for!(basecraft_protocol::distance_records::DistanceRunReport),
                ),
                (
                    "distance-group-report",
                    schemars::schema_for!(
                        basecraft_protocol::distance_records::DistanceGroupReport
                    ),
                ),
                (
                    "mafft-addition",
                    schemars::schema_for!(basecraft_protocol::producer_records::MafftAddition),
                ),
                (
                    "primer-report",
                    schemars::schema_for!(basecraft_protocol::producer_records::PrimerReport),
                ),
                (
                    "zenodo-selection",
                    schemars::schema_for!(basecraft_protocol::producer_records::ZenodoSelection),
                ),
                (
                    "zenodo-receipt",
                    schemars::schema_for!(basecraft_protocol::producer_records::ZenodoReceipt),
                ),
                (
                    "iqtree-recovery",
                    schemars::schema_for!(basecraft_protocol::producer_records::IqtreeRecovery),
                ),
                (
                    "sanger-draft",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SangerDraft),
                ),
                (
                    "sanger-placement",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SangerPlacement),
                ),
                (
                    "sanger-request",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SangerRequest),
                ),
                (
                    "sanger-job-record",
                    schemars::schema_for!(basecraft_protocol::sanger_records::SangerJobRecord),
                ),
                ("report-citations", schemars::schema_for!(Vec<String>)),
                (
                    "submitted-input",
                    schemars::schema_for!(basecraft_protocol::input_records::SubmittedInput),
                ),
                (
                    "analysis-run",
                    schemars::schema_for!(basecraft_protocol::records::AnalysisRun),
                ),
                (
                    "recovery-integrity-error",
                    schemars::schema_for!(basecraft_protocol::records::RecoveryIntegrityError),
                ),
                (
                    "result-binding",
                    schemars::schema_for!(basecraft_protocol::records::ResultBinding),
                ),
                (
                    "provenance-export-map",
                    schemars::schema_for!(basecraft_protocol::export_records::ProvenanceExportMap),
                ),
                (
                    "plugin-installation-index",
                    schemars::schema_for!(
                        basecraft_protocol::installation_records::InstallationIndex
                    ),
                ),
                (
                    "managed-plugin-runtime",
                    schemars::schema_for!(basecraft_protocol::installation_records::Runtime),
                ),
                (
                    "run-record",
                    schemars::schema_for!(basecraft_protocol::records::RunRecord),
                ),
                (
                    "local-job-index",
                    schemars::schema_for!(basecraft_protocol::records::LocalJobIndex),
                ),
                (
                    "store-marker",
                    schemars::schema_for!(basecraft_protocol::records::StoreMarker),
                ),
                (
                    "analysis-index",
                    schemars::schema_for!(basecraft_protocol::records::AnalysisIndex),
                ),
                (
                    "indexed-folders",
                    schemars::schema_for!(basecraft_protocol::records::IndexedFolders),
                ),
                (
                    "pending-save",
                    schemars::schema_for!(basecraft_protocol::records::PendingSave),
                ),
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
                    schemars::schema_for!(basecraft_protocol::records::JobRecord),
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
            let f = basecraft_io::fasta::IndexedFasta::open(&file)?;
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
    save: Option<basecraft_ops::records::SavePolicy>,
    mut save_options: save::Options,
    project: Option<&basecraft_ops::project::Project>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(policy) = save {
        use basecraft_ops::records::SavePolicy;
        if policy != SavePolicy::Cancel {
            if save_options.save_known_policy != SavePolicy::Cancel
                && save_options.save_known_policy != policy
            {
                return Err("Conflicting --input-conflict and --save-known-policy".into());
            }
            save_options.save_known_policy = policy;
        }
    }
    let mut map = serde_json::Map::new();
    for p in params {
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| format!("parameter {p:?} is not key=value"))?;
        let value = serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.into()));
        map.insert(k.to_string(), value);
    }
    let manifest = basecraft_plugin_host::Manifest::load(dir)?;
    run_registered_plugin(
        &manifest,
        file,
        operation,
        map,
        save.is_some().then_some(save_options),
        project,
    )
}

/// `plugin` for a schema-2 package: the registered sequence operation through the
/// same job runner as `operations run`, against the file opened in this session.
fn run_registered_plugin(
    manifest: &basecraft_plugin_host::Manifest,
    file: &std::path::Path,
    operation: &str,
    parameters: serde_json::Map<String, serde_json::Value>,
    save: Option<save::Options>,
    project: Option<&basecraft_ops::project::Project>,
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
    let mut session = script_session(root, project)?;
    basecraft_platform::interrupt::install()?;
    let (outcome, target) =
        operations::run_foreground(&registry, &request, policy, "CLI", &mut session)?;
    println!("{outcome}");
    if outcome["state"] != "completed" {
        return Err(format!("{} did not complete", registered.id).into());
    }
    if let Some(policy) = save {
        policy.save(
            &mut session,
            target.ok_or("operation returned no target document")?,
            None,
            false,
        )?;
    }
    Ok(())
}
