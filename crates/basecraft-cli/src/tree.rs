//! Tree edits a script needs, by name (D1): reroot on an outgroup, ladderize,
//! rotate a clade, label tips from the specimen table. Each is one typed edit,
//! applied and saved like `basecraft edit`, and the same edit the window makes.
use std::path::PathBuf;

use basecraft_protocol::Op;
use clap::{Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum TreeCommand {
    /// Root the tree on the branch above the outgroup: one tip, or the
    /// smallest clade holding every tip given.
    Reroot {
        file: PathBuf,
        /// An outgroup tip; repeat for a clade.
        #[arg(long, required = true)]
        outgroup: Vec<String>,
    },
    /// Order every node's branches by size.
    Ladderize {
        file: PathBuf,
        #[arg(long, value_enum)]
        order: Order,
    },
    /// Reverse the order of a clade's branches.
    Rotate {
        file: PathBuf,
        /// The clade: one tip, or tips whose smallest clade it is; repeat.
        #[arg(long, required = true)]
        clade: Vec<String>,
    },
    /// Label the tips from the specimen table of the project (`--project`).
    /// Tips whose link is missing or ambiguous keep their own name.
    Labels {
        file: PathBuf,
        /// Fields in order, comma-separated: specimen, genus, species,
        /// accession, column:NAME.
        #[arg(long, required = true, value_delimiter = ',')]
        fields: Vec<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Order {
    /// Smallest clades first (on top).
    SmallestFirst,
    /// Largest clades first.
    LargestFirst,
}

pub fn run(
    command: TreeCommand,
    project: Option<&basecraft_ops::project::Project>,
    save_options: crate::save::Options,
) -> Result<(), Box<dyn std::error::Error>> {
    let (file, operation) = match command {
        TreeCommand::Reroot { file, outgroup } => (file, Op::TreeReroot { clade: outgroup }),
        TreeCommand::Ladderize { file, order } => (
            file,
            Op::TreeLadderize {
                small_first: matches!(order, Order::SmallestFirst),
            },
        ),
        TreeCommand::Rotate { file, clade } => (file, Op::TreeRotate { clade }),
        TreeCommand::Labels { file, fields } => {
            let project = project.ok_or(basecraft_ops::Error::NoProject(
                "Labelling tips from the specimen table",
            ))?;
            let fields = fields
                .iter()
                .map(|field| basecraft_ops::metadata::label_field(field))
                .collect::<Result<Vec<_>, _>>()?;
            let file = file.canonicalize()?;
            let root = file.parent().ok_or("the tree has no folder")?;
            let mut session = crate::script_session(root, Some(project))?;
            let doc = session.open(&file)?;
            let kept = basecraft_ops::metadata::label_tips(&mut session, doc, project, fields)?;
            save_options.save(&mut session, doc, None, false)?;
            println!(
                "{}",
                serde_json::json!({"tree": file, "kept_own_name": kept})
            );
            return Ok(());
        }
    };
    crate::edit_file(&file, vec![operation], project, save_options)
}
