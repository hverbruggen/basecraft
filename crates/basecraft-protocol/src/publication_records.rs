//! Schema-1 publication writer shapes, not retry/read acceptance policies.
//!
//! Serialize through `serde_json::Value`, then the existing pretty JSON writer
//! with one final LF. Paths remain native-spelled JSON strings. These known
//! shapes do not define an extension-preserving rewrite interface: collection
//! retry checks presence only; member retry selects ownership fields separately.
use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Expected members recorded before ordinary collection retention.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectionIntent {
    pub schema: u32,
    pub members: BTreeMap<PathBuf, String>,
}

/// A collection's project destination and members, recorded before the
/// collection's folder moves there (D26).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectionExportIntent {
    pub schema: u32,
    pub destination: PathBuf,
    pub members: BTreeMap<PathBuf, String>,
    /// The identity of the folder that becomes `destination`, as the
    /// operating system reports it (`platform` plus its fields), so a repair
    /// adopts only the folder this run moved.
    #[schemars(with = "serde_json::Value")]
    pub identity: serde_json::Value,
}

/// Identity of the synced pending file, recorded before no-replace promotion.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MemberExportReceipt {
    pub schema: u32,
    pub destination: PathBuf,
    pub hash: String,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}
