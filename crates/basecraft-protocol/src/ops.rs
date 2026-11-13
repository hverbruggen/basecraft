//! Operations and the operation log.
//!
//! Every change to project state is an operation. The log is JSON Lines,
//! one [`LogEntry`] per line, append-only, one stream per machine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CladeAnnotation, CladeRef, Feature, SupportKind, TreeStyle};

/// Log format version, written into every entry.
pub const LOG_VERSION: u32 = 1;

/// A change to a document. Coordinates are 0-based, half-open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", content = "params")]
pub enum Op {
    /// Open a file. Records its content hash, so replay can check it starts
    /// from the same input.
    #[serde(rename = "doc.open")]
    Open { path: String },
    /// Create an independent unsaved DNA sequence. The initial residues are
    /// retained so recovery and replay do not need an external source file.
    #[serde(rename = "doc.create_sequence")]
    CreateSequence { name: String, residues: String },
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
        /// The record, by name, in a file that holds several. Absent means
        /// the open record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
    },
    #[serde(rename = "feature.add")]
    FeatureAdd {
        feature: Feature,
        /// The record, by name, in a file that holds several. Absent means
        /// the open record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
    },
    #[serde(rename = "feature.remove")]
    FeatureRemove {
        id: u32,
        /// The record, by name, in a file that holds several. Absent means
        /// the open record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
    },
    #[serde(rename = "feature.update")]
    FeatureUpdate {
        id: u32,
        feature: Feature,
        /// The record, by name, in a file that holds several. Absent means
        /// the open record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
    },
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
        /// The record, by name, in a file that holds several. Absent means
        /// the open record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
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
    /// Append sequences, optionally replacing reference rows with their
    /// gap-expanded forms from an alignment addition tool.
    #[serde(rename = "aln.add_sequences")]
    AlnAddSequences {
        rows: Vec<(String, String)>,
        reference: Option<Vec<String>>,
    },
    // Tree edits. Clades are named by one or two tips: see `CladeRef`.
    /// Root the tree halfway along the branch above `clade`.
    #[serde(rename = "tree.reroot")]
    TreeReroot { clade: CladeRef },
    /// Root halfway along a longest tip-to-tip branch-length path.
    #[serde(rename = "tree.midpoint_root")]
    TreeMidpointRoot {},
    /// Globally minimize unweighted root-to-all-tip distance variance.
    #[serde(rename = "tree.minimum_variance_root")]
    TreeMinimumVarianceRoot {},
    /// Move the root along its branch: `clade`, one of the root's two
    /// children, gets `fraction` of the branch's length, the other the
    /// rest.
    #[serde(rename = "tree.root_position")]
    TreeRootPosition { clade: CladeRef, fraction: f64 },
    /// Reverse the order of the clade's children.
    #[serde(rename = "tree.rotate")]
    TreeRotate { clade: CladeRef },
    /// Order every node's children by size: smallest first (on top), or
    /// largest first.
    #[serde(rename = "tree.ladderize")]
    TreeLadderize { small_first: bool },
    #[serde(rename = "tree.rename_tip")]
    TreeRenameTip { from: String, to: String },
    /// State what the tree's branch lengths measure.
    #[serde(rename = "tree.set_branch_lengths")]
    TreeSetBranchLengths {
        meaning: crate::facts::BranchLengths,
    },
    /// State the letters an alignment or sequence is written in. A file has
    /// one companion, so in a multi-record FASTA or GenBank file this states
    /// the alphabet of every record in the file, not only the open one.
    #[serde(rename = "doc.set_alphabet")]
    SetAlphabet {
        alphabet: crate::facts::StatedAlphabet,
    },
    /// Add, change (same `id`) or, with no annotation, remove a clade
    /// annotation.
    #[serde(rename = "tree.annotate")]
    TreeAnnotate {
        id: u32,
        annotation: Option<CladeAnnotation>,
    },
    /// Settle labels a reroot left on another group of tips (D11): each goes
    /// where `choice` says. One undo step; refused as a whole when one label
    /// can't be placed. Given without `labels`, the session fills in every
    /// label displaced now, so the log always names them.
    #[serde(rename = "tree.place_label")]
    TreePlaceLabel {
        #[serde(default)]
        labels: Vec<u32>,
        choice: crate::tree::LabelPlacement,
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
        #[serde(default)]
        kind: SupportKind,
    },
    #[serde(rename = "tree.remove_support")]
    TreeRemoveSupport { name: String },
    #[serde(rename = "tree.style")]
    TreeStyle {
        style: TreeStyle,
        /// Support sets removed on accepting Style; cancellation makes no edits.
        remove_supports: Vec<String>,
    },
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
        /// The tool's own parameters. Basecraft's bookkeeping is in the typed
        /// fields below, never in parameter names.
        #[serde(default)]
        params: serde_json::Value,
        inputs: Vec<String>,
        outputs: Vec<String>,
        /// The exact input this activity read, retained in its own run.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_run: Option<crate::records::ActivityInputRun>,
        /// The document edits this activity made, such as imported values.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contributions: Option<crate::records::ActivityContributions>,
    },
    /// An atomic species-hypothesis edit against a workspace.
    #[serde(rename = "delimitation.edit")]
    DelimitationEdit {
        edit: Box<crate::delimitation::DelimitationEdit>,
    },
    /// An atomic edit to the project's specimen and taxon metadata.
    #[serde(rename = "metadata.edit")]
    MetadataEdit {
        edit: Box<crate::metadata::MetadataEdit>,
    },
    /// An atomic edit to a GenBank marker submission document.
    #[serde(rename = "submission.edit")]
    SubmissionEdit {
        edit: Box<crate::submission::SubmissionEdit>,
    },
    /// Initial review referencing an immutable record of drafts and biological events.
    #[serde(rename = "doc.create_sanger_review")]
    CreateSangerReview {
        review: crate::sanger_records::ReviewRecord,
    },
    /// An explicit analysis continuation. Selection and decisions have their own operations.
    #[serde(rename = "sanger.update")]
    SangerUpdate {
        review: crate::sanger_records::ReviewRecord,
    },
    /// Select a retained call set without copying its evidence.
    #[serde(rename = "sanger.select")]
    SangerSelect { call_set: String },
    /// A decision addressed to an immutable Sanger call-set digest.
    #[serde(rename = "sanger.review")]
    SangerReview {
        call_set: String,
        edit: Box<crate::sanger::ReviewOp>,
    },
    /// A terminal job record, including failures and cancellation.
    #[serde(rename = "job.finished")]
    JobFinished { report: crate::records::JobRecord },
    /// Undo the operation with log id `of`.
    #[serde(rename = "history.undo")]
    Undo { of: String },
    /// Redo the operation with log id `of`.
    #[serde(rename = "history.redo")]
    Redo { of: String },
}

impl Op {
    /// Replace residues `start..end` of the open record.
    pub fn seq_replace(start: u64, end: u64, residues: impl Into<String>) -> Self {
        Op::SeqReplace {
            start,
            end,
            residues: residues.into(),
            record: None,
        }
    }

    /// Add an annotation to the open record.
    pub fn feature_add(feature: Feature) -> Self {
        Op::FeatureAdd {
            feature,
            record: None,
        }
    }

    /// Remove annotation `id` from the open record.
    pub fn feature_remove(id: u32) -> Self {
        Op::FeatureRemove { id, record: None }
    }

    /// Replace annotation `id` of the open record.
    pub fn feature_update(id: u32, feature: Feature) -> Self {
        Op::FeatureUpdate {
            id,
            feature,
            record: None,
        }
    }

    /// The record a sequence edit or extraction names. Records in a file
    /// that holds several are known by name (LOCUS name, or the FASTA
    /// header up to the first space); absent means the open record.
    /// `None` for operations that don't name a record.
    pub fn record(&mut self) -> Option<&mut Option<String>> {
        match self {
            Op::SeqReplace { record, .. }
            | Op::FeatureAdd { record, .. }
            | Op::FeatureRemove { record, .. }
            | Op::FeatureUpdate { record, .. }
            | Op::Extract { record, .. } => Some(record),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Op::Open { .. } => "doc.open",
            Op::CreateSequence { .. } => "doc.create_sequence",
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
            Op::AlnAddSequences { .. } => "aln.add_sequences",
            Op::TreeReroot { .. } => "tree.reroot",
            Op::TreeMidpointRoot {} => "tree.midpoint_root",
            Op::TreeMinimumVarianceRoot {} => "tree.minimum_variance_root",
            Op::TreeRootPosition { .. } => "tree.root_position",
            Op::TreeRotate { .. } => "tree.rotate",
            Op::TreeLadderize { .. } => "tree.ladderize",
            Op::TreeRenameTip { .. } => "tree.rename_tip",
            Op::TreeSetBranchLengths { .. } => "tree.set_branch_lengths",
            Op::SetAlphabet { .. } => "doc.set_alphabet",
            Op::TreeAnnotate { .. } => "tree.annotate",
            Op::TreePlaceLabel { .. } => "tree.place_label",
            Op::TreeAddSupport { .. } => "tree.add_support",
            Op::TreeRemoveSupport { .. } => "tree.remove_support",
            Op::TreeStyle { .. } => "tree.style",
            Op::DelimitationEdit { .. } => "delimitation.edit",
            Op::MetadataEdit { .. } => "metadata.edit",
            Op::SubmissionEdit { .. } => "submission.edit",
            Op::SangerReview { .. } => "sanger.review",
            Op::CreateSangerReview { .. } => "doc.create_sanger_review",
            Op::SangerUpdate { .. } => "sanger.update",
            Op::SangerSelect { .. } => "sanger.select",
            Op::JobFinished { .. } => "job.finished",
            Op::Undo { .. } => "history.undo",
            Op::Redo { .. } => "history.redo",
        }
    }
}

/// Who made a change: a person in the GUI (`human`), a script through the
/// CLI or Python API (`script`), an assistant over MCP (`agent`), or a tool,
/// plugin or outside program. New kinds are additive; older records keep
/// their kinds.
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
    /// An assistant over MCP, named by its client's model ID.
    Agent {
        model: String,
    },
    /// A script driving Basecraft through the CLI or the Python API, named by
    /// its client (for example `CLI` or `Python`).
    Script {
        client: String,
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
    /// Named counts and document kind. Absent in logs written before these
    /// descriptors were introduced; the original fields retain their meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<DocumentCounts>,
}

/// A document fingerprint names what it counts instead of interpreting a
/// metadata link as an alignment row or a submission frame as a feature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentCounts {
    SangerReview {
        columns: u64,
        unresolved: u64,
        call_sets: u64,
    },
    Sequence {
        residues: u64,
        features: u64,
        /// Records in the file, when it holds more than the one shown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        records: Option<u64>,
    },
    Alignment {
        columns: u64,
        rows: u64,
    },
    Tree {
        tips: u64,
        annotations: u64,
    },
    Delimitation {
        specimens: u64,
        hypotheses: u64,
    },
    Metadata {
        specimens: u64,
        identifications: u64,
        links: u64,
    },
    Submission {
        records: u64,
        frames: u64,
        accessions: u64,
    },
}

impl DocState {
    /// Construct a complete document fingerprint.
    pub fn from_counts(counts: DocumentCounts) -> Self {
        let (len, features, rows) = match &counts {
            DocumentCounts::SangerReview {
                columns,
                unresolved,
                call_sets,
            } => (*columns, *unresolved, Some(*call_sets)),
            DocumentCounts::Sequence {
                residues, features, ..
            } => (*residues, *features, None),
            DocumentCounts::Alignment { columns, rows } => (*columns, 0, Some(*rows)),
            DocumentCounts::Tree { tips, annotations } => (*tips, *annotations, None),
            DocumentCounts::Delimitation {
                specimens,
                hypotheses,
            } => (*specimens, *hypotheses, None),
            DocumentCounts::Metadata {
                specimens,
                identifications,
                links,
            } => (*specimens, *identifications, Some(*links)),
            DocumentCounts::Submission {
                records,
                frames,
                accessions,
            } => (*records, *frames, Some(*accessions)),
        };
        Self {
            len,
            features,
            rows,
            counts: Some(counts),
        }
    }

    /// Replay requires all counts and the document kind to agree.
    pub fn matches_recorded(&self, recorded: &Self) -> bool {
        self == recorded
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpResult {
    /// Content hashes of files read or written, by project-relative path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashes: Vec<(String, String)>,
    /// Id assigned to a new feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<u32>,
    /// What the edit did that the user should hear about, shown in History.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Original files described by an open journal record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRecord {
    pub checkpoint: bool,
    pub feature_slots: Vec<bool>,
    pub node_ids: Vec<usize>,
    pub root: std::path::PathBuf,
    pub files: Vec<crate::provenance::InputReference>,
}

/// One line of the operation log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRecord>,
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
    fn named_fingerprints_round_trip_and_detect_changes() {
        let fixtures =
            include_str!("../../../testdata/fixtures/records-v1/document-fingerprints.jsonl");
        for line in fixtures.lines() {
            let current: DocState = serde_json::from_str(line).unwrap();
            assert_eq!(serde_json::to_string(&current).unwrap(), line);
            assert_eq!(
                DocState::from_counts(current.counts.clone().unwrap()),
                current
            );
            let mut wrong = current.clone();
            wrong.len += 1;
            assert!(!current.matches_recorded(&wrong));
            wrong = current.clone();
            wrong.counts = None;
            assert!(!current.matches_recorded(&wrong));
        }
        let sequence = DocState::from_counts(DocumentCounts::Sequence {
            residues: 10,
            features: 2,
            records: None,
        });
        let tree = DocState::from_counts(DocumentCounts::Tree {
            tips: 10,
            annotations: 2,
        });
        assert_eq!(
            (sequence.len, sequence.features, sequence.rows),
            (tree.len, tree.features, tree.rows)
        );
        assert!(!sequence.matches_recorded(&tree));
        assert!(!tree.matches_recorded(&sequence));
    }

    #[test]
    fn entry_json_shape_and_roundtrip() {
        let e = LogEntry {
            source: None,
            v: LOG_VERSION,
            id: "01JB9Q".into(),
            t: "2026-10-10T09:14:03.512Z".into(),
            host: "imachome".into(),
            session: "01JB9P".into(),
            actor: Actor::Human { user: "hv".into() },
            doc: "rbcL/rbcL.gb".into(),
            op: Op::seq_replace(4, 6, "ACG"),
            result: OpResult::default(),
            state: DocState {
                counts: None,
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
    fn actor_kinds_keep_their_wire_shape() {
        let script = Actor::Script {
            client: "Python".into(),
        };
        let json = serde_json::to_string(&script).unwrap();
        assert_eq!(json, r#"{"kind":"script","client":"Python"}"#);
        assert_eq!(serde_json::from_str::<Actor>(&json).unwrap(), script);
        // Records written before the script kind still read unchanged.
        assert_eq!(
            serde_json::from_str::<Actor>(r#"{"kind":"agent","model":"mcp-client"}"#).unwrap(),
            Actor::Agent {
                model: "mcp-client".into()
            }
        );
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
