//! Operations and the operation log.
//!
//! Every change to project state is an operation. The log is JSON Lines,
//! one [`LogEntry`] per line, append-only, one stream per machine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Feature;

/// Log format version, written into every entry.
pub const LOG_VERSION: u32 = 1;

/// A change to a document. Coordinates are 0-based, half-open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", content = "params")]
pub enum Op {
    /// Open a file. Records its content hash, so replay can check it starts
    /// from the same input.
    #[serde(rename = "doc.open")]
    Open { path: String },
    /// Save a document. Records the hash of each file written.
    #[serde(rename = "doc.save")]
    Save { path: String },
    /// Replace residues `start..end` with `residues`. Insertions and
    /// deletions are replacements with an empty side.
    #[serde(rename = "seq.replace")]
    SeqReplace {
        start: u64,
        end: u64,
        residues: String,
    },
    #[serde(rename = "feature.add")]
    FeatureAdd { feature: Feature },
    #[serde(rename = "feature.remove")]
    FeatureRemove { id: u32 },
    #[serde(rename = "feature.update")]
    FeatureUpdate { id: u32, feature: Feature },
    /// Undo the operation with log id `of`.
    #[serde(rename = "history.undo")]
    Undo { of: String },
    /// Redo the operation with log id `of`.
    #[serde(rename = "history.redo")]
    Redo { of: String },
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::Open { .. } => "doc.open",
            Op::Save { .. } => "doc.save",
            Op::SeqReplace { .. } => "seq.replace",
            Op::FeatureAdd { .. } => "feature.add",
            Op::FeatureRemove { .. } => "feature.remove",
            Op::FeatureUpdate { .. } => "feature.update",
            Op::Undo { .. } => "history.undo",
            Op::Redo { .. } => "history.redo",
        }
    }
}

/// Who made a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Actor {
    Human {
        user: String,
    },
    Tool {
        tool: String,
        version: String,
    },
    Plugin {
        id: String,
        version: String,
    },
    Agent {
        model: String,
    },
    /// A change Basecraft detected on disk but didn't make.
    External,
}

/// A cheap fingerprint of a document after an operation. Replay compares
/// it at every step; full content hashes are compared at open and save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocState {
    pub len: u64,
    pub features: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpResult {
    /// Content hashes of files read or written, by project-relative path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashes: Vec<(String, String)>,
    /// Id assigned to a new feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<u32>,
}

/// One line of the operation log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LogEntry {
    pub v: u32,
    /// ULID: unique, and sorts by time.
    pub id: String,
    /// RFC 3339 UTC.
    pub t: String,
    pub host: String,
    pub session: String,
    pub actor: Actor,
    /// Project-relative path of the document the op applies to.
    pub doc: String,
    #[serde(flatten)]
    pub op: Op,
    #[serde(default, skip_serializing_if = "is_default")]
    pub result: OpResult,
    pub state: DocState,
}

fn is_default(r: &OpResult) -> bool {
    *r == OpResult::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_json_shape_and_roundtrip() {
        let e = LogEntry {
            v: LOG_VERSION,
            id: "01JB9Q".into(),
            t: "2026-10-10T09:14:03.512Z".into(),
            host: "imachome".into(),
            session: "01JB9P".into(),
            actor: Actor::Human { user: "hv".into() },
            doc: "rbcL/rbcL.gb".into(),
            op: Op::SeqReplace {
                start: 4,
                end: 6,
                residues: "ACG".into(),
            },
            result: OpResult::default(),
            state: DocState {
                len: 101,
                features: 3,
            },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""op":"seq.replace","params":{"start":4,"end":6,"residues":"ACG"}"#),
            "{json}"
        );
        assert!(
            json.contains(r#""actor":{"kind":"human","user":"hv"}"#),
            "{json}"
        );
        assert!(!json.contains("result"));
        assert_eq!(serde_json::from_str::<LogEntry>(&json).unwrap(), e);
    }
}
