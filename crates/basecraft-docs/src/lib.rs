//! Edit models for open documents: piece tables, coordinate maps, and
//! sequence documents whose annotations move with edits.

pub mod coord;
pub mod piece;
pub mod sequence;

pub use coord::{Bias, Change};
pub use piece::{PieceTable, Residues};
pub use sequence::{EditResult, FeatureId, SequenceDoc};
