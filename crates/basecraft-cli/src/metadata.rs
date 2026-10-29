//! `basecraft metadata`: the project's specimen metadata from scripts.
//!
//! Imports print a reviewable plan as JSON; `--apply` applies it as one
//! `metadata.edit` and saves. Planning, storage and edits are the shared
//! `basecraft_ops::metadata` implementation the GUI and MCP use.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_ops::metadata::{self, HeaderImport, ImportOptions, SheetImport};
use basecraft_protocol::metadata::{FieldTarget, HeaderPattern, MetadataEdit};
use clap::{Args, Subcommand};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum MetadataCommand {
    /// Inspect partitions/labels or plan explicit workspace links and partition promotion.
    /// Request is JSON with action `partitions`, `labels`, `link_records`,
    /// `link_workspace` or `promote_partition`.
    Consumer {
        request: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Apply a planned edit and save using the shared Session.
        #[arg(long)]
        apply: bool,
    },
    /// Create empty metadata tables in the project's `metadata/` folder.
    Init {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Print the metadata as JSON: roster, taxa, identifications and links.
    Show {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Plan (or with --apply, apply) an import from structured FASTA headers.
    ImportHeaders {
        /// FASTA file inside the project.
        file: PathBuf,
        /// Field separator, such as `|`.
        #[arg(long)]
        separator: String,
        /// What each field holds, comma-separated: `specimen`, `taxon`,
        /// `ignore` (or `-`) or a column name such as `eventDate`.
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
        /// Remember the pattern under this name.
        #[arg(long)]
        save_pattern: Option<String>,
        #[command(flatten)]
        import: ImportArgs,
    },
    /// Plan (or with --apply, apply) an import from a spreadsheet
    /// (.ods, .xlsx, .csv or .tsv).
    ImportSheet {
        file: PathBuf,
        /// `HEADER=TARGET`, repeatable. TARGET is `specimen`, `record`,
        /// `taxon`, `ignore` or a column name. Without any, the mapping is
        /// guessed from the headers.
        #[arg(long = "map")]
        mapping: Vec<String>,
        /// Sequence file whose records the `record` column names.
        #[arg(long)]
        document: Option<PathBuf>,
        /// Remember the mapping under this name.
        #[arg(long)]
        save_mapping: Option<String>,
        #[command(flatten)]
        import: ImportArgs,
    },
    /// Write a pre-filled .ods template for every record of a sequence file.
    Template {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// List a sequence file's records that are not linked to a specimen.
    Status {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Print the INSDC source qualifiers derived for a specimen.
    Insdc {
        specimen: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

#[derive(Args)]
pub struct ImportArgs {
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Leave rows for specimens the roster lacks unmatched.
    #[arg(long)]
    no_new_specimens: bool,
    /// Replace existing values and links that differ.
    #[arg(long)]
    overwrite: bool,
    /// Apply the plan and save, instead of only printing it.
    #[arg(long)]
    apply: bool,
}

impl ImportArgs {
    fn options(&self) -> ImportOptions {
        ImportOptions {
            create_specimens: !self.no_new_specimens,
            overwrite: self.overwrite,
        }
    }
}

/// A field or column target by its CLI name.
fn target(name: &str) -> Result<FieldTarget> {
    Ok(match name.trim() {
        "specimen" | "specimen_id" => FieldTarget::Specimen,
        "record" => FieldTarget::Record,
        "taxon" | "organism" => FieldTarget::Taxon,
        "ignore" | "-" | "" => FieldTarget::Ignore,
        column if metadata::valid_column_name(column) => FieldTarget::Column {
            column: column.into(),
        },
        other => return Err(format!("{other:?} is not a field target").into()),
    })
}

fn open(project: &Path, create: bool) -> Result<(basecraft_ops::Session, basecraft_ops::DocId)> {
    let project = project.canonicalize()?;
    let mut session = crate::script_session(&project)?;
    let doc = metadata::open(&mut session, create)?
        .ok_or("this project has no metadata yet; run `basecraft metadata init`")?;
    for notice in session.metadata_notices(doc) {
        eprintln!("basecraft: {notice}");
    }
    Ok((session, doc))
}

fn project_path(project: &Path, file: &Path) -> Result<String> {
    let file = std::path::absolute(file)?;
    Ok(metadata::document_key(&project.canonicalize()?, &file)?)
}

fn finish(
    mut session: basecraft_ops::Session,
    doc: basecraft_ops::DocId,
    plan: metadata::Plan,
    apply: bool,
    remember: Option<MetadataEdit>,
) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&plan)?);
    if !apply {
        return Ok(());
    }
    session.begin_group();
    let applied = (|| -> Result<()> {
        if !plan.changes.is_empty() {
            metadata::apply_plan(&mut session, doc, plan.changes)?;
        }
        if let Some(edit) = remember {
            session.apply(
                doc,
                basecraft_protocol::Op::MetadataEdit {
                    edit: Box::new(edit),
                },
            )?;
        }
        Ok(())
    })();
    session.end_group();
    applied?;
    session.save(doc, None)?;
    eprintln!("basecraft: applied and saved");
    Ok(())
}

pub fn run(command: MetadataCommand) -> Result<()> {
    match command {
        MetadataCommand::Consumer {
            request,
            project,
            apply,
        } => {
            let (mut session, doc) = open(&project, false)?;
            let request = serde_json::from_str(&request)?;
            let plan = metadata::consumer_plan(&session, doc, &request)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
            if apply {
                let operation = plan.get("operation").ok_or("this request is read-only")?;
                let target = plan["target"].as_str().ok_or("missing plan target")?;
                let target_doc = if target == session.doc_key(doc) {
                    doc
                } else {
                    session.open(&session.root().join(target))?
                };
                session.apply(target_doc, serde_json::from_value(operation.clone())?)?;
                session.save(target_doc, None)?;
            }
        }
        MetadataCommand::Init { project } => {
            let manifest = metadata::create(&project.canonicalize()?)?;
            println!("{}", manifest.display());
        }
        MetadataCommand::Show { project } => {
            let (session, doc) = open(&project, false)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "metadata": session.metadata(doc),
                    "notices": session.metadata_notices(doc),
                }))?
            );
        }
        MetadataCommand::ImportHeaders {
            file,
            separator,
            fields,
            save_pattern,
            import,
        } => {
            let document = project_path(&import.project, &file)?;
            let (session, doc) = open(&import.project, true)?;
            let pattern = HeaderPattern {
                name: save_pattern.clone().unwrap_or_else(|| "CLI".into()),
                separator,
                fields: fields.iter().map(|f| target(f)).collect::<Result<_>>()?,
            };
            let request = HeaderImport {
                document,
                pattern: pattern.clone(),
                options: import.options(),
            };
            let plan = metadata::plan_header_import(&session, doc, &request)?;
            let remember = save_pattern.map(|name| {
                let mut settings = session.metadata(doc).import.clone();
                settings.header_patterns.retain(|p| p.name != name);
                settings.header_patterns.push(pattern);
                MetadataEdit::SetImportSettings { settings }
            });
            finish(session, doc, plan, import.apply, remember)?;
        }
        MetadataCommand::ImportSheet {
            file,
            mapping,
            document,
            save_mapping,
            import,
        } => {
            let document = document
                .map(|d| project_path(&import.project, &d))
                .transpose()?;
            let (session, doc) = open(&import.project, true)?;
            let mapping = if mapping.is_empty() {
                None
            } else {
                Some(
                    mapping
                        .iter()
                        .map(|m| {
                            let (header, name) = m
                                .split_once('=')
                                .ok_or_else(|| format!("{m:?} is not HEADER=TARGET"))?;
                            Ok((header.to_string(), target(name)?))
                        })
                        .collect::<Result<BTreeMap<_, _>>>()?,
                )
            };
            let request = SheetImport {
                path: std::path::absolute(&file)?.to_string_lossy().into_owned(),
                mapping,
                document,
                options: import.options(),
            };
            let plan = metadata::plan_sheet_import(&session, doc, &request)?;
            let remember = save_mapping.map(|name| {
                let mut settings = session.metadata(doc).import.clone();
                settings.spreadsheet_mappings.retain(|m| m.name != name);
                settings.spreadsheet_mappings.push(
                    basecraft_protocol::metadata::SpreadsheetMapping {
                        name,
                        columns: plan.mapping.clone(),
                    },
                );
                MetadataEdit::SetImportSettings { settings }
            });
            finish(session, doc, plan, import.apply, remember)?;
        }
        MetadataCommand::Template {
            file,
            output,
            project,
        } => {
            let document = project_path(&project, &file)?;
            let (session, doc) = open(&project, false)?;
            let records = metadata::template_for(&session, doc, &document, &output)?;
            eprintln!("basecraft: wrote {records} records to {}", output.display());
        }
        MetadataCommand::Status { file, project } => {
            let document = project_path(&project, &file)?;
            let (session, doc) = open(&project, false)?;
            let status = metadata::link_status(&session, doc, &document)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        MetadataCommand::Insdc { specimen, project } => {
            let (session, doc) = open(&project, false)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&metadata::insdc(&session, doc, &specimen)?)?
            );
        }
    }
    Ok(())
}
