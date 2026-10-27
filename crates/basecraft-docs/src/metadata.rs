//! Project specimen metadata: validation, transactional edits, the text form
//! of typed cells, Darwin Core column terms and their INSDC source qualifiers.
use std::collections::{BTreeMap, BTreeSet};

use basecraft_protocol::delimitation::{Group, Hypothesis, HypothesisOrigin, Provenance};
use basecraft_protocol::metadata::{
    Cell, ChangeSet, Column, ColumnKind, Identification, METADATA_VERSION, MetadataEdit,
    MissingReason, PartialDate, ProjectMetadata, RecordRef, Row, Taxon,
};

use crate::alignment::EditError;

fn fail<T>(message: impl Into<String>) -> Result<T, EditError> {
    Err(EditError(message.into()))
}

/// The roster's ID column in tables and spreadsheets.
pub const SPECIMEN_ID: &str = "specimen_id";

/// A Darwin Core (or INSDC-only) column Basecraft knows: its type, what it
/// holds, and the INSDC source qualifier it feeds.
#[derive(Clone, Copy, Debug)]
pub struct KnownColumn {
    pub name: &'static str,
    pub kind: KnownKind,
    pub description: &'static str,
    /// The INSDC source qualifier this term contributes to, if any.
    pub insdc: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownKind {
    Text,
    Date,
    Latitude,
    Longitude,
}

impl KnownColumn {
    pub fn column(&self) -> Column {
        Column {
            name: self.name.into(),
            kind: match self.kind {
                KnownKind::Text => ColumnKind::Text,
                KnownKind::Date => ColumnKind::Date,
                KnownKind::Latitude => ColumnKind::Latitude,
                KnownKind::Longitude => ColumnKind::Longitude,
            },
            description: self.description.into(),
        }
    }
}

/// Collection columns, by Darwin Core term, in template order. `isolate` and
/// `strain` have no Darwin Core term and keep their INSDC names.
pub const KNOWN_COLUMNS: &[KnownColumn] = &[
    KnownColumn {
        name: "catalogNumber",
        kind: KnownKind::Text,
        description: "Voucher number in its collection",
        insdc: Some("specimen_voucher"),
    },
    KnownColumn {
        name: "institutionCode",
        kind: KnownKind::Text,
        description: "Institution holding the voucher (INSDC institution code)",
        insdc: Some("specimen_voucher"),
    },
    KnownColumn {
        name: "collectionCode",
        kind: KnownKind::Text,
        description: "Collection within the institution",
        insdc: Some("specimen_voucher"),
    },
    KnownColumn {
        name: "isolate",
        kind: KnownKind::Text,
        description: "Isolate name, when there is no voucher",
        insdc: Some("isolate"),
    },
    KnownColumn {
        name: "strain",
        kind: KnownKind::Text,
        description: "Culture strain",
        insdc: Some("strain"),
    },
    KnownColumn {
        name: "eventDate",
        kind: KnownKind::Date,
        description: "Collection date, at its true precision",
        insdc: Some("collection_date"),
    },
    KnownColumn {
        name: "country",
        kind: KnownKind::Text,
        description: "Country or ocean (INSDC country list)",
        insdc: Some("geo_loc_name"),
    },
    KnownColumn {
        name: "locality",
        kind: KnownKind::Text,
        description: "Region and locality",
        insdc: Some("geo_loc_name"),
    },
    KnownColumn {
        name: "decimalLatitude",
        kind: KnownKind::Latitude,
        description: "Latitude in decimal degrees (WGS84)",
        insdc: Some("lat_lon"),
    },
    KnownColumn {
        name: "decimalLongitude",
        kind: KnownKind::Longitude,
        description: "Longitude in decimal degrees (WGS84)",
        insdc: Some("lat_lon"),
    },
    KnownColumn {
        name: "recordedBy",
        kind: KnownKind::Text,
        description: "Collector names",
        insdc: Some("collected_by"),
    },
    KnownColumn {
        name: "habitat",
        kind: KnownKind::Text,
        description: "Habitat or environment",
        insdc: None,
    },
];

pub fn known_column(name: &str) -> Option<&'static KnownColumn> {
    KNOWN_COLUMNS.iter().find(|c| c.name == name)
}

/// Whether `name` can be a column name: an ASCII letter, then letters,
/// digits or underscores, at most 64 characters.
pub fn valid_column_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 64
        && bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != SPECIMEN_ID
}

/// Whether `id` can be an identifier: not empty, no surrounding space, no
/// control characters, at most 256 bytes.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.trim() == id && !id.chars().any(char::is_control)
}

fn check_text(what: &str, text: &str) -> Result<(), EditError> {
    if text.trim() != text {
        return fail(format!("{what} {text:?} has surrounding spaces"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
    {
        return fail(format!("{what} contains a control character"));
    }
    if MissingReason::from_term(text).is_some() {
        return fail(format!(
            "{what} is the INSDC term {text:?}; record it as a missing-value reason"
        ));
    }
    Ok(())
}

/// Check a value against its column's kind.
pub fn check_cell(column: &Column, cell: &Cell) -> Result<(), EditError> {
    let what = format!("{} value", column.name);
    match (&column.kind, cell) {
        (_, Cell::Missing(_)) | (ColumnKind::Date, Cell::Date(_)) => Ok(()),
        (ColumnKind::Text, Cell::Text(text)) => {
            if text.is_empty() {
                return fail(format!("{what} is empty; clear the value instead"));
            }
            check_text(&what, text)
        }
        (ColumnKind::Latitude, Cell::Degrees(v)) if v.is_finite() && v.abs() <= 90.0 => Ok(()),
        (ColumnKind::Longitude, Cell::Degrees(v)) if v.is_finite() && v.abs() <= 180.0 => Ok(()),
        (ColumnKind::Latitude | ColumnKind::Longitude, Cell::Degrees(v)) => {
            fail(format!("{what} {v} is outside the valid range"))
        }
        (ColumnKind::Number { .. }, Cell::Number(v)) if v.is_finite() => Ok(()),
        (ColumnKind::Categorical { levels }, Cell::Category(level)) if levels.contains(level) => {
            Ok(())
        }
        (ColumnKind::Categorical { .. }, Cell::Category(level)) => fail(format!(
            "{what} {level:?} is not one of the column's levels"
        )),
        _ => fail(format!("{what} does not match the column type")),
    }
}

/// The text form of a value, as written to TSV and spreadsheets.
pub fn format_cell(cell: &Cell) -> String {
    match cell {
        Cell::Text(text) | Cell::Category(text) => text.clone(),
        Cell::Date(date) => date.to_string(),
        Cell::Degrees(v) | Cell::Number(v) => format_number(*v),
        Cell::Missing(reason) => reason.as_str().into(),
    }
}

/// Shortest text that reads back as the same number.
pub fn format_number(v: f64) -> String {
    let text = format!("{v}");
    if text == "-0" { "0".into() } else { text }
}

/// Read a value for `column` from text. Empty text is no value. INSDC
/// missing-value terms become reasons in every column.
pub fn parse_cell(column: &Column, text: &str) -> Result<Option<Cell>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Some(reason) = MissingReason::from_term(text) {
        return Ok(Some(Cell::Missing(reason)));
    }
    let number = || {
        text.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{text:?} is not a number"))
    };
    let cell = match &column.kind {
        ColumnKind::Text => Cell::Text(text.into()),
        ColumnKind::Date => Cell::Date(PartialDate::parse(text)?),
        ColumnKind::Latitude | ColumnKind::Longitude => Cell::Degrees(number()?),
        ColumnKind::Number { .. } => Cell::Number(number()?),
        ColumnKind::Categorical { levels } => {
            let level = levels
                .iter()
                .find(|l| l.as_str() == text)
                .or_else(|| levels.iter().find(|l| l.eq_ignore_ascii_case(text)))
                .ok_or_else(|| format!("{text:?} is not one of {}", levels.join(", ")))?;
            Cell::Category(level.clone())
        }
    };
    check_cell(column, &cell).map_err(|e| e.0)?;
    Ok(Some(cell))
}

pub fn validate(meta: &ProjectMetadata) -> Result<(), EditError> {
    if meta.version != METADATA_VERSION {
        return fail(format!(
            "metadata version {} is not supported (this Basecraft reads version {METADATA_VERSION})",
            meta.version
        ));
    }
    let mut columns = BTreeMap::new();
    for column in &meta.specimens.columns {
        if !valid_column_name(&column.name) {
            return fail(format!("{:?} is not a valid column name", column.name));
        }
        if columns.insert(column.name.as_str(), column).is_some() {
            return fail(format!("column {} is declared twice", column.name));
        }
        if let ColumnKind::Categorical { levels } = &column.kind {
            let unique: BTreeSet<_> = levels.iter().collect();
            if levels.is_empty() || unique.len() != levels.len() {
                return fail(format!("column {} needs distinct levels", column.name));
            }
            for level in levels {
                if !valid_id(level) {
                    return fail(format!("column {} has an invalid level", column.name));
                }
                check_text(&format!("{} level", column.name), level)?;
            }
        }
        if let ColumnKind::Number { unit: Some(unit) } = &column.kind {
            if !valid_id(unit) {
                return fail(format!("column {} has an invalid unit", column.name));
            }
        }
    }
    let mut roster = BTreeSet::new();
    for row in &meta.specimens.rows {
        if !valid_id(&row.id) {
            return fail(format!("{:?} is not a valid specimen ID", row.id));
        }
        if !roster.insert(row.id.as_str()) {
            return fail(format!("specimen {} appears twice", row.id));
        }
        for (name, cell) in &row.cells {
            let column = columns.get(name.as_str()).ok_or_else(|| {
                EditError(format!("specimen {} has undeclared column {name}", row.id))
            })?;
            check_cell(column, cell)
                .map_err(|e| EditError(format!("specimen {}: {}", row.id, e.0)))?;
        }
    }
    let mut taxa = BTreeSet::new();
    let mut names = BTreeSet::new();
    for taxon in &meta.taxa {
        if !valid_id(&taxon.id) || !taxa.insert(taxon.id.as_str()) {
            return fail(format!("taxon ID {:?} is invalid or repeated", taxon.id));
        }
        if !valid_id(&taxon.name) {
            return fail(format!("taxon {} needs a name", taxon.id));
        }
        check_text("taxon name", &taxon.name)?;
        if !names.insert(taxon.name.as_str()) {
            return fail(format!("two taxa are named {:?}", taxon.name));
        }
        if let Some(submitted) = &taxon.submitted_name {
            if !valid_id(submitted) {
                return fail(format!("taxon {} has an invalid submitted name", taxon.id));
            }
            check_text("submitted name", submitted)?;
        }
    }
    let mut identifications = BTreeSet::new();
    for identification in &meta.identifications {
        if !valid_id(&identification.id) || !identifications.insert(identification.id.as_str()) {
            return fail(format!(
                "identification ID {:?} is invalid or repeated",
                identification.id
            ));
        }
        if !roster.contains(identification.specimen.as_str()) {
            return fail(format!(
                "identification {} refers to unknown specimen {}",
                identification.id, identification.specimen
            ));
        }
        if !taxa.contains(identification.taxon.as_str()) {
            return fail(format!(
                "identification {} refers to unknown taxon {}",
                identification.id, identification.taxon
            ));
        }
        for (what, text) in [
            ("identifier", &identification.identified_by),
            ("identification remarks", &identification.remarks),
        ] {
            if let Some(text) = text {
                if text.is_empty() {
                    return fail(format!("{what} is empty"));
                }
                check_text(what, text)?;
            }
        }
    }
    let mut records = BTreeSet::new();
    for link in &meta.links {
        if !valid_id(&link.document) || !valid_id(&link.record) {
            return fail("a record link has an invalid document or record");
        }
        if !records.insert((link.document.as_str(), link.record.as_str())) {
            return fail(format!(
                "record {} in {} is linked twice",
                link.record, link.document
            ));
        }
        if !roster.contains(link.specimen.as_str()) {
            return fail(format!(
                "record {} links to unknown specimen {}",
                link.record, link.specimen
            ));
        }
    }
    for pattern in &meta.import.header_patterns {
        if pattern.separator.is_empty() || pattern.name.is_empty() {
            return fail("a header pattern needs a name and a separator");
        }
    }
    Ok(())
}

/// Apply one edit transactionally: on error, `meta` is unchanged.
pub fn apply(meta: &mut ProjectMetadata, edit: &MetadataEdit) -> Result<(), EditError> {
    let mut next = meta.clone();
    match edit {
        MetadataEdit::Apply { changes } => apply_changes(&mut next, changes)?,
        MetadataEdit::RemoveSpecimens { specimens } => {
            if specimens.is_empty() {
                return fail("no specimens to remove");
            }
            for id in specimens {
                if !next.specimens.rows.iter().any(|r| &r.id == id) {
                    return fail(format!("specimen {id} is not in the roster"));
                }
            }
            if let Some(i) = next
                .identifications
                .iter()
                .find(|i| specimens.contains(&i.specimen))
            {
                return fail(format!(
                    "specimen {} has identifications; remove them first",
                    i.specimen
                ));
            }
            if let Some(link) = next.links.iter().find(|l| specimens.contains(&l.specimen)) {
                return fail(format!(
                    "specimen {} has linked records; unlink them first",
                    link.specimen
                ));
            }
            next.specimens.rows.retain(|r| !specimens.contains(&r.id));
        }
        MetadataEdit::RemoveColumn { column } => {
            let before = next.specimens.columns.len();
            next.specimens.columns.retain(|c| &c.name != column);
            if next.specimens.columns.len() == before {
                return fail(format!("there is no column {column}"));
            }
            for row in &mut next.specimens.rows {
                row.cells.remove(column);
            }
        }
        MetadataEdit::Unlink { records } => {
            let wanted: BTreeSet<&RecordRef> = records.iter().collect();
            let before = next.links.len();
            next.links.retain(|l| !wanted.contains(&l.record_ref()));
            if next.links.len() + wanted.len() != before {
                return fail("a record to unlink is not linked");
            }
        }
        MetadataEdit::RemoveIdentifications { identifications } => {
            let before = next.identifications.len();
            next.identifications
                .retain(|i| !identifications.contains(&i.id));
            if next.identifications.len() + identifications.len() != before {
                return fail("an identification to remove does not exist");
            }
        }
        MetadataEdit::SetImportSettings { settings } => next.import = settings.clone(),
    }
    validate(&next)?;
    *meta = next;
    Ok(())
}

fn apply_changes(meta: &mut ProjectMetadata, changes: &ChangeSet) -> Result<(), EditError> {
    if changes.is_empty() {
        return fail("the change set is empty");
    }
    for column in &changes.columns {
        if meta.specimens.columns.iter().any(|c| c.name == column.name) {
            return fail(format!("column {} already exists", column.name));
        }
        meta.specimens.columns.push(column.clone());
    }
    for id in &changes.specimens {
        if meta.specimens.rows.iter().any(|r| &r.id == id) {
            return fail(format!("specimen {id} is already in the roster"));
        }
        meta.specimens.rows.push(Row {
            id: id.clone(),
            cells: BTreeMap::new(),
        });
    }
    let index: BTreeMap<String, usize> = meta
        .specimens
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.clone(), i))
        .collect();
    for change in &changes.cells {
        let row = index.get(&change.specimen).ok_or_else(|| {
            EditError(format!("specimen {} is not in the roster", change.specimen))
        })?;
        if !meta
            .specimens
            .columns
            .iter()
            .any(|c| c.name == change.column)
        {
            return fail(format!("there is no column {}", change.column));
        }
        let cells = &mut meta.specimens.rows[*row].cells;
        match &change.value {
            Some(value) => {
                cells.insert(change.column.clone(), value.clone());
            }
            None => {
                cells.remove(&change.column);
            }
        }
    }
    for taxon in &changes.taxa {
        if let Some(existing) = meta.taxa.iter_mut().find(|t| t.id == taxon.id) {
            *existing = taxon.clone();
        } else {
            meta.taxa.push(taxon.clone());
        }
    }
    for identification in &changes.identifications {
        if meta
            .identifications
            .iter()
            .any(|i| i.id == identification.id)
        {
            return fail(format!(
                "identification {} already exists",
                identification.id
            ));
        }
        meta.identifications.push(identification.clone());
    }
    for link in &changes.links {
        if let Some(existing) = meta
            .links
            .iter_mut()
            .find(|l| l.document == link.document && l.record == link.record)
        {
            existing.specimen.clone_from(&link.specimen);
        } else {
            meta.links.push(link.clone());
        }
    }
    Ok(())
}

/// A specimen's current identification: its last recorded one.
pub fn current_identification<'a>(
    meta: &'a ProjectMetadata,
    specimen: &str,
) -> Option<&'a Identification> {
    meta.identifications
        .iter()
        .rev()
        .find(|i| i.specimen == specimen)
}

pub fn taxon<'a>(meta: &'a ProjectMetadata, id: &str) -> Option<&'a Taxon> {
    meta.taxa.iter().find(|t| t.id == id)
}

pub fn taxon_by_name<'a>(meta: &'a ProjectMetadata, name: &str) -> Option<&'a Taxon> {
    meta.taxa.iter().find(|t| t.name == name)
}

/// The specimen a record is linked to.
pub fn linked_specimen<'a>(
    meta: &'a ProjectMetadata,
    document: &str,
    record: &str,
) -> Option<&'a str> {
    meta.links
        .iter()
        .find(|l| l.document == document && l.record == record)
        .map(|l| l.specimen.as_str())
}

/// The records of `records` that have no stored link in `document`.
pub fn unlinked<'a>(meta: &ProjectMetadata, document: &str, records: &'a [String]) -> Vec<&'a str> {
    let linked: BTreeSet<&str> = meta
        .links
        .iter()
        .filter(|l| l.document == document)
        .map(|l| l.record.as_str())
        .collect();
    records
        .iter()
        .map(String::as_str)
        .filter(|r| !linked.contains(r))
        .collect()
}

/// Current identifications as a partition of the roster, in the species
/// hypothesis form other partitions use: one group per taxon, specimens
/// without an identification unassigned. Group IDs are taxon IDs.
pub fn identification_partition(meta: &ProjectMetadata) -> Hypothesis {
    let mut groups: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut unassigned = BTreeSet::new();
    for row in &meta.specimens.rows {
        match current_identification(meta, &row.id) {
            Some(i) => {
                groups
                    .entry(i.taxon.as_str())
                    .or_default()
                    .insert(row.id.clone());
            }
            None => {
                unassigned.insert(row.id.clone());
            }
        }
    }
    Hypothesis {
        id: "identifications".into(),
        name: "Current identifications".into(),
        origin: HypothesisOrigin::Manual,
        parent: None,
        groups: meta
            .taxa
            .iter()
            .filter_map(|t| {
                groups.remove(t.id.as_str()).map(|members| Group {
                    id: t.id.clone(),
                    label: t.name.clone(),
                    color: String::new(),
                    members,
                    scores: Vec::new(),
                    individual_scores: BTreeMap::new(),
                    attributes: BTreeMap::new(),
                })
            })
            .collect(),
        unassigned,
        scores: Vec::new(),
        provenance: Provenance {
            method: "identification".into(),
            ..Provenance::default()
        },
        attributes: BTreeMap::new(),
    }
}

/// INSDC source qualifiers for one specimen, derived from its Darwin Core
/// columns and current identification. `missing` lists the qualifiers
/// INSDC requires for a marker submission that have no value (nor a
/// missing-value reason).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InsdcSource {
    /// Qualifier name and value, in a stable order.
    pub qualifiers: Vec<(String, String)>,
    pub missing: Vec<String>,
}

/// Map a specimen to INSDC source qualifiers:
///
/// | INSDC | from |
/// | --- | --- |
/// | organism | taxon `submitted_name`, else `name`, of the current identification |
/// | specimen_voucher | `institutionCode:collectionCode:catalogNumber` (codes optional) |
/// | isolate, strain | `isolate`, `strain` |
/// | collection_date | `eventDate` at its precision (ISO 8601) |
/// | geo_loc_name | `country`, then `: locality` |
/// | lat_lon | `decimalLatitude`, `decimalLongitude` as `d.dddd N d.dddd W` |
/// | collected_by | `recordedBy` |
/// | identified_by | the current identification's `identified_by` |
///
/// A missing-value reason is passed through as the qualifier value.
pub fn insdc_source(meta: &ProjectMetadata, specimen: &str) -> Result<InsdcSource, EditError> {
    let row = meta
        .specimens
        .rows
        .iter()
        .find(|r| r.id == specimen)
        .ok_or_else(|| EditError(format!("specimen {specimen} is not in the roster")))?;
    let get = |name: &str| row.cells.get(name);
    let text = |name: &str| get(name).map(format_cell);
    let mut out = InsdcSource::default();
    let mut push = |name: &str, value: Option<String>| {
        if let Some(value) = value {
            out.qualifiers.push((name.into(), value));
        }
    };
    let identification = current_identification(meta, specimen);
    let organism = identification
        .and_then(|i| taxon(meta, &i.taxon))
        .map(|t| t.submitted_name.clone().unwrap_or_else(|| t.name.clone()));
    push("organism", organism);
    let voucher = match get("catalogNumber") {
        Some(Cell::Missing(reason)) => Some(reason.as_str().to_string()),
        Some(cell) => {
            let mut parts = Vec::new();
            for code in ["institutionCode", "collectionCode"] {
                if let Some(Cell::Text(t)) = get(code) {
                    parts.push(t.clone());
                }
            }
            parts.push(format_cell(cell));
            Some(parts.join(":"))
        }
        None => None,
    };
    push("specimen_voucher", voucher);
    push("isolate", text("isolate"));
    push("strain", text("strain"));
    push("collection_date", text("eventDate"));
    let geo = match (get("country"), get("locality")) {
        (Some(Cell::Text(country)), Some(Cell::Text(locality))) => {
            Some(format!("{country}: {locality}"))
        }
        (Some(country), _) => Some(format_cell(country)),
        (None, _) => None,
    };
    push("geo_loc_name", geo);
    let lat_lon = match (get("decimalLatitude"), get("decimalLongitude")) {
        (Some(Cell::Degrees(lat)), Some(Cell::Degrees(lon))) => Some(format!(
            "{:.4} {} {:.4} {}",
            lat.abs(),
            if *lat < 0.0 { "S" } else { "N" },
            lon.abs(),
            if *lon < 0.0 { "W" } else { "E" }
        )),
        (Some(Cell::Missing(reason)), _) | (_, Some(Cell::Missing(reason))) => {
            Some(reason.as_str().into())
        }
        _ => None,
    };
    push("lat_lon", lat_lon);
    push("collected_by", text("recordedBy"));
    push(
        "identified_by",
        identification.and_then(|i| i.identified_by.clone()),
    );
    let has = |name: &str| out.qualifiers.iter().any(|(q, _)| q == name);
    let mut missing = Vec::new();
    for required in ["organism", "collection_date", "geo_loc_name"] {
        if !has(required) {
            missing.push(required.to_string());
        }
    }
    if !has("specimen_voucher") && !has("isolate") && !has("strain") {
        missing.push("specimen_voucher, isolate or strain".into());
    }
    out.missing = missing;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::metadata::{CellChange, RecordLink};

    fn base() -> ProjectMetadata {
        let mut meta = ProjectMetadata::default();
        apply(
            &mut meta,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    columns: KNOWN_COLUMNS.iter().map(KnownColumn::column).collect(),
                    specimens: vec!["HV1".into(), "HV2".into()],
                    taxa: vec![Taxon {
                        id: "t1".into(),
                        name: "Caulerpa taxifolia".into(),
                        submitted_name: None,
                        taxid: Some(76_317),
                    }],
                    ..ChangeSet::default()
                },
            },
        )
        .unwrap();
        meta
    }

    fn set(meta: &mut ProjectMetadata, specimen: &str, column: &str, text: &str) {
        let col = meta
            .specimens
            .columns
            .iter()
            .find(|c| c.name == column)
            .unwrap()
            .clone();
        let value = parse_cell(&col, text).unwrap();
        apply(
            meta,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    cells: vec![CellChange {
                        specimen: specimen.into(),
                        column: column.into(),
                        value,
                    }],
                    ..ChangeSet::default()
                },
            },
        )
        .unwrap();
    }

    #[test]
    fn cells_follow_their_column_type() {
        let date = known_column("eventDate").unwrap().column();
        assert_eq!(
            parse_cell(&date, "2019-05").unwrap(),
            Some(Cell::Date(PartialDate::parse("2019-05").unwrap()))
        );
        assert!(parse_cell(&date, "May 2019").is_err());
        assert_eq!(
            parse_cell(&date, "not collected").unwrap(),
            Some(Cell::Missing(MissingReason::NotCollected))
        );
        let lat = known_column("decimalLatitude").unwrap().column();
        assert!(parse_cell(&lat, "91").is_err());
        assert_eq!(parse_cell(&lat, " ").unwrap(), None);
        let cat = Column {
            name: "sex".into(),
            kind: ColumnKind::Categorical {
                levels: vec!["female".into(), "male".into()],
            },
            description: String::new(),
        };
        assert_eq!(
            parse_cell(&cat, "Female").unwrap(),
            Some(Cell::Category("female".into()))
        );
        assert!(parse_cell(&cat, "other").is_err());
        let text = known_column("locality").unwrap().column();
        // A text that is an INSDC term is stored as the reason, never as text.
        assert!(check_cell(&text, &Cell::Text("missing".into())).is_err());
    }

    #[test]
    fn edits_are_transactional_and_referential() {
        let mut meta = base();
        let before = meta.clone();
        let bad = MetadataEdit::Apply {
            changes: ChangeSet {
                specimens: vec!["HV3".into()],
                identifications: vec![Identification {
                    id: "i1".into(),
                    specimen: "HV3".into(),
                    taxon: "nope".into(),
                    identified_by: None,
                    date: None,
                    remarks: None,
                }],
                ..ChangeSet::default()
            },
        };
        assert!(apply(&mut meta, &bad).is_err());
        assert_eq!(meta, before);
        let identify = |id: &str, taxon: &str| MetadataEdit::Apply {
            changes: ChangeSet {
                identifications: vec![Identification {
                    id: id.into(),
                    specimen: "HV1".into(),
                    taxon: taxon.into(),
                    identified_by: Some("H. Verbruggen".into()),
                    date: None,
                    remarks: None,
                }],
                ..ChangeSet::default()
            },
        };
        apply(&mut meta, &identify("i1", "t1")).unwrap();
        apply(
            &mut meta,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    taxa: vec![Taxon {
                        id: "t2".into(),
                        name: "Caulerpa sp. 3".into(),
                        submitted_name: Some("Caulerpa sp. 3 HV-2026".into()),
                        taxid: None,
                    }],
                    links: vec![RecordLink {
                        document: "rbcL.fasta".into(),
                        record: "HV1_rbcL".into(),
                        specimen: "HV1".into(),
                    }],
                    ..ChangeSet::default()
                },
            },
        )
        .unwrap();
        // Re-identification adds a record; the earlier one stays.
        apply(&mut meta, &identify("i2", "t2")).unwrap();
        assert_eq!(meta.identifications.len(), 2);
        assert_eq!(current_identification(&meta, "HV1").unwrap().taxon, "t2");
        let partition = identification_partition(&meta);
        assert_eq!(partition.groups.len(), 1);
        assert_eq!(partition.groups[0].id, "t2");
        assert!(partition.unassigned.contains("HV2"));
        // A linked or identified specimen cannot silently disappear.
        let remove = MetadataEdit::RemoveSpecimens {
            specimens: ["HV1".to_string()].into(),
        };
        assert!(apply(&mut meta, &remove).is_err());
        assert_eq!(
            unlinked(&meta, "rbcL.fasta", &["HV1_rbcL".into(), "HV2_rbcL".into()]),
            ["HV2_rbcL"]
        );
    }

    #[test]
    fn insdc_qualifiers_come_from_darwin_core() {
        let mut meta = base();
        set(&mut meta, "HV1", "institutionCode", "MELU");
        set(&mut meta, "HV1", "catalogNumber", "A123");
        set(&mut meta, "HV1", "eventDate", "2019-05");
        set(&mut meta, "HV1", "country", "Australia");
        set(&mut meta, "HV1", "locality", "Victoria, Port Phillip Bay");
        set(&mut meta, "HV1", "decimalLatitude", "-38.1");
        set(&mut meta, "HV1", "decimalLongitude", "144.85");
        let source = insdc_source(&meta, "HV1").unwrap();
        let map: BTreeMap<_, _> = source.qualifiers.iter().cloned().collect();
        assert_eq!(map["specimen_voucher"], "MELU:A123");
        assert_eq!(map["collection_date"], "2019-05");
        assert_eq!(map["geo_loc_name"], "Australia: Victoria, Port Phillip Bay");
        assert_eq!(map["lat_lon"], "38.1000 S 144.8500 E");
        assert_eq!(source.missing, ["organism"]);
        set(&mut meta, "HV2", "eventDate", "not collected");
        let other = insdc_source(&meta, "HV2").unwrap();
        assert!(
            other
                .qualifiers
                .contains(&("collection_date".into(), "not collected".into()))
        );
    }
}
