//! Editor view models and input models.
//!
//! Computes what each editor draws for the visible region and turns input
//! events into operations. No Qt or GPU code lives here.

pub mod alignment;
pub mod delimitation;
pub mod find;
pub mod frame;
pub mod genome;
pub mod sanger;
pub mod sanger_alignment;
pub mod sanger_map;
pub mod sanger_review;
pub mod theme;
pub mod tree;
pub mod zoom;

pub use alignment::AlignmentView;
pub use frame::{Frame, Rgba};
pub use genome::{GenomeView, Mode};
pub use theme::Theme;
pub use tree::TreeView;

pub mod svg;

pub mod sanger_triage;

pub mod accessible;

pub mod commands;
pub mod overview;
pub mod species_analysis;
pub mod workspace;
