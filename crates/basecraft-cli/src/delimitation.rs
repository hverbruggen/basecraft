//! Thin adapters for the shared saved-hypothesis workflow.
use std::path::PathBuf;

use basecraft_ops::delimitation::{self, CombineSelection};
use clap::Subcommand;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum DelimitationCommand {
    /// Inspect an unverified alignment hint; does not read or validate the hint.
    LinkedAlignment {
        path: PathBuf,
        /// Explicit root of the read-only session; no membership is inferred.
        #[arg(long)]
        project: PathBuf,
    },
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
    /// Attach the tree in TREE (one tree, Newick or NEXUS) as the display tree
    /// of the species workspace WORKSPACE and save it. The tree file is kept as
    /// the workspace's evidence.
    DisplayTree {
        workspace: PathBuf,
        #[arg(long)]
        tree: PathBuf,
    },
}

fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}

pub fn run(
    command: DelimitationCommand,
    project: Option<&basecraft_ops::project::Project>,
    save_options: crate::save::Options,
) -> Result<()> {
    let value = match command {
        DelimitationCommand::LinkedAlignment { path, project } => {
            let project = basecraft_ops::project::Project::new(absolute(project)?)?;
            let mut session =
                basecraft_ops::Session::unlogged(project.root()).in_project(Some(project));
            let doc = session.open(&absolute(path)?)?;
            serde_json::json!({"path": delimitation::linked_alignment(&session, doc)?})
        }
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
            let output = absolute(output)?;
            let keep: Option<CombineSelection> =
                selection.map(|s| serde_json::from_str(&s)).transpose()?;
            // Existing inputs and selection aliases are resolved together by
            // the shared workflow. Output/project handling stays unchanged.
            let mut session = crate::project_session(&absolute(project)?)?;
            delimitation::combine_to(&mut session, &paths, &output, keep.as_ref())?;
            serde_json::json!({"output": output})
        }
        DelimitationCommand::DisplayTree { workspace, tree } => {
            let workspace = workspace.canonicalize()?;
            let root = workspace.parent().ok_or("the workspace has no folder")?;
            let mut session = crate::script_session(root.to_path_buf(), project)?;
            let doc = session.open(&workspace)?;
            delimitation::attach_display_tree(&mut session, doc, &absolute(tree)?)?;
            save_options.save(&mut session, doc, None, false)?;
            let shown = &session.delimitation(doc)?.view.display_tree;
            serde_json::json!({"workspace": workspace, "display_tree": shown})
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
