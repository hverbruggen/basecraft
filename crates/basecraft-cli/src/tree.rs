//! Tree edits a script needs, by name (D1): reroot on an outgroup, ladderize,
//! rotate a clade. Each is one typed edit, applied and saved like
//! `basecraft edit`, and the same edit the window makes.
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
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Order {
    /// Smallest clades first (on top).
    SmallestFirst,
    /// Largest clades first.
    LargestFirst,
}

/// The file and the one edit a tree command asks for.
pub fn operation(command: TreeCommand) -> (PathBuf, Op) {
    match command {
        TreeCommand::Reroot { file, outgroup } => (file, Op::TreeReroot { clade: outgroup }),
        TreeCommand::Ladderize { file, order } => (
            file,
            Op::TreeLadderize {
                small_first: matches!(order, Order::SmallestFirst),
            },
        ),
        TreeCommand::Rotate { file, clade } => (file, Op::TreeRotate { clade }),
    }
}
