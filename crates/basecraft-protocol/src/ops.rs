//! Operations and the operation log.
//!
//! Every change to project state is an operation. The log is JSON Lines,
//! one [`LogEntry`] per line, append-only, one stream per machine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CladeAnnotation, CladeRef, Feature, TreeStyle};

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
    /// Write residues `start..end` and the annotations within them to a new
    /// file at `path`. Annotations cut by the range are clipped and marked
    /// incomplete at the cut. Records the new file's hash.
    #[serde(rename = "doc.extract")]
    Extract {
        start: u64,
        end: u64,
        path: String,
        /// Parts to splice together, 5' to 3', instead of `start..end`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segments: Vec<(u64, u64)>,
        /// Write the reverse complement, so a reverse-strand annotation
        /// reads 5' to 3' in the new file.
        #[serde(default, skip_serializing_if = "is_false")]
        reverse: bool,
    },
    // Alignment edits. Columns are 0-based, half-open; `rows` are half-open
    // row ranges, `[[first, past_last], ...]`. Every edit keeps the
    // alignment rectangular: rows that end up shorter than the widest get
    // trailing gaps.
    /// Replace columns `start..end` of one row with `residues`.
    #[serde(rename = "aln.replace")]
    AlnReplace {
        row: u32,
        start: u64,
        end: u64,
        residues: String,
    },
    /// Insert `residues` before column `col` in `rows`. Like gaps, they use
    /// up each row's trailing gaps first, unless every row gets them.
    #[serde(rename = "aln.insert")]
    AlnInsert {
        rows: Vec<(u32, u32)>,
        col: u64,
        residues: String,
    },
    /// Type one residue, or a gap, over the block `rows` × `start..end`.
    #[serde(rename = "aln.fill")]
    AlnFill {
        rows: Vec<(u32, u32)>,
        start: u64,
        end: u64,
        residue: String,
    },
    /// Insert `count` gaps before column `col`. Rows use up their own
    /// trailing gaps first, unless every row gets gaps: that's a new column.
    #[serde(rename = "aln.insert_gaps")]
    AlnInsertGaps {
        rows: Vec<(u32, u32)>,
        col: u64,
        count: u64,
    },
    /// Delete columns `start..end` of `rows`; what follows moves left.
    #[serde(rename = "aln.delete")]
    AlnDelete {
        rows: Vec<(u32, u32)>,
        start: u64,
        end: u64,
    },
    /// Slide the block `rows` × `start..end` by `by` columns into the gaps
    /// beside it.
    #[serde(rename = "aln.shift")]
    AlnShift {
        rows: Vec<(u32, u32)>,
        start: u64,
        end: u64,
        by: i64,
    },
    /// Delete every column that is a gap in all rows.
    #[serde(rename = "aln.remove_gap_columns")]
    AlnRemoveGapColumns {},
    /// Move `rows`, in order, so the first lands at index `to` among the
    /// rows that remain.
    #[serde(rename = "aln.move_rows")]
    AlnMoveRows { rows: Vec<(u32, u32)>, to: u32 },
    #[serde(rename = "aln.rename_row")]
    AlnRenameRow { row: u32, name: String },
    #[serde(rename = "aln.remove_rows")]
    AlnRemoveRows { rows: Vec<(u32, u32)> },
    // Tree edits. Clades are named by one or two tips: see `CladeRef`.
    /// Root the tree halfway along the branch above `clade`.
    #[serde(rename = "tree.reroot")]
    TreeReroot { clade: CladeRef },
    /// Reverse the order of the clade's children.
    #[serde(rename = "tree.rotate")]
    TreeRotate { clade: CladeRef },
    /// Order every node's children by size: smallest first (on top), or
    /// largest first.
    #[serde(rename = "tree.ladderize")]
    TreeLadderize { small_first: bool },
    #[serde(rename = "tree.rename_tip")]
    TreeRenameTip { from: String, to: String },
    /// Add, change (same `id`) or, with no annotation, remove a clade
    /// annotation.
    #[serde(rename = "tree.annotate")]
    TreeAnnotate {
        id: u32,
        annotation: Option<CladeAnnotation>,
    },
    /// A set of support values, such as bootstraps from another analysis:
    /// one per internal node in preorder, as text.
    #[serde(rename = "tree.add_support")]
    TreeAddSupport {
        name: String,
        /// Where the values came from, for the record.
        #[serde(default)]
        source: String,
        values: Vec<Option<String>>,
    },
    #[serde(rename = "tree.remove_support")]
    TreeRemoveSupport { name: String },
    #[serde(rename = "tree.style")]
    TreeStyle { style: TreeStyle },
    /// A figure or other derived file written from a document, such as a
    /// tree as SVG. The result holds its hash; replay doesn't rewrite it.
    #[serde(rename = "doc.export")]
    Export { path: String, format: String },
    /// A plugin operation that reads and writes files, such as an aligner.
    /// `inputs` and `outputs` are project-relative paths; the result holds
    /// their content hashes. The actor names the tool and its version.
    #[serde(rename = "tool.run")]
    ToolRun {
        plugin: String,
        operation: String,
        #[serde(default)]
        params: serde_json::Value,
        inputs: Vec<String>,
        outputs: Vec<String>,
    },
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
            Op::Extract { .. } => "doc.extract",
            Op::Export { .. } => "doc.export",
            Op::SeqReplace { .. } => "seq.replace",
            Op::FeatureAdd { .. } => "feature.add",
            Op::FeatureRemove { .. } => "feature.remove",
            Op::FeatureUpdate { .. } => "feature.update",
            Op::AlnReplace { .. } => "aln.replace",
            Op::AlnInsert { .. } => "aln.insert",
            Op::AlnFill { .. } => "aln.fill",
            Op::ToolRun { .. } => "tool.run",
            Op::AlnInsertGaps { .. } => "aln.insert_gaps",
            Op::AlnDelete { .. } => "aln.delete",
            Op::AlnShift { .. } => "aln.shift",
            Op::AlnRemoveGapColumns {} => "aln.remove_gap_columns",
            Op::AlnMoveRows { .. } => "aln.move_rows",
            Op::AlnRenameRow { .. } => "aln.rename_row",
            Op::AlnRemoveRows { .. } => "aln.remove_rows",
            Op::TreeReroot { .. } => "tree.reroot",
            Op::TreeRotate { .. } => "tree.rotate",
            Op::TreeLadderize { .. } => "tree.ladderize",
            Op::TreeRenameTip { .. } => "tree.rename_tip",
            Op::TreeAnnotate { .. } => "tree.annotate",
            Op::TreeAddSupport { .. } => "tree.add_support",
            Op::TreeRemoveSupport { .. } => "tree.remove_support",
            Op::TreeStyle { .. } => "tree.style",
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
    /// Residues of a sequence, columns of an alignment, tips of a tree.
    pub len: u64,
    /// Annotations: features of a sequence, clade annotations of a tree.
    pub features: u64,
    /// Rows of an alignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
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
                rows: None,
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
        assert!(!json.contains("rows"));
        assert_eq!(serde_json::from_str::<LogEntry>(&json).unwrap(), e);
    }

    #[test]
    fn alignment_op_json_shape() {
        let op = Op::AlnShift {
            rows: vec![(12, 15)],
            start: 402,
            end: 418,
            by: -2,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(
            json,
            r#"{"op":"aln.shift","params":{"rows":[[12,15]],"start":402,"end":418,"by":-2}}"#
        );
        let gaps = serde_json::to_string(&Op::AlnRemoveGapColumns {}).unwrap();
        assert_eq!(gaps, r#"{"op":"aln.remove_gap_columns","params":{}}"#);
        assert_eq!(
            serde_json::from_str::<Op>(&gaps).unwrap(),
            Op::AlnRemoveGapColumns {}
        );
    }
}
