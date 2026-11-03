//! Project specimen and taxon metadata contracts. No implementation dependencies.
//!
//! A project has one specimen roster with stable local IDs, typed metadata
//! columns, taxa, identifications (recorded specimen-to-taxon assignments)
//! and stored links from sequence records to specimens. Collection columns
//! use Darwin Core term names; INSDC source qualifiers are derived from them
//! on export. The same [`Table`], [`Column`] and [`Cell`] types are meant for
//! later trait tables.
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The metadata format version this crate reads and writes.
pub const METADATA_VERSION: u32 = 1;

/// The project's specimen metadata, as held in memory and edited by
/// [`MetadataEdit`]s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectMetadata {
    pub version: u32,
    /// The specimen roster: one row per specimen, keyed by its local ID.
    pub specimens: Table,
    #[serde(default)]
    pub taxa: Vec<Taxon>,
    /// In the order they were recorded. A specimen's current identification
    /// is its last one; earlier ones remain as its identification history.
    #[serde(default)]
    pub identifications: Vec<Identification>,
    #[serde(default)]
    pub links: Vec<RecordLink>,
    #[serde(default)]
    pub import: ImportSettings,
    /// Saved project partitions; members are stable roster IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partitions: Vec<crate::delimitation::Hypothesis>,
    /// Fields written by a later Basecraft, kept unchanged on save.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            version: METADATA_VERSION,
            specimens: Table::default(),
            taxa: Vec::new(),
            identifications: Vec::new(),
            links: Vec::new(),
            import: ImportSettings::default(),
            partitions: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// A typed table: declared columns and rows keyed by a stable ID.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Column {
    /// A Darwin Core term (`catalogNumber`, `eventDate`, ...) or a user name:
    /// an ASCII letter followed by letters, digits or underscores.
    pub name: String,
    pub kind: ColumnKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// What a column holds. Every kind also accepts an INSDC missing-value reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColumnKind {
    Text,
    /// A calendar date at its true precision: year, year and month, or day.
    Date,
    /// WGS84 decimal degrees, -90 to 90.
    Latitude,
    /// WGS84 decimal degrees, -180 to 180.
    Longitude,
    /// A number, in `unit` when given.
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    /// One of `levels`.
    Categorical {
        levels: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    pub id: String,
    /// Column name to value; absent columns have no value.
    #[serde(default)]
    pub cells: BTreeMap<String, Cell>,
}

/// One value. The variant must match its column's kind; any column may hold
/// a missing-value reason instead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cell {
    Text(String),
    Date(PartialDate),
    /// Latitude or longitude in decimal degrees.
    Degrees(f64),
    Number(f64),
    Category(String),
    Missing(MissingReason),
}

/// A date at year, month or day precision, written `YYYY`, `YYYY-MM` or
/// `YYYY-MM-DD`. A missing month or day is unknown, never the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartialDate {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl PartialDate {
    /// Parse `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, checking the calendar.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("{text:?} is not a date written YYYY, YYYY-MM or YYYY-MM-DD");
        let parts: Vec<&str> = text.split('-').collect();
        let number = |part: &str, width: usize| {
            if part.len() == width && part.bytes().all(|b| b.is_ascii_digit()) {
                part.parse::<u16>().ok()
            } else {
                None
            }
        };
        let (year, month, day) = match parts.as_slice() {
            [y] => (number(y, 4).ok_or_else(bad)?, None, None),
            [y, m] => (
                number(y, 4).ok_or_else(bad)?,
                Some(number(m, 2).ok_or_else(bad)?),
                None,
            ),
            [y, m, d] => (
                number(y, 4).ok_or_else(bad)?,
                Some(number(m, 2).ok_or_else(bad)?),
                Some(number(d, 2).ok_or_else(bad)?),
            ),
            _ => return Err(bad()),
        };
        if year == 0 {
            return Err(bad());
        }
        let month = month.map(u8::try_from).transpose().map_err(|_| bad())?;
        let day = day.map(u8::try_from).transpose().map_err(|_| bad())?;
        if let Some(m) = month {
            if !(1..=12).contains(&m) {
                return Err(format!("{text:?} has no month {m}"));
            }
            if let Some(d) = day {
                let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                let last = match m {
                    2 if leap => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                if d == 0 || d > last {
                    return Err(format!("{text:?} has no day {d}"));
                }
            }
        }
        Ok(Self { year, month, day })
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(m) = self.month {
            write!(f, "-{m:02}")?;
            if let Some(d) = self.day {
                write!(f, "-{d:02}")?;
            }
        }
        Ok(())
    }
}

impl Serialize for PartialDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PartialDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for PartialDate {
    fn schema_name() -> Cow<'static, str> {
        "PartialDate".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$",
            "description": "A date at year, month or day precision"
        })
    }
}

/// Why a value is missing: the INSDC missing-value vocabulary, written as
/// INSDC writes it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum MissingReason {
    #[serde(rename = "not applicable")]
    NotApplicable,
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "not collected")]
    NotCollected,
    #[serde(rename = "not provided")]
    NotProvided,
    #[serde(rename = "restricted access")]
    RestrictedAccess,
    #[serde(rename = "missing: control sample")]
    ControlSample,
    #[serde(rename = "missing: sample group")]
    SampleGroup,
    #[serde(rename = "missing: synthetic construct")]
    SyntheticConstruct,
    #[serde(rename = "missing: lab stock")]
    LabStock,
    #[serde(rename = "missing: third party data")]
    ThirdPartyData,
    #[serde(rename = "missing: data agreement established pre-2023")]
    DataAgreementPre2023,
    #[serde(rename = "missing: endangered species")]
    EndangeredSpecies,
    #[serde(rename = "missing: human-identifiable")]
    HumanIdentifiable,
}

impl MissingReason {
    pub const ALL: [Self; 13] = [
        Self::NotApplicable,
        Self::Missing,
        Self::NotCollected,
        Self::NotProvided,
        Self::RestrictedAccess,
        Self::ControlSample,
        Self::SampleGroup,
        Self::SyntheticConstruct,
        Self::LabStock,
        Self::ThirdPartyData,
        Self::DataAgreementPre2023,
        Self::EndangeredSpecies,
        Self::HumanIdentifiable,
    ];

    /// The INSDC term.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not applicable",
            Self::Missing => "missing",
            Self::NotCollected => "not collected",
            Self::NotProvided => "not provided",
            Self::RestrictedAccess => "restricted access",
            Self::ControlSample => "missing: control sample",
            Self::SampleGroup => "missing: sample group",
            Self::SyntheticConstruct => "missing: synthetic construct",
            Self::LabStock => "missing: lab stock",
            Self::ThirdPartyData => "missing: third party data",
            Self::DataAgreementPre2023 => "missing: data agreement established pre-2023",
            Self::EndangeredSpecies => "missing: endangered species",
            Self::HumanIdentifiable => "missing: human-identifiable",
        }
    }

    /// The reason an INSDC term names, ignoring case and surrounding space.
    pub fn from_term(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL
            .into_iter()
            .find(|r| r.as_str().eq_ignore_ascii_case(text))
    }
}

/// A taxon that specimens can be identified as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Taxon {
    pub id: String,
    /// The name used in this project's research.
    pub name: String,
    /// The name to submit when it differs, such as an informal name for an
    /// undescribed species (`Caulerpa sp. 3 HV-2026`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_name: Option<String>,
    /// NCBI Taxonomy ID, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taxid: Option<u64>,
}

/// A recorded assignment of a specimen to a taxon. Re-identification adds
/// another; it never overwrites the earlier one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Identification {
    pub id: String,
    pub specimen: String,
    pub taxon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identified_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remarks: Option<String>,
}

/// A sequence record in a project document.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct RecordRef {
    /// Project-relative path of the sequence file.
    pub document: String,
    /// The record's ID: its FASTA header up to the first whitespace, or its
    /// alignment row name.
    pub record: String,
}

/// A stored link from a sequence record to the specimen it came from. Many
/// records may link to one specimen. Links are made at import and never
/// re-derived from names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordLink {
    pub document: String,
    pub record: String,
    pub specimen: String,
}

impl RecordLink {
    pub fn record_ref(&self) -> RecordRef {
        RecordRef {
            document: self.document.clone(),
            record: self.record.clone(),
        }
    }
}

/// Saved import settings, so a project's next import repeats its last one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImportSettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inclusion_rules: Vec<crate::submission::InclusionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_patterns: Vec<HeaderPattern>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spreadsheet_mappings: Vec<SpreadsheetMapping>,
}

/// How to read fields from structured FASTA headers, such as
/// `HV1234|Caulerpa taxifolia|2019-05-12`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HeaderPattern {
    pub name: String,
    /// Splits the header line (without `>`). Must not be empty.
    pub separator: String,
    /// What each field holds, in order. Fields past the end are ignored.
    pub fields: Vec<FieldTarget>,
}

/// Remembered spreadsheet column mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpreadsheetMapping {
    pub name: String,
    /// Source column header to what it holds. Unmapped columns are ignored.
    pub columns: BTreeMap<String, FieldTarget>,
}

/// What an imported field holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum FieldTarget {
    Ignore,
    /// The specimen's local ID: the key that rows are matched on.
    Specimen,
    /// A sequence record ID to link to the row's specimen.
    Record,
    /// A taxon name; the specimen is identified as that taxon.
    Taxon,
    /// A value for a specimen column.
    Column {
        column: String,
    },
}

/// One value to set, or with no value, to clear.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CellChange {
    pub specimen: String,
    pub column: String,
    pub value: Option<Cell>,
}

/// Additions and updates applied together: one undo step. Every import
/// route produces one, reviewed before it is applied.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChangeSet {
    /// New specimen columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    /// New specimen IDs, appended to the roster.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specimens: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<CellChange>,
    /// New taxa, or replacements for taxa with the same ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taxa: Vec<Taxon>,
    /// Appended identifications.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifications: Vec<Identification>,
    /// New links, or replacements for a record's existing link.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<RecordLink>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
            && self.specimens.is_empty()
            && self.cells.is_empty()
            && self.taxa.is_empty()
            && self.identifications.is_empty()
            && self.links.is_empty()
    }
}

/// One atomic metadata edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MetadataEdit {
    /// Add or replace a project partition using the shared hypothesis model.
    PutPartition {
        partition: Box<crate::delimitation::Hypothesis>,
    },
    RemovePartition {
        id: String,
    },
    Apply {
        changes: ChangeSet,
    },
    /// Refused while a removed specimen has identifications or links.
    RemoveSpecimens {
        specimens: BTreeSet<String>,
    },
    /// Remove a column and its values.
    RemoveColumn {
        column: String,
    },
    Unlink {
        records: Vec<RecordRef>,
    },
    /// Withdraw identifications recorded in error. Re-identification adds a
    /// new identification instead.
    RemoveIdentifications {
        identifications: BTreeSet<String>,
    },
    SetImportSettings {
        settings: ImportSettings,
    },
}

/// Ordered display fields. These never replace record or specimen identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LabelField {
    Specimen,
    Genus,
    Species,
    Accession,
    Column { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ComposedLabel {
    pub record: String,
    pub specimen: Option<String>,
    pub label: String,
    /// Unknown or ambiguous links retain the original record as the label.
    pub issue: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_keep_their_precision() {
        for text in ["2019", "2019-05", "2019-05-12", "2024-02-29"] {
            let date = PartialDate::parse(text).unwrap();
            assert_eq!(date.to_string(), text);
            let json = serde_json::to_string(&date).unwrap();
            assert_eq!(serde_json::from_str::<PartialDate>(&json).unwrap(), date);
        }
        for text in [
            "19",
            "2019-5",
            "2019-13",
            "2023-02-29",
            "2019-04-31",
            "0000",
            "",
            "2019/05",
        ] {
            assert!(PartialDate::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn missing_reasons_use_insdc_terms() {
        for reason in MissingReason::ALL {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, format!("\"{}\"", reason.as_str()));
            assert_eq!(MissingReason::from_term(reason.as_str()), Some(reason));
        }
        assert_eq!(
            MissingReason::from_term(" Not Collected "),
            Some(MissingReason::NotCollected)
        );
        assert_eq!(MissingReason::from_term("unknown"), None);
    }

    #[test]
    fn edit_json_shape() {
        let edit = MetadataEdit::Apply {
            changes: ChangeSet {
                specimens: vec!["HV1".into()],
                cells: vec![CellChange {
                    specimen: "HV1".into(),
                    column: "eventDate".into(),
                    value: Some(Cell::Date(PartialDate::parse("2019-05").unwrap())),
                }],
                ..ChangeSet::default()
            },
        };
        let json = serde_json::to_string(&edit).unwrap();
        assert_eq!(
            json,
            r#"{"action":"apply","changes":{"specimens":["HV1"],"cells":[{"specimen":"HV1","column":"eventDate","value":{"date":"2019-05"}}]}}"#
        );
        assert_eq!(serde_json::from_str::<MetadataEdit>(&json).unwrap(), edit);
    }

    #[test]
    fn unknown_top_level_fields_survive() {
        let json = r#"{"version":1,"specimens":{"columns":[],"rows":[]},"later":{"a":1}}"#;
        let meta: ProjectMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.extra["later"], serde_json::json!({"a":1}));
        let back = serde_json::to_value(&meta).unwrap();
        assert_eq!(back["later"], serde_json::json!({"a":1}));
    }
}
