//! `basecraft submission`: GenBank marker submissions from scripts.
//!
//! The same `basecraft_ops::submission` implementation the GUI and MCP use:
//! typed `submission.edit` operations (saved after each command), review,
//! package export, table2asn validation through the registered operation
//! runner, and returned accessions.
use std::io::Write as _;
use std::path::{Path, PathBuf};

use basecraft_ops::submission;
use basecraft_protocol::submission::{FrameChoice, SubmissionEdit};
use clap::Subcommand;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum SubmissionCommand {
    /// Create a submission document (`NAME.submission.json`) for an
    /// alignment inside the project.
    New {
        file: PathBuf,
        #[arg(long)]
        alignment: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Print the submission document as JSON.
    Show {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Prepare the records and print them with every local issue as JSON:
    /// extraction, specimen data, features and reading frames.
    Review {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Apply one `submission.edit` (JSON, such as
    /// `{"action":"set_rows","rows":[{"record":"x","decision":"exclude"}]}`)
    /// and save.
    Edit {
        file: PathBuf,
        edit: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Confirm reading frames: the detected frame of every record that has
    /// exactly one, or `--set RECORD=CODON_START`. Prints the frames.
    Frames {
        file: PathBuf,
        /// Confirm every uniquely detected frame not yet confirmed.
        #[arg(long)]
        confirm_detected: bool,
        /// RECORD=1|2|3, repeatable.
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Write the submission package (FASTA, source and feature tables,
    /// template, records, coordinate map, README) to a folder.
    Export {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Validate an exported package with table2asn (the registered
    /// operation basecraft.table2asn/validate@1) and record its reports.
    Validate {
        package: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Read the accession list NCBI returned and record it. Prints the plan;
    /// `--apply` records and saves.
    Accessions {
        file: PathBuf,
        list: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Write a publication table of records, specimens and accessions.
    AccessionTable {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Print the submitter preference; `--set` merges a JSON object of
    /// fields (first_name, last_name, email, institution, city, country, ...).
    Submitter {
        #[arg(long)]
        set: Option<String>,
    },
}

fn open(project: &Path, file: &Path) -> Result<(basecraft_ops::Session, basecraft_ops::DocId)> {
    let project = project.canonicalize()?;
    let mut session = crate::script_session(&project)?;
    let file = std::path::absolute(file)?;
    let doc = session.open(&file)?;
    if !session.is_submission(doc) {
        return Err(format!("{} is not a submission document", file.display()).into());
    }
    Ok((session, doc))
}

fn store() -> Result<basecraft_platform::settings::SettingsStore> {
    let paths = basecraft_platform::paths::AppPaths::discover()?;
    Ok(basecraft_platform::settings::SettingsStore::new(
        paths.preferences(),
    ))
}

/// Print any serializable value as pretty JSON.
macro_rules! json_out {
    ($value:expr) => {
        println!("{}", serde_json::to_string_pretty(&$value)?)
    };
}

pub fn run(command: SubmissionCommand) -> Result<()> {
    match command {
        SubmissionCommand::New {
            file,
            alignment,
            project,
        } => {
            let project = project.canonicalize()?;
            let mut session = crate::script_session(&project)?;
            let doc = submission::create(
                &mut session,
                &std::path::absolute(file)?,
                &std::path::absolute(alignment)?,
            )?;
            json_out!(session.submission(doc));
        }
        SubmissionCommand::Show { file, project } => {
            let (session, doc) = open(&project, &file)?;
            json_out!(session.submission(doc));
        }
        SubmissionCommand::Review { file, project } => {
            let (session, doc) = open(&project, &file)?;
            let submitter = store()?.load()?.submitter;
            json_out!(submission::review(&session, doc, &submitter)?);
        }
        SubmissionCommand::Edit {
            file,
            edit,
            project,
        } => {
            let edit: SubmissionEdit = serde_json::from_str(&edit)?;
            let (mut session, doc) = open(&project, &file)?;
            submission::edit(&mut session, doc, edit)?;
            session.save(doc, None)?;
            json_out!(session.submission(doc));
        }
        SubmissionCommand::Frames {
            file,
            confirm_detected,
            set,
            project,
        } => {
            let (mut session, doc) = open(&project, &file)?;
            let submitter = store()?.load()?.submitter;
            let mut frames = Vec::new();
            if confirm_detected {
                let review = submission::review(&session, doc, &submitter)?;
                for record in &review.prepared.records {
                    if let Some(frame) = &record.frame {
                        if let (None, Some(detected)) = (frame.chosen, frame.detected) {
                            frames.push(FrameChoice {
                                record: record.record.clone(),
                                codon_start: detected,
                            });
                        }
                    }
                }
            }
            for item in set {
                let (record, start) = item
                    .rsplit_once('=')
                    .ok_or_else(|| format!("{item:?} is not RECORD=CODON_START"))?;
                frames.push(FrameChoice {
                    record: record.into(),
                    codon_start: start.parse()?,
                });
            }
            if !frames.is_empty() {
                submission::edit(
                    &mut session,
                    doc,
                    SubmissionEdit::SetFrames {
                        frames,
                        clear: Vec::new(),
                    },
                )?;
                session.save(doc, None)?;
            }
            json_out!(session.submission(doc).frames);
        }
        SubmissionCommand::Export {
            file,
            output,
            project,
        } => {
            let (mut session, doc) = open(&project, &file)?;
            let submitter = store()?.load()?.submitter;
            let output = std::path::absolute(output)?;
            json_out!(submission::export(&mut session, doc, &output, &submitter)?);
        }
        SubmissionCommand::Validate { package, project } => {
            let project = project.canonicalize()?;
            let package = std::path::absolute(package)?;
            let request = submission::validation_request(&project, &package)?;
            let request: basecraft_protocol::operation::OperationRequest =
                serde_json::from_value(request)?;
            let registry = basecraft_plugin_host::registry::Registry::load(
                &basecraft_plugin_host::registry::RegistryConfig::discover()?,
            )?;
            let policy = basecraft_plugin_host::policy::SessionPolicy::new(&project, true)?;
            let mut session = crate::script_session(&project)?;
            basecraft_platform::interrupt::install()?;
            let (outcome, _) = crate::operations::run_foreground(
                &registry,
                &request,
                policy,
                "CLI",
                &mut session,
            )?;
            if outcome.get("state").is_none() {
                // A refused submission: the shared structured error.
                json_out!(outcome);
                return Err("table2asn validation was refused".into());
            }
            json_out!(submission::record_validation(&project, &package, &outcome,)?);
        }
        SubmissionCommand::Accessions {
            file,
            list,
            apply,
            project,
        } => {
            let text = std::fs::read_to_string(&list)?;
            let (mut session, doc) = open(&project, &file)?;
            let submitter = store()?.load()?.submitter;
            let plan = submission::plan_accessions(&session, doc, &text, &submitter)?;
            json_out!(plan);
            if apply && !plan.accessions.is_empty() {
                submission::edit(
                    &mut session,
                    doc,
                    SubmissionEdit::SetAccessions {
                        accessions: plan.accessions,
                    },
                )?;
                session.save(doc, None)?;
                eprintln!("basecraft: recorded and saved");
            }
        }
        SubmissionCommand::AccessionTable {
            file,
            output,
            project,
        } => {
            let (session, doc) = open(&project, &file)?;
            let submitter = store()?.load()?.submitter;
            let table = submission::accession_table(&session, doc, &submitter)?;
            basecraft_io::write_atomic(&output, |w| w.write_all(&table))?;
            eprintln!("basecraft: wrote {}", output.display());
        }
        SubmissionCommand::Submitter { set } => {
            let store = store()?;
            let mut submitter = store.load()?.submitter;
            if let Some(set) = set {
                let mut value = serde_json::to_value(&submitter)?;
                let fields: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(&set)?;
                for (name, field) in fields {
                    if value.get(&name).is_none() {
                        return Err(format!("{name} is not a submitter field").into());
                    }
                    value[name] = field;
                }
                submitter = serde_json::from_value(value)?;
                store.update(basecraft_platform::settings::Change::Submitter(Box::new(
                    submitter.clone(),
                )))?;
            }
            json_out!(serde_json::json!({
                "submitter": submitter,
                "missing": submitter.missing(),
            }));
        }
    }
    Ok(())
}
