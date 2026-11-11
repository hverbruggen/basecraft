//! GenBank marker submission contracts. No implementation dependencies.
//!
//! A submission document names one source alignment by project path and
//! content hash, the decision for each of its rows, batch settings that
//! apply to every exported record, reviewed reading frames, export
//! settings, the publication (authors and title) and accessions returned
//! by the archive. Specimen data come from the project metadata at export
//! time; the submitter's contact details are an application preference.
use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The submission format version this crate reads and writes.
pub const SUBMISSION_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Submission {
    pub version: u32,
    #[serde(default)]
    pub destination: Destination,
    /// The alignment the records are extracted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceAlignment>,
    /// Explicit decisions by record name; absent rows use accession-aware defaults.
    #[serde(default)]
    pub rows: Vec<RowDecision>,
    /// Fingerprint of the reviewed source, effective decisions and accession evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_confirmation: Option<String>,
    /// Identity mappings from successfully written packages; independent of row selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exported: Vec<ExportMapping>,
    pub batch: BatchSettings,
    /// Reading frames the user confirmed or chose, by record name.
    #[serde(default)]
    pub frames: Vec<FrameChoice>,
    #[serde(default)]
    pub export: ExportSettings,
    #[serde(default)]
    pub publication: Publication,
    /// Absent in legacy documents: their publication is also the submission citation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_details: Option<Publication>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub publication_differs: bool,
    /// Marker-specific drafts survive type switches without transferring scientific text.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marker_drafts: BTreeMap<String, Marker>,
    /// Rule origin for explicit decisions; absent entries are manual/legacy decisions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_rules: BTreeMap<String, InclusionRule>,
    #[serde(default, skip_serializing_if = "AccessionPolicy::is_legacy")]
    pub accession_policy: AccessionPolicy,
    /// Accessions returned by the archive, by record name.
    #[serde(default)]
    pub accessions: Vec<AccessionEntry>,
    /// Fields written by a later Basecraft, kept unchanged on save.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Submission {
    fn default() -> Self {
        Self {
            version: SUBMISSION_VERSION,
            destination: Destination::Genbank,
            source: None,
            rows: Vec::new(),
            selection_confirmation: None,
            exported: Vec::new(),
            batch: BatchSettings::default(),
            frames: Vec::new(),
            export: ExportSettings::default(),
            publication: Publication::default(),
            submission_details: None,
            publication_differs: false,
            marker_drafts: BTreeMap::new(),
            row_rules: BTreeMap::new(),
            accession_policy: AccessionPolicy::default(),
            accessions: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Submission {
    pub fn submission_citation(&self) -> &Publication {
        self.submission_details
            .as_ref()
            .unwrap_or(&self.publication)
    }

    pub fn publication_citation(&self) -> &Publication {
        if self.publication_differs {
            &self.publication
        } else {
            self.submission_citation()
        }
    }
}

/// Retained identities for interpreting archive returns after later selection edits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExportMapping {
    pub source: Option<SourceAlignment>,
    /// Submitted `SeqID` to original biological record name.
    pub records: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    #[default]
    Genbank,
}

/// An alignment file inside the project and the BLAKE3 hash of the bytes
/// the submission was prepared from. A different file content is refused
/// until the source is selected again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceAlignment {
    /// Project-relative path.
    pub document: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RowDecision {
    /// The alignment row name (FASTA ID).
    pub record: String,
    #[serde(flatten)]
    pub decision: Decision,
}

/// What to do with a row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// A newly generated sequence, exported for submission.
    #[default]
    New,
    /// Already in an archive (a downloaded reference, or submitted
    /// before); not exported.
    Existing { accession: String },
    /// Left out of the submission.
    Exclude,
}

/// Settings shared by every record of the batch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BatchSettings {
    pub marker: Marker,
    #[serde(default)]
    pub mol_type: MolType,
    /// The organelle the marker is from; none for nuclear markers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organelle: Option<Organelle>,
    /// NCBI genetic code (translation table) number.
    pub genetic_code: u8,
    #[serde(default)]
    pub ends: EndPolicy,
}

impl Default for BatchSettings {
    fn default() -> Self {
        Self {
            marker: Marker::Rrna {
                product: String::new(),
            },
            mol_type: MolType::GenomicDna,
            organelle: None,
            genetic_code: 1,
            ends: EndPolicy::Auto,
        }
    }
}

/// The marker type, which decides the one feature each record gets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Marker {
    /// A protein-coding gene: a `gene` and a `CDS` feature.
    Coding {
        /// Gene symbol, such as `rbcL`; may be empty.
        #[serde(default)]
        gene: String,
        product: String,
    },
    /// A ribosomal RNA gene, such as `18S ribosomal RNA`.
    Rrna { product: String },
    /// The ITS region: one `misc_RNA` with a note listing the components
    /// present. Internal boundaries are not annotated.
    Its { components: Vec<ItsComponent> },
    /// A non-coding region, such as an intergenic spacer: one
    /// `misc_feature` with this description as its note.
    Spacer { description: String },
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ItsComponent {
    /// Small subunit (18S) ribosomal RNA.
    #[serde(rename = "SSU")]
    SmallSubunit,
    #[serde(rename = "ITS1")]
    Its1,
    #[serde(rename = "5.8S")]
    Rrna58S,
    #[serde(rename = "ITS2")]
    Its2,
    /// Large subunit (28S) ribosomal RNA.
    #[serde(rename = "LSU")]
    LargeSubunit,
}

impl ItsComponent {
    pub const ALL: [Self; 5] = [
        Self::SmallSubunit,
        Self::Its1,
        Self::Rrna58S,
        Self::Its2,
        Self::LargeSubunit,
    ];

    /// The component as current GenBank ITS records name it in a note.
    pub fn description(self) -> &'static str {
        match self {
            Self::SmallSubunit => "small subunit ribosomal RNA",
            Self::Its1 => "internal transcribed spacer 1",
            Self::Rrna58S => "5.8S ribosomal RNA",
            Self::Its2 => "internal transcribed spacer 2",
            Self::LargeSubunit => "large subunit ribosomal RNA",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MolType {
    #[default]
    GenomicDna,
    GenomicRna,
}

impl MolType {
    /// The INSDC `mol_type` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GenomicDna => "genomic DNA",
            Self::GenomicRna => "genomic RNA",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Organelle {
    Mitochondrion,
    Chloroplast,
    Plastid,
    Apicoplast,
    Chromoplast,
    Cyanelle,
    Kinetoplast,
    Nucleomorph,
}

impl Organelle {
    /// The NCBI `location` source modifier value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mitochondrion => "mitochondrion",
            Self::Chloroplast => "chloroplast",
            Self::Plastid => "plastid",
            Self::Apicoplast => "apicoplast",
            Self::Chromoplast => "chromoplast",
            Self::Cyanelle => "cyanelle",
            Self::Kinetoplast => "kinetoplast",
            Self::Nucleomorph => "nucleomorph",
        }
    }

    /// Whether the organelle's genes use the mitochondrial genetic code
    /// modifier (`mgcode`) rather than `gcode`.
    pub fn mitochondrial(self) -> bool {
        matches!(self, Self::Mitochondrion | Self::Kinetoplast)
    }
}

/// How the feature's ends are marked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndPolicy {
    /// Coding markers: an end is complete when it has a start (5') or stop
    /// (3') codon in frame. Other markers: both ends partial.
    #[default]
    Auto,
    /// Both ends partial.
    Partial,
    /// Both ends complete.
    Complete,
}

/// A reading frame the user reviewed for one record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrameChoice {
    pub record: String,
    /// 1, 2 or 3: the first base of the first complete codon.
    pub codon_start: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ExportSettings {
    /// Remove runs of unknown bases (N, ?) at the ends of each record.
    /// The trimmed lengths are recorded in the coordinate map.
    pub trim_terminal_unknown: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            trim_terminal_unknown: true,
        }
    }
}

/// The publication the sequences belong to, written to the `.sbt`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Publication {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub authors: Vec<Author>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Author {
    pub first: String,
    pub last: String,
    /// Middle names or initials, without the first name's, such as `Marie` or `M.R.`.
    /// Bare uppercase runs (`MR`) retain their legacy initials meaning; uppercase full names are ambiguous.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub middle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccessionEntry {
    pub record: String,
    /// Accession, with its version when the archive gave one.
    pub accession: String,
}

/// One atomic submission edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SubmissionEdit {
    /// Select the source alignment (or accept a changed one).
    SetSource {
        source: SourceAlignment,
    },
    /// Set the decision of these rows.
    SetRows {
        rows: Vec<RowDecision>,
    },
    /// Confirm the selection fingerprint returned by submission review.
    ConfirmSelection {
        selection_hash: String,
    },
    SetBatch {
        batch: BatchSettings,
    },
    /// Save annotation settings with type-specific drafts. Legacy `set_batch`
    /// retains its historical meaning so previously saved histories replay exactly.
    SetBatchDraft {
        batch: BatchSettings,
    },
    /// Confirm or choose reading frames; `clear` forgets choices.
    SetFrames {
        #[serde(default)]
        frames: Vec<FrameChoice>,
        #[serde(default)]
        clear: Vec<String>,
    },
    SetExport {
        export: ExportSettings,
    },
    SetPublication {
        publication: Publication,
    },
    SetAccessionPolicy {
        policy: AccessionPolicy,
    },
    /// Both citations in one undoable change. Disabled custom publication values are retained.
    SetDetails {
        submission: Publication,
        publication: Publication,
        publication_differs: bool,
    },
    /// Apply a reviewed rule. The shared ops boundary checks this plan against live inputs.
    ApplyInclusion {
        plan: Box<InclusionPlan>,
    },
    /// Retain the mapping after writing a submission package.
    RecordExport {
        mapping: ExportMapping,
    },
    /// Record accessions returned by the archive.
    SetAccessions {
        accessions: Vec<AccessionEntry>,
    },
    /// A source row was renamed: its decision, frame, rule and accession follow it.
    RenameRecord {
        from: String,
        to: String,
    },
}

/// Legacy suggestions are retained on old documents. Configured sources prevent
/// specimen identifiers elsewhere in a name from influencing inclusion suggestions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum AccessionPolicy {
    #[default]
    LegacyHeuristic,
    Configured {
        input: Option<IdentifierSource>,
        include_metadata: bool,
    },
}

impl AccessionPolicy {
    fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacyHeuristic)
    }
}

/// Identifier fields use the same literal splitting primitive as metadata header imports.
/// Submission rules always use the FASTA identifier, never the full description header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum IdentifierSource {
    Identifier,
    Field { separator: String, index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Exact,
    Contains,
    Starts,
    Ends,
    Wildcard,
    Regex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InclusionAction {
    Include,
    Exclude,
    #[default]
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InclusionRule {
    pub name: String,
    pub input: IdentifierSource,
    pub mode: MatchMode,
    pub pattern: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub trim: bool,
    pub matching: InclusionAction,
    pub nonmatching: InclusionAction,
    #[serde(default)]
    pub unresolved: InclusionAction,
    #[serde(default)]
    pub override_manual: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InclusionPreviewRow {
    pub record: String,
    pub extracted: Option<String>,
    pub matched: Option<bool>,
    pub reason: String,
    pub preserved_manual: bool,
    pub current: Decision,
    pub proposed: Decision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InclusionPlan {
    pub rule: InclusionRule,
    /// Hash of target content, source bytes and metadata evidence; recomputed before apply.
    pub fingerprint: String,
    pub rows: Vec<InclusionPreviewRow>,
    pub matching: usize,
    pub unresolved: usize,
    pub preserved: usize,
    pub changed: usize,
    pub included: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_document_json_shapes() {
        let edit = SubmissionEdit::SetRows {
            rows: vec![
                RowDecision {
                    record: "HV1".into(),
                    decision: Decision::Existing {
                        accession: "MZ123456.1".into(),
                    },
                },
                RowDecision {
                    record: "HV2".into(),
                    decision: Decision::Exclude,
                },
            ],
        };
        let json = serde_json::to_string(&edit).unwrap();
        assert_eq!(
            json,
            r#"{"action":"set_rows","rows":[{"record":"HV1","decision":"existing","accession":"MZ123456.1"},{"record":"HV2","decision":"exclude"}]}"#
        );
        assert_eq!(serde_json::from_str::<SubmissionEdit>(&json).unwrap(), edit);
        let batch = BatchSettings {
            marker: Marker::Its {
                components: vec![ItsComponent::Its1, ItsComponent::Rrna58S],
            },
            ..BatchSettings::default()
        };
        let json = serde_json::to_string(&batch).unwrap();
        assert!(
            json.contains(r#""marker":{"kind":"its","components":["ITS1","5.8S"]}"#),
            "{json}"
        );
    }

    #[test]
    fn unknown_fields_survive() {
        let json = r#"{"version":1,"batch":{"marker":{"kind":"rrna","product":"18S ribosomal RNA"},"genetic_code":1},"later":[1]}"#;
        let sub: Submission = serde_json::from_str(json).unwrap();
        assert!(sub.export.trim_terminal_unknown);
        assert_eq!(sub.extra["later"], serde_json::json!([1]));
        assert_eq!(
            serde_json::to_value(&sub).unwrap()["later"],
            serde_json::json!([1])
        );
    }
}
