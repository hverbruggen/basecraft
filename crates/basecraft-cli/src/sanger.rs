//! Sanger review decisions as commands (D1). A review starts from a read's
//! instrument calls and is saved as a review file; decisions and the consensus
//! export then work on that file. The window makes the same calls.
use std::path::{Path, PathBuf};

use basecraft_ops::sanger_commands;
use basecraft_protocol::sanger::ReviewOp;
use clap::Subcommand;
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum SangerCommand {
    /// Start a review of READ (.ab1) from its instrument calls and save it as
    /// a new review file.
    Review {
        read: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Decide draft columns of a saved review and save it: accept the call,
    /// reject it, set it back to pending, or give the base to put there.
    Decide {
        review: PathBuf,
        /// Draft columns from 1: `12`, `12,15` or `12-14`.
        #[arg(long)]
        columns: String,
        /// `accept`, `reject`, `pending`, or a base (IUPAC, `-` for a gap).
        #[arg(long)]
        choice: String,
    },
    /// Export the reviewed consensus as FASTA. Every column must be decided.
    Export {
        review: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// The sequence's name in the FASTA file, such as the specimen ID.
        #[arg(long)]
        label: String,
    },
    /// Add the reviewed consensus to ALIGNMENT as a new last row and save it.
    /// Every column must be decided; the review is recorded as the row's
    /// evidence. The row is not placed: align afterwards (`run mafft/align`).
    Add {
        review: PathBuf,
        #[arg(long)]
        alignment: PathBuf,
        /// The row's name, such as the specimen ID.
        #[arg(long)]
        label: String,
    },
}

fn session_for(
    file: &Path,
    project: Option<&basecraft_ops::project::Project>,
) -> Result<(basecraft_ops::Session, PathBuf)> {
    let file = file.canonicalize()?;
    let root = file.parent().ok_or("the file has no folder")?.to_path_buf();
    Ok((crate::script_session(root, project)?, file))
}

pub fn run(
    command: SangerCommand,
    project: Option<&basecraft_ops::project::Project>,
    save_options: crate::save::Options,
) -> Result<()> {
    let value = match command {
        SangerCommand::Review { read, output } => {
            let (mut session, read) = session_for(&read, project)?;
            let review = sanger_commands::review_read(&mut session, &read)?;
            session.save_new(review, Some(&output))?;
            let reviewed = session.sanger_review(review)?;
            json!({"review": output, "columns": reviewed.draft.columns.len(),
                "unresolved": reviewed.unresolved()})
        }
        SangerCommand::Decide {
            review,
            columns,
            choice,
        } => {
            let edit = ReviewOp::Set {
                columns: sanger_commands::columns(&columns)?,
                choice: sanger_commands::choice(&choice)?,
            };
            let (mut session, file) = session_for(&review, project)?;
            let doc = session.open(&file)?;
            sanger_commands::decide(&mut session, doc, edit)?;
            save_options.save(&mut session, doc, None, false)?;
            json!({"review": file, "unresolved": session.sanger_review(doc)?.unresolved()})
        }
        SangerCommand::Export {
            review,
            output,
            label,
        } => {
            let (mut session, file) = session_for(&review, project)?;
            let doc = session.open(&file)?;
            session.export_sanger_consensus(doc, &output, &label)?;
            json!({"consensus": output})
        }
        SangerCommand::Add {
            review,
            alignment,
            label,
        } => {
            let (mut session, alignment) = session_for(&alignment, project)?;
            let review = session.open(&review.canonicalize()?)?;
            let doc = session.open(&alignment)?;
            sanger_commands::add_consensus(&mut session, review, doc, &label)?;
            save_options.save(&mut session, doc, None, false)?;
            json!({"alignment": alignment, "rows": session.alignment(doc)?.row_count()})
        }
    };
    println!("{value}");
    Ok(())
}
