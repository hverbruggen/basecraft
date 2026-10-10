//! Answers editors' requests for what is visible: feature lookups by range
//! and per-zoom sequence summaries.

pub mod interval;
pub mod summary;

pub use interval::IntervalIndex;
pub use summary::SeqSummary;
