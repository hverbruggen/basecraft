//! Editor view models and input models.
//!
//! Computes what each editor draws for the visible region and turns input
//! events into operations. No Qt or GPU code lives here.

pub mod frame;
pub mod genome;
pub mod theme;

pub use frame::{Frame, Rgba};
pub use genome::{GenomeView, Mode};
pub use theme::Theme;
