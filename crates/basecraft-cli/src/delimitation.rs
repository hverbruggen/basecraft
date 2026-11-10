//! Thin adapters for the shared saved-hypothesis workflow.
use std::path::PathBuf;

use basecraft_ops::delimitation::{self, CombineSelection};
use clap::Subcommand;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum DelimitationCommand {
    /// Inspect hypotheses in recommendation order without writing records.
    Candidates { paths: Vec<PathBuf> },
    /// Suggest a free combined SPART filename beside the last input.
    Name { paths: Vec<PathBuf> },
    /// Combine selected saved hypotheses into a new SPART or SPART XML file.
    Combine {
        #[arg(required = true, num_args = 2..)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        /// JSON object mapping input paths to hypothesis IDs. Omit to keep all.
        #[arg(long)]
        selection: Option<String>,
        /// Root of this recoverable CLI session; no project membership is inferred.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}

pub fn run(command: DelimitationCommand) -> Result<()> {
    let value = match command {
        DelimitationCommand::Candidates { paths } => {
            serde_json::to_value(delimitation::combine_candidates(&paths)?)?
        }
        DelimitationCommand::Name { paths } => {
            serde_json::json!({"path": delimitation::combined_path(&paths)})
        }
        DelimitationCommand::Combine {
            paths,
            output,
            selection,
            project,
        } => {
            let paths = paths
                .into_iter()
                .map(absolute)
                .collect::<std::io::Result<Vec<_>>>()?;
            let output = absolute(output)?;
            let keep: Option<CombineSelection> =
                selection.map(|s| serde_json::from_str(&s)).transpose()?;
            let keep = keep
                .map(|keep| {
                    keep.into_iter()
                        .map(|(path, ids)| Ok((absolute(path)?, ids)))
                        .collect::<std::io::Result<CombineSelection>>()
                })
                .transpose()?;
            let mut session = crate::script_session(absolute(project)?)?;
            delimitation::combine_to(&mut session, &paths, &output, keep.as_ref())?;
            serde_json::json!({"output": output})
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
