//! Edit models for open documents: piece tables, coordinate maps,
//! sequence documents whose annotations move with edits, and alignments.

pub mod alignment;
pub mod coord;
pub mod piece;
pub mod sequence;
pub mod tree;

pub use alignment::AlignmentDoc;
pub use coord::{Bias, Change};
pub use piece::{PieceTable, Residues};
pub use sequence::{EditResult, FeatureId, SequenceDoc};
pub use tree::{SupportSet, TreeDoc, TreeNode};
