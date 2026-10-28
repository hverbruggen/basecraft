//! Edit models for open documents: piece tables, coordinate maps,
//! sequence documents whose annotations move with edits, and alignments.

pub mod alignment;
pub mod coord;
pub mod delimitation;
pub mod genetic_code;
pub mod metadata;
pub mod piece;
pub mod sanger;
pub mod sequence;
pub mod submission;
pub mod tree;

pub use alignment::AlignmentDoc;
pub use coord::{Bias, Change};
pub use piece::{PieceTable, Residues};
pub use sequence::{EditResult, FeatureId, SequenceDoc};
pub use tree::{SupportSet, TreeDoc, TreeNode};
