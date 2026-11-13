//! Existing alignment Save As recovery and unavailable-guard writer shapes.
//!
//! These are producer contracts, not automatic repair/read acceptance policies.
//! Serialize through `serde_json::Value` for recursive sorting, then pretty JSON
//! without a final newline. Intended associations remain opaque serialized bytes.
use std::{ffi::OsString, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Prior directory entry, preserved without decoding bytes or following symlinks.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "entry", content = "value", rename_all = "snake_case")]
pub enum PreviousAlignmentMetadata {
    Absent,
    File(Vec<u8>),
    Symlink(PathBuf),
}

/// Uniquely retained `*.alignment-recovery.json`, kept with the tree's link
/// records in `.basecraft/documents`; schema 1, no rewrite workflow.
/// Deserialization describes known writer shapes only. It is not an extension-
/// preserving manual repair interface or a substitute for the selective readers.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AlignmentLinkRecovery {
    pub schema: u32,
    pub note: String,
    pub previous_origin: PreviousAlignmentMetadata,
    pub previous_view: PreviousAlignmentMetadata,
    /// Always emitted, including null. Never decode/re-encode these bytes.
    pub intended_origin: Option<Vec<u8>>,
    pub intended_view: Option<Vec<u8>>,
}

/// Temporary origin/view guard; production keeps its selected-field Value reader.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AlignmentLinkUnavailable {
    pub schema: u32,
    pub unavailable: String,
    /// Existing native `OsStr` basename representation (Unix bytes / Windows units),
    /// or null. The selected-field reader only displays a recovery hint for a
    /// string supplied in an externally authored guard; it does not decode this.
    #[schemars(with = "Option<NativeFilename>")]
    pub recovery: Option<OsString>,
}

/// Schema description of Serde's existing OsStr/OsString representation.
/// Production serialization uses `OsString` itself, without a conversion policy.
#[derive(JsonSchema)]
pub enum NativeFilename {
    Unix(Vec<u8>),
    Windows(Vec<u16>),
}
