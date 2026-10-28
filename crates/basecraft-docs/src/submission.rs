//! GenBank marker submissions: validation, transactional edits, and the
//! preparation of submission records from alignment rows and project
//! metadata.
//!
//! Preparation never changes biological sequence silently. It removes
//! alignment gaps only; `?` becomes `N` (both mean an unknown base);
//! ambiguity codes and internal `N` are kept. Terminal runs of unknown
//! bases are trimmed only when the export setting asks for it, and every
//! exported base keeps its alignment column in the coordinate map.
use std::collections::{BTreeMap, BTreeSet};

use basecraft_protocol::metadata::ProjectMetadata;
use basecraft_protocol::submission::{
    AccessionEntry, Decision, EndPolicy, FrameChoice, ItsComponent, Marker, RowDecision,
    SUBMISSION_VERSION, Submission, SubmissionEdit,
};
use serde::{Deserialize, Serialize};

use crate::genetic_code::{self, GeneticCode};
use crate::metadata::{current_identification, insdc_source, linked_specimen, taxon};

/// NCBI's limit for sequence IDs in submission files.
pub const MAX_EXPORT_ID: usize = 25;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError(pub String);

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

fn err(message: impl Into<String>) -> EditError {
    EditError(message.into())
}

/// Text written into tab-separated submission files: no tabs, line
/// breaks or other control characters, at most `max` characters.
fn check_text(what: &str, text: &str, max: usize) -> Result<(), EditError> {
    if text.chars().any(char::is_control) {
        return Err(err(format!(
            "{what} must not contain tabs, line breaks or control characters"
        )));
    }
    if text.chars().count() > max {
        return Err(err(format!("{what} is longer than {max} characters")));
    }
    if text.trim() != text {
        return Err(err(format!("{what} must not start or end with spaces")));
    }
    Ok(())
}

/// Whether `text` looks like an INSDC nucleotide accession, optionally
/// with its version: one or two letters and five, six or eight digits
/// (`U12345`, `MZ123456.1`, `OQ12345678`).
pub fn valid_accession(text: &str) -> bool {
    let (base, version) = text.split_once('.').unwrap_or((text, ""));
    let letters = base.bytes().take_while(u8::is_ascii_uppercase).count();
    let digits = &base[letters..];
    (1..=2).contains(&letters)
        && matches!(digits.len(), 5 | 6 | 8)
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (version.is_empty() || version.bytes().all(|b| b.is_ascii_digit()))
        && !(text.contains('.') && version.is_empty())
}

fn check_record(record: &str) -> Result<(), EditError> {
    if record.is_empty() || record.len() > 256 || record.chars().any(char::is_control) {
        return Err(err(format!("{record:?} is not a record name")));
    }
    Ok(())
}

/// Check a whole submission document.
pub fn validate(sub: &Submission) -> Result<(), EditError> {
    if sub.version != SUBMISSION_VERSION {
        return Err(err(format!(
            "submission format version {} is not supported",
            sub.version
        )));
    }
    if let Some(source) = &sub.source {
        if source.document.is_empty() || source.hash.is_empty() {
            return Err(err("the source alignment needs a path and a hash"));
        }
    }
    let batch = &sub.batch;
    if genetic_code::code(batch.genetic_code).is_none() {
        return Err(err(format!(
            "genetic code {} is not supported",
            batch.genetic_code
        )));
    }
    match &batch.marker {
        Marker::Coding { gene, product } => {
            check_text("gene", gene, 100)?;
            check_text("product", product, 500)?;
        }
        Marker::Rrna { product } => check_text("product", product, 500)?,
        Marker::Its { components } => {
            let unique: BTreeSet<_> = components.iter().collect();
            if unique.len() != components.len() {
                return Err(err("an ITS component is listed twice"));
            }
        }
        Marker::Spacer { description } => check_text("description", description, 500)?,
    }
    let mut seen = BTreeSet::new();
    for row in &sub.rows {
        check_record(&row.record)?;
        if !seen.insert(&row.record) {
            return Err(err(format!("row {} has two decisions", row.record)));
        }
        if let Decision::Existing { accession } = &row.decision {
            if !valid_accession(accession) {
                return Err(err(format!("{accession:?} is not an accession")));
            }
        }
    }
    let mut seen = BTreeSet::new();
    for frame in &sub.frames {
        check_record(&frame.record)?;
        if !(1..=3).contains(&frame.codon_start) {
            return Err(err("codon_start must be 1, 2 or 3"));
        }
        if !seen.insert(&frame.record) {
            return Err(err(format!(
                "record {} has two reading frames",
                frame.record
            )));
        }
    }
    check_text("title", &sub.publication.title, 2000)?;
    for author in &sub.publication.authors {
        if author.last.is_empty() {
            return Err(err("every author needs a last name"));
        }
        check_text("author last name", &author.last, 100)?;
        check_text("author first name", &author.first, 100)?;
        check_text("author middle initials", &author.middle, 20)?;
    }
    let mut seen = BTreeSet::new();
    for entry in &sub.accessions {
        check_record(&entry.record)?;
        if !valid_accession(&entry.accession) {
            return Err(err(format!("{:?} is not an accession", entry.accession)));
        }
        if !seen.insert(&entry.record) {
            return Err(err(format!("record {} has two accessions", entry.record)));
        }
    }
    Ok(())
}

fn upsert<T: Clone>(list: &mut Vec<T>, items: &[T], key: impl Fn(&T) -> &str) {
    for item in items {
        if let Some(existing) = list.iter_mut().find(|e| key(e) == key(item)) {
            *existing = item.clone();
        } else {
            list.push(item.clone());
        }
    }
}

/// Apply one edit transactionally: on error, `sub` is unchanged.
pub fn apply(sub: &mut Submission, edit: &SubmissionEdit) -> Result<(), EditError> {
    let mut next = sub.clone();
    match edit {
        SubmissionEdit::SetSource { source } => next.source = Some(source.clone()),
        SubmissionEdit::SetRows { rows } => {
            upsert(&mut next.rows, rows, |r: &RowDecision| &r.record);
        }
        SubmissionEdit::SetBatch { batch } => next.batch = batch.clone(),
        SubmissionEdit::SetFrames { frames, clear } => {
            next.frames.retain(|f| !clear.contains(&f.record));
            upsert(&mut next.frames, frames, |f: &FrameChoice| &f.record);
        }
        SubmissionEdit::SetExport { export } => next.export = export.clone(),
        SubmissionEdit::SetPublication { publication } => next.publication = publication.clone(),
        SubmissionEdit::SetAccessions { accessions } => {
            upsert(&mut next.accessions, accessions, |a: &AccessionEntry| {
                &a.record
            });
        }
    }
    validate(&next)?;
    *sub = next;
    Ok(())
}

/// A row's decision; rows without one are new.
pub fn decision<'a>(sub: &'a Submission, record: &str) -> &'a Decision {
    static NEW: Decision = Decision::New;
    sub.rows
        .iter()
        .find(|r| r.record == record)
        .map_or(&NEW, |r| &r.decision)
}

/// The accession recorded for a record: from the archive, else an
/// existing accession given as its decision.
pub fn accession<'a>(sub: &'a Submission, record: &str) -> Option<&'a str> {
    sub.accessions
        .iter()
        .find(|a| a.record == record)
        .map(|a| a.accession.as_str())
        .or_else(|| match decision(sub, record) {
            Decision::Existing { accession } => Some(accession),
            _ => None,
        })
}

/// The note an ITS feature carries, such as
/// `contains internal transcribed spacer 1, 5.8S ribosomal RNA, and
/// internal transcribed spacer 2`. Components are listed 5' to 3'.
pub fn its_note(components: &[ItsComponent]) -> String {
    let names: Vec<&str> = ItsComponent::ALL
        .iter()
        .filter(|c| components.contains(c))
        .map(|c| c.description())
        .collect();
    let list = match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    };
    format!("contains {list}")
}

// Preparation.

/// How serious a finding is. Blocking issues keep a package from being
/// ready; review issues need a look but don't block; unavailable means a
/// check could not run, which says nothing about the sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocking,
    Review,
    Info,
    Unavailable,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocking => "blocking",
            Self::Review => "review",
            Self::Info => "info",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A finding, tied to the record (and specimen) it concerns when it
/// concerns one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    /// `basecraft` for local checks; `table2asn` for its reports.
    pub source: String,
    /// A stable code: Basecraft's, or the tool's (`SEQ_INST.TerminalNs`).
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specimen: Option<String>,
    /// The tool's own severity word, when it reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_severity: Option<String>,
}

impl Issue {
    fn local(severity: Severity, code: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            source: "basecraft".into(),
            code: code.into(),
            message: message.into(),
            record: None,
            specimen: None,
            tool_severity: None,
        }
    }
}

/// An alignment row as read from the source file.
#[derive(Clone, Debug)]
pub struct SourceRow {
    pub name: String,
    /// Aligned residues, gaps included.
    pub residues: Vec<u8>,
}

/// A run of exported bases that were consecutive alignment columns.
/// Coordinates are 0-based, half-open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub export_start: u64,
    pub export_end: u64,
    pub column_start: u64,
}

/// The single feature a record gets, spanning the whole record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSpec {
    /// `CDS`, `rRNA`, `misc_RNA` or `misc_feature`.
    pub kind: String,
    /// Gene symbol: coding markers also get a `gene` feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gene: Option<String>,
    pub partial5: bool,
    pub partial3: bool,
    pub qualifiers: Vec<(String, String)>,
}

/// Reading frame detection and review for a coding record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameReport {
    /// Frames (`codon_start`) without an internal stop codon.
    pub candidates: Vec<u8>,
    /// The one clean frame, when exactly one is.
    pub detected: Option<u8>,
    /// The frame the user confirmed or chose.
    pub chosen: Option<u8>,
    /// What is exported: the chosen frame, else the detected one.
    pub effective: Option<u8>,
    /// Translation of the exported residues in the effective frame.
    pub translation: String,
    /// 1-based codon numbers of internal stop codons in that frame.
    pub internal_stops: Vec<usize>,
}

/// One alignment row as it will be exported (or why it won't be).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreparedRecord {
    pub row: usize,
    pub record: String,
    pub decision: Decision,
    /// The ID in submission files; only new records have one.
    pub export_id: Option<String>,
    pub specimen: Option<String>,
    /// The name submitted as `organism`.
    pub organism: Option<String>,
    /// The research identification, when it differs from `organism`.
    pub research_name: Option<String>,
    /// INSDC source qualifiers other than `organism`.
    pub qualifiers: Vec<(String, String)>,
    /// Exported bases.
    pub residues: String,
    /// Unknown bases trimmed from the 5' and 3' ends.
    pub trimmed: (u64, u64),
    pub segments: Vec<Segment>,
    pub feature: Option<FeatureSpec>,
    pub frame: Option<FrameReport>,
    pub accession: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prepared {
    pub records: Vec<PreparedRecord>,
    pub issues: Vec<Issue>,
}

impl Prepared {
    /// Records exported for submission, in row order.
    pub fn exported(&self) -> impl Iterator<Item = &PreparedRecord> {
        self.records.iter().filter(|r| r.export_id.is_some())
    }

    pub fn blocking(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Blocking)
            .count()
    }
}

/// A safe export ID for a record name: letters, digits, `_`, `-` and `.`
/// kept, anything else `_`, at most [`MAX_EXPORT_ID`] characters, unique
/// among `taken` (a numeric suffix is added when needed).
pub fn export_id(record: &str, taken: &BTreeSet<String>) -> String {
    let mut base: String = record
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if base.is_empty() || base.starts_with(['.', '-']) {
        base.insert(0, 'S');
    }
    base.truncate(MAX_EXPORT_ID);
    if !taken.contains(&base) {
        return base;
    }
    (2..=u32::MAX)
        .map(|n| {
            let suffix = format!("_{n}");
            let mut id = base.clone();
            id.truncate(MAX_EXPORT_ID - suffix.len());
            id + &suffix
        })
        .find(|id| !taken.contains(id))
        .unwrap_or_default()
}

const IUPAC: &[u8] = b"ACGTRYSWKMBDHVN";

fn unknown(b: u8) -> bool {
    b == b'N'
}

/// The submitted organism name: the identification's submitted name, or
/// for a specimen identified only to genus (`Genus sp.`), that name with
/// the isolate, strain or voucher appended so it names one organism.
fn organism_name(name: &str, qualifiers: &[(String, String)]) -> String {
    let mut words = name.split_whitespace();
    let only_genus = matches!(
        (words.next(), words.next(), words.next()),
        (Some(_), Some("sp."), None)
    );
    if !only_genus {
        return name.to_string();
    }
    let get = |q: &str| {
        qualifiers
            .iter()
            .find(|(k, _)| k == q)
            .map(|(_, v)| v.as_str())
            .filter(|v| basecraft_protocol::metadata::MissingReason::from_term(v).is_none())
    };
    let label = get("isolate")
        .or_else(|| get("strain"))
        .or_else(|| get("specimen_voucher").map(|v| v.rsplit(':').next().unwrap_or(v)));
    label.map_or_else(|| name.to_string(), |label| format!("{name} {label}"))
}

/// Find the frames of `seq` without internal stop codons. A stop codon
/// ending exactly at the end of the sequence is terminal, not internal.
fn stops(code: &GeneticCode, seq: &[u8], codon_start: u8) -> Vec<usize> {
    let offset = usize::from(codon_start - 1);
    let Some(rest) = seq.get(offset..) else {
        return Vec::new();
    };
    let codons: Vec<&[u8]> = rest.chunks_exact(3).collect();
    let ends_on_codon = rest.len() % 3 == 0;
    codons
        .iter()
        .enumerate()
        .filter(|(i, codon)| code.is_stop(codon) && !(ends_on_codon && *i + 1 == codons.len()))
        .map(|(i, _)| i + 1)
        .collect()
}

struct Context<'a> {
    sub: &'a Submission,
    meta: &'a ProjectMetadata,
    document: &'a str,
    code: &'static GeneticCode,
    masked: &'a BTreeSet<u64>,
}

/// Prepare every row of the source alignment: its decision, extraction,
/// specimen data, feature and reading frame, with the issues found.
/// `document` is the alignment's project-relative path (record links are
/// stored against it).
pub fn prepare(
    sub: &Submission,
    rows: &[SourceRow],
    meta: &ProjectMetadata,
    document: &str,
) -> Prepared {
    let mut out = Prepared::default();
    let Some(code) = genetic_code::code(sub.batch.genetic_code) else {
        out.issues.push(Issue::local(
            Severity::Blocking,
            "genetic_code",
            format!("genetic code {} is not supported", sub.batch.genetic_code),
        ));
        return out;
    };
    batch_issues(sub, &mut out.issues);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows {
        *counts.entry(row.name.as_str()).or_default() += 1;
    }
    // Columns where every new row is unknown or a gap, and some row is
    // unknown: masked in the alignment.
    let new_rows: Vec<&SourceRow> = rows
        .iter()
        .filter(|r| matches!(decision(sub, &r.name), Decision::New))
        .collect();
    let width = rows.iter().map(|r| r.residues.len()).max().unwrap_or(0);
    let masked: BTreeSet<u64> = (0..width)
        .filter(|&col| {
            let mut any = false;
            let all = new_rows.iter().all(|r| match r.residues.get(col) {
                Some(b'N' | b'n' | b'?') => {
                    any = true;
                    true
                }
                Some(b) => crate::alignment::is_gap(*b),
                None => true,
            });
            all && any
        })
        .map(|col| col as u64)
        .collect();
    let mut taken = BTreeSet::new();
    let context = Context {
        sub,
        meta,
        document,
        code,
        masked: &masked,
    };
    for (index, row) in rows.iter().enumerate() {
        let mut issues = Vec::new();
        if counts[row.name.as_str()] > 1 {
            issues.push(Issue::local(
                Severity::Blocking,
                "duplicate_record",
                format!(
                    "{} rows are named {}; decisions and specimen links can't tell them apart. Rename them in the alignment",
                    counts[row.name.as_str()],
                    row.name
                ),
            ));
        }
        let record = prepare_row(&context, index, row, &mut taken, &mut issues);
        for mut issue in issues {
            issue.record = Some(row.name.clone());
            issue.specimen.clone_from(&record.specimen);
            out.issues.push(issue);
        }
        out.records.push(record);
    }
    if out.exported().next().is_none() {
        out.issues.push(Issue::local(
            Severity::Blocking,
            "no_records",
            "No rows are marked as new sequences",
        ));
    }
    out
}

fn batch_issues(sub: &Submission, issues: &mut Vec<Issue>) {
    let mut block = |code: &str, message: &str| {
        issues.push(Issue::local(Severity::Blocking, code, message));
    };
    if sub.source.is_none() {
        block("no_source", "Choose the source alignment");
    }
    match &sub.batch.marker {
        Marker::Coding { product, .. } | Marker::Rrna { product } if product.is_empty() => {
            block("no_product", "Enter the product name for the batch");
        }
        Marker::Its { components } if components.is_empty() => {
            block(
                "no_its_components",
                "Choose the ITS components the sequences contain",
            );
        }
        Marker::Spacer { description } if description.is_empty() => {
            block("no_description", "Describe the region the sequences cover");
        }
        _ => {}
    }
    if sub.publication.title.is_empty() {
        block("no_title", "Enter the publication title");
    }
    if sub.publication.authors.is_empty() {
        block("no_authors", "Add at least one author");
    }
}

fn prepare_row(
    cx: &Context<'_>,
    index: usize,
    row: &SourceRow,
    taken: &mut BTreeSet<String>,
    issues: &mut Vec<Issue>,
) -> PreparedRecord {
    let decision = decision(cx.sub, &row.name).clone();
    let specimen = linked_specimen(cx.meta, cx.document, &row.name).map(str::to_string);
    let mut record = PreparedRecord {
        row: index,
        record: row.name.clone(),
        decision: decision.clone(),
        export_id: None,
        specimen: specimen.clone(),
        organism: None,
        research_name: None,
        qualifiers: Vec::new(),
        residues: String::new(),
        trimmed: (0, 0),
        segments: Vec::new(),
        feature: None,
        frame: None,
        accession: accession(cx.sub, &row.name).map(str::to_string),
    };
    if decision != Decision::New {
        return record;
    }
    let id = export_id(&row.name, taken);
    taken.insert(id.clone());
    if id != row.name {
        issues.push(Issue::local(
            Severity::Info,
            "export_id",
            format!("Exported as {id}: NCBI IDs are at most 25 letters, digits, _ - or ."),
        ));
    }
    record.export_id = Some(id);
    // Ungap, keeping each base's column.
    let mut bases: Vec<(u8, u64)> = Vec::with_capacity(row.residues.len());
    let mut invalid = BTreeSet::new();
    for (col, &b) in row.residues.iter().enumerate() {
        if crate::alignment::is_gap(b) {
            continue;
        }
        let b = if b == b'?' {
            b'N'
        } else {
            b.to_ascii_uppercase()
        };
        if !IUPAC.contains(&b) {
            invalid.insert(b as char);
        }
        bases.push((b, col as u64));
    }
    if !invalid.is_empty() {
        issues.push(Issue::local(
            Severity::Blocking,
            "invalid_residue",
            format!(
                "Characters that are not IUPAC nucleotide codes: {}",
                invalid.iter().collect::<String>()
            ),
        ));
    }
    let lead = bases.iter().take_while(|(b, _)| unknown(*b)).count();
    if lead == bases.len() {
        issues.push(Issue::local(
            Severity::Blocking,
            "no_bases",
            "The row has no known bases",
        ));
        return record;
    }
    let trail = bases.iter().rev().take_while(|(b, _)| unknown(*b)).count();
    if lead + trail > 0 {
        if cx.sub.export.trim_terminal_unknown {
            issues.push(Issue::local(
                Severity::Info,
                "trimmed_unknown",
                format!("Trimmed unknown bases: {lead} at the 5' end, {trail} at the 3' end"),
            ));
            bases.truncate(bases.len() - trail);
            bases.drain(..lead);
            record.trimmed = (lead as u64, trail as u64);
        } else {
            issues.push(Issue::local(
                Severity::Review,
                "terminal_unknown",
                format!("Unknown bases at the ends ({lead} at 5', {trail} at 3'); NCBI flags sequences that start or end with N"),
            ));
        }
    }
    let (keep5, keep3) = if record.trimmed == (0, 0) {
        (lead, trail)
    } else {
        (0, 0)
    };
    let internal_n = bases[keep5..bases.len() - keep3]
        .iter()
        .filter(|(b, _)| unknown(*b))
        .count();
    if internal_n > 0 {
        issues.push(Issue::local(
            Severity::Review,
            "internal_unknown",
            format!("{internal_n} unknown bases (N) inside the sequence are kept"),
        ));
    }
    let masked = bases
        .iter()
        .filter(|(_, col)| cx.masked.contains(col))
        .count();
    if masked > 0 {
        issues.push(Issue::local(
            Severity::Review,
            "masked_columns",
            format!("{masked} bases fall in alignment columns that are unknown in every row (masked); they are exported as N. Check that the region was really sequenced"),
        ));
    }
    let ambiguous = bases
        .iter()
        .filter(|(b, _)| !matches!(b, b'A' | b'C' | b'G' | b'T' | b'N'))
        .count();
    if ambiguous > 0 {
        issues.push(Issue::local(
            Severity::Info,
            "ambiguity_codes",
            format!("{ambiguous} IUPAC ambiguity codes are kept"),
        ));
    }
    for (i, &(_, col)) in bases.iter().enumerate() {
        let i = i as u64;
        match record.segments.last_mut() {
            Some(s) if s.column_start + (s.export_end - s.export_start) == col => s.export_end += 1,
            _ => record.segments.push(Segment {
                export_start: i,
                export_end: i + 1,
                column_start: col,
            }),
        }
    }
    let residues: Vec<u8> = bases.iter().map(|(b, _)| *b).collect();
    record.residues = String::from_utf8_lossy(&residues).into_owned();
    source_data(cx, &mut record, issues);
    feature(cx, &mut record, &residues, issues);
    record
}

fn source_data(cx: &Context<'_>, record: &mut PreparedRecord, issues: &mut Vec<Issue>) {
    let Some(specimen) = record.specimen.clone() else {
        issues.push(Issue::local(
            Severity::Blocking,
            "unlinked",
            format!(
                "Not linked to a specimen in {}; link it with a metadata import",
                cx.document
            ),
        ));
        return;
    };
    let source = match insdc_source(cx.meta, &specimen) {
        Ok(source) => source,
        Err(e) => {
            issues.push(Issue::local(Severity::Blocking, "specimen", e.0));
            return;
        }
    };
    for missing in &source.missing {
        issues.push(Issue::local(
            Severity::Blocking,
            "missing_source",
            format!("Specimen {specimen} has no {missing} (enter a value or an INSDC missing-value reason)"),
        ));
    }
    let mut qualifiers = source.qualifiers;
    if let Some(at) = qualifiers.iter().position(|(q, _)| q == "organism") {
        let (_, name) = qualifiers.remove(at);
        let organism = organism_name(&name, &qualifiers);
        if organism != name {
            issues.push(Issue::local(
                Severity::Info,
                "organism_name",
                format!("Submitted as {organism}: identified only to genus"),
            ));
        }
        let research = current_identification(cx.meta, &specimen)
            .and_then(|i| taxon(cx.meta, &i.taxon))
            .map(|t| t.name.clone());
        record.research_name = research.filter(|r| *r != organism);
        record.organism = Some(organism);
    }
    record.qualifiers = qualifiers;
}

fn feature(cx: &Context<'_>, record: &mut PreparedRecord, seq: &[u8], issues: &mut Vec<Issue>) {
    let policy = cx.sub.batch.ends;
    let (kind, qualifiers, gene) = match &cx.sub.batch.marker {
        Marker::Coding { gene, product } => {
            let frame = frame_report(cx, &record.record, seq, issues);
            let Some(start) = frame.effective else {
                record.frame = Some(frame);
                return;
            };
            let offset = usize::from(start - 1);
            let rest = &seq[offset.min(seq.len())..];
            let starts = rest.get(..3).is_some_and(|c| cx.code.is_start(c));
            let stops =
                rest.len() % 3 == 0 && rest.len() >= 3 && cx.code.is_stop(&rest[rest.len() - 3..]);
            let (partial5, partial3) = match policy {
                EndPolicy::Auto => (!(start == 1 && starts), !stops),
                EndPolicy::Partial => (true, true),
                EndPolicy::Complete => {
                    if start != 1 {
                        issues.push(Issue::local(
                            Severity::Blocking,
                            "complete_frame",
                            "A complete 5' end needs codon_start 1",
                        ));
                    }
                    if !starts {
                        issues.push(Issue::local(
                            Severity::Review,
                            "no_start_codon",
                            "Marked complete but the first codon is not a start codon",
                        ));
                    }
                    if !stops {
                        issues.push(Issue::local(
                            Severity::Review,
                            "no_stop_codon",
                            "Marked complete but the sequence does not end with a stop codon",
                        ));
                    }
                    (false, false)
                }
            };
            record.frame = Some(frame);
            record.feature = Some(FeatureSpec {
                kind: "CDS".into(),
                gene: (!gene.is_empty()).then(|| gene.clone()),
                partial5,
                partial3,
                qualifiers: vec![
                    ("product".into(), product.clone()),
                    ("codon_start".into(), start.to_string()),
                    ("transl_table".into(), cx.code.id.to_string()),
                ],
            });
            return;
        }
        Marker::Rrna { product } => ("rRNA", vec![("product".into(), product.clone())], None),
        Marker::Its { components } => (
            "misc_RNA",
            vec![("note".into(), its_note(components))],
            None,
        ),
        Marker::Spacer { description } => (
            "misc_feature",
            vec![("note".into(), description.clone())],
            None,
        ),
    };
    let partial = policy != EndPolicy::Complete;
    record.feature = Some(FeatureSpec {
        kind: kind.into(),
        gene,
        partial5: partial,
        partial3: partial,
        qualifiers,
    });
}

fn frame_report(cx: &Context<'_>, name: &str, seq: &[u8], issues: &mut Vec<Issue>) -> FrameReport {
    let candidates: Vec<u8> = (1..=3)
        .filter(|&f| usize::from(f) + 2 <= seq.len() && stops(cx.code, seq, f).is_empty())
        .collect();
    let detected = (candidates.len() == 1).then(|| candidates[0]);
    let chosen = cx
        .sub
        .frames
        .iter()
        .find(|f| f.record == name)
        .map(|f| f.codon_start);
    let effective = chosen.or(detected);
    let code = cx.code.id;
    match (chosen, candidates.as_slice()) {
        (None, []) => issues.push(Issue::local(
            Severity::Blocking,
            "no_clean_frame",
            format!("No reading frame is free of stop codons under genetic code {code}; check the sequence, the genetic code and the batch's marker type"),
        )),
        (None, [one]) => issues.push(Issue::local(
            Severity::Blocking,
            "frame_unconfirmed",
            format!("Detected reading frame codon_start {one}; confirm or change it"),
        )),
        (None, several) => issues.push(Issue::local(
            Severity::Blocking,
            "frame_ambiguous",
            format!(
                "Frames {} are all free of stop codons; choose one",
                several.iter().map(u8::to_string).collect::<Vec<_>>().join(" and ")
            ),
        )),
        (Some(_), _) => {}
    }
    let (translation, internal_stops) = effective.map_or((String::new(), Vec::new()), |f| {
        let offset = usize::from(f - 1).min(seq.len());
        (
            String::from_utf8_lossy(&cx.code.translate(&seq[offset..])).into_owned(),
            stops(cx.code, seq, f),
        )
    });
    if let (Some(f), false) = (chosen, internal_stops.is_empty()) {
        issues.push(Issue::local(
            Severity::Blocking,
            "internal_stop",
            format!(
                "Internal stop codons at codon {} in the chosen frame (codon_start {f}, genetic code {code})",
                internal_stops.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    FrameReport {
        candidates,
        detected,
        chosen,
        effective,
        translation,
        internal_stops,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::metadata::{
        Cell, CellChange, ChangeSet, Identification, MetadataEdit, PartialDate, RecordLink, Taxon,
    };
    use basecraft_protocol::submission::{Author, BatchSettings, SourceAlignment};

    fn meta() -> ProjectMetadata {
        let mut meta = ProjectMetadata::default();
        let text = |s: &str, c: &str, v: &str| CellChange {
            specimen: s.into(),
            column: c.into(),
            value: Some(Cell::Text(v.into())),
        };
        let mut cells = Vec::new();
        for s in ["HV1", "HV2"] {
            cells.push(text(s, "catalogNumber", s));
            cells.push(text(s, "country", "Australia"));
            cells.push(CellChange {
                specimen: s.into(),
                column: "eventDate".into(),
                value: Some(Cell::Date(PartialDate::parse("2019-05").unwrap())),
            });
        }
        crate::metadata::apply(
            &mut meta,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    columns: crate::metadata::KNOWN_COLUMNS
                        .iter()
                        .map(crate::metadata::KnownColumn::column)
                        .collect(),
                    specimens: vec!["HV1".into(), "HV2".into()],
                    cells,
                    taxa: vec![
                        Taxon {
                            id: "t1".into(),
                            name: "Caulerpa taxifolia".into(),
                            submitted_name: None,
                            taxid: None,
                        },
                        Taxon {
                            id: "t2".into(),
                            name: "Caulerpa sp. 3".into(),
                            submitted_name: Some("Caulerpa sp.".into()),
                            taxid: None,
                        },
                    ],
                    identifications: vec![
                        Identification {
                            id: "i1".into(),
                            specimen: "HV1".into(),
                            taxon: "t1".into(),
                            identified_by: None,
                            date: None,
                            remarks: None,
                        },
                        Identification {
                            id: "i2".into(),
                            specimen: "HV2".into(),
                            taxon: "t2".into(),
                            identified_by: None,
                            date: None,
                            remarks: None,
                        },
                    ],
                    links: ["HV1", "HV2"]
                        .iter()
                        .map(|s| RecordLink {
                            document: "aln.fasta".into(),
                            record: format!("{s}_rbcL"),
                            specimen: (*s).into(),
                        })
                        .collect(),
                },
            },
        )
        .unwrap();
        meta
    }

    fn submission(marker: Marker, code: u8) -> Submission {
        Submission {
            source: Some(SourceAlignment {
                document: "aln.fasta".into(),
                hash: "b3".into(),
            }),
            batch: BatchSettings {
                marker,
                genetic_code: code,
                ..BatchSettings::default()
            },
            publication: basecraft_protocol::submission::Publication {
                title: "Caulerpa markers".into(),
                authors: vec![Author {
                    first: "Jane".into(),
                    last: "Doe".into(),
                    middle: String::new(),
                }],
            },
            ..Submission::default()
        }
    }

    fn rows(seqs: &[(&str, &str)]) -> Vec<SourceRow> {
        seqs.iter()
            .map(|(n, s)| SourceRow {
                name: (*n).into(),
                residues: s.as_bytes().to_vec(),
            })
            .collect()
    }

    fn codes(p: &Prepared, record: &str) -> Vec<String> {
        p.issues
            .iter()
            .filter(|i| i.record.as_deref() == Some(record))
            .map(|i| i.code.clone())
            .collect()
    }

    #[test]
    fn its_note_lists_components_in_order() {
        assert_eq!(
            its_note(&[
                ItsComponent::Its2,
                ItsComponent::Its1,
                ItsComponent::Rrna58S
            ]),
            "contains internal transcribed spacer 1, 5.8S ribosomal RNA, and internal transcribed spacer 2"
        );
        assert_eq!(
            its_note(&[ItsComponent::Its1]),
            "contains internal transcribed spacer 1"
        );
        assert_eq!(
            its_note(&[ItsComponent::Rrna58S, ItsComponent::Its1]),
            "contains internal transcribed spacer 1 and 5.8S ribosomal RNA"
        );
    }

    #[test]
    fn export_ids_are_safe_short_and_unique() {
        let mut taken = BTreeSet::new();
        let a = export_id("Caulerpa taxifolia|HV1234|rbcL partial", &taken);
        assert_eq!(a, "Caulerpa_taxifolia_HV1234");
        taken.insert(a);
        let b = export_id("Caulerpa taxifolia|HV1234|rbcL other", &taken);
        assert_eq!(b, "Caulerpa_taxifolia_HV12_2");
        assert!(b.len() <= MAX_EXPORT_ID);
        assert_eq!(export_id(".x", &BTreeSet::new()), "S.x");
    }

    #[test]
    fn accessions_are_checked() {
        for ok in ["U12345", "MZ123456", "MZ123456.1", "OQ12345678.2"] {
            assert!(valid_accession(ok), "{ok}");
        }
        for bad in [
            "",
            "mz123456",
            "MZ1234",
            "MZ123456.",
            "ABC12345",
            "MZ123456.x",
        ] {
            assert!(!valid_accession(bad), "{bad}");
        }
    }

    #[test]
    fn extraction_ungaps_keeps_ambiguity_trims_and_maps_columns() {
        let sub = submission(
            Marker::Rrna {
                product: "18S ribosomal RNA".into(),
            },
            1,
        );
        // Columns 0, 1, 6 and 12 are masked (N, ? or a gap in every row).
        let p = prepare(
            &sub,
            &rows(&[
                ("HV1_rbcL", "NN-ACGNRT--A?CA"),
                ("HV2_rbcL", "--GACG-ATTAA-CA"),
            ]),
            &meta(),
            "aln.fasta",
        );
        let r = &p.records[0];
        assert_eq!(r.residues, "ACGNRTANCA");
        assert_eq!(r.trimmed, (2, 0));
        assert_eq!(
            r.segments,
            vec![
                Segment {
                    export_start: 0,
                    export_end: 6,
                    column_start: 3
                },
                Segment {
                    export_start: 6,
                    export_end: 10,
                    column_start: 11
                },
            ]
        );
        let c = codes(&p, "HV1_rbcL");
        for code in [
            "trimmed_unknown",
            "internal_unknown",
            "ambiguity_codes",
            "masked_columns",
        ] {
            assert!(c.contains(&code.to_string()), "{code}: {c:?}");
        }
        assert_eq!(p.blocking(), 0, "{:?}", p.issues);
        let f = r.feature.as_ref().unwrap();
        assert_eq!(
            (f.kind.as_str(), f.partial5, f.partial3),
            ("rRNA", true, true)
        );
        // Unidentified to species: the voucher names the organism.
        assert_eq!(p.records[1].organism.as_deref(), Some("Caulerpa sp. HV2"));
        assert_eq!(
            p.records[1].research_name.as_deref(),
            Some("Caulerpa sp. 3")
        );
        assert_eq!(p.records[0].organism.as_deref(), Some("Caulerpa taxifolia"));
        assert_eq!(p.records[0].research_name, None);
    }

    #[test]
    fn untrimmed_ends_duplicates_unlinked_and_missing_metadata_are_flagged() {
        let mut sub = submission(
            Marker::Its {
                components: vec![ItsComponent::Its1],
            },
            1,
        );
        sub.export.trim_terminal_unknown = false;
        let mut m = meta();
        crate::metadata::apply(
            &mut m,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    cells: vec![CellChange {
                        specimen: "HV2".into(),
                        column: "country".into(),
                        value: None,
                    }],
                    ..ChangeSet::default()
                },
            },
        )
        .unwrap();
        let p = prepare(
            &sub,
            &rows(&[
                ("HV1_rbcL", "NACGTN"),
                ("HV2_rbcL", "ACGT"),
                ("x", "ACGT"),
                ("x", "ACGT"),
            ]),
            &m,
            "aln.fasta",
        );
        assert_eq!(p.records[0].residues, "NACGTN");
        assert!(codes(&p, "HV1_rbcL").contains(&"terminal_unknown".into()));
        assert!(!codes(&p, "HV1_rbcL").contains(&"internal_unknown".into()));
        assert!(codes(&p, "HV2_rbcL").contains(&"missing_source".into()));
        let x = codes(&p, "x");
        assert!(x.contains(&"duplicate_record".into()) && x.contains(&"unlinked".into()));
        assert_eq!(p.records[2].export_id.as_deref(), Some("x"));
        assert_eq!(p.records[3].export_id.as_deref(), Some("x_2"));
        assert_eq!(
            p.records[0].feature.as_ref().unwrap().qualifiers,
            vec![(
                "note".into(),
                "contains internal transcribed spacer 1".into()
            )]
        );
    }

    #[test]
    fn decisions_exclude_and_keep_existing_accessions() {
        let mut sub = submission(
            Marker::Rrna {
                product: "18S ribosomal RNA".into(),
            },
            1,
        );
        apply(
            &mut sub,
            &SubmissionEdit::SetRows {
                rows: vec![
                    RowDecision {
                        record: "HV1_rbcL".into(),
                        decision: Decision::Existing {
                            accession: "MZ123456.1".into(),
                        },
                    },
                    RowDecision {
                        record: "HV2_rbcL".into(),
                        decision: Decision::Exclude,
                    },
                ],
            },
        )
        .unwrap();
        let p = prepare(
            &sub,
            &rows(&[("HV1_rbcL", "ACGT"), ("HV2_rbcL", "ACGT")]),
            &meta(),
            "aln.fasta",
        );
        assert!(p.exported().next().is_none());
        assert_eq!(p.records[0].accession.as_deref(), Some("MZ123456.1"));
        assert!(p.issues.iter().any(|i| i.code == "no_records"));
        assert!(
            apply(
                &mut sub,
                &SubmissionEdit::SetRows {
                    rows: vec![RowDecision {
                        record: "HV1_rbcL".into(),
                        decision: Decision::Existing {
                            accession: "nope".into()
                        }
                    }],
                },
            )
            .is_err()
        );
        assert_eq!(sub.rows.len(), 2);
    }

    // rbcL from Caulerpa (plastid, code 11), starting in frame 2.
    const RBCL: &str = "AGGTGTTGGATTTAAAGCTGGTGTTAAAGATTATAGATTAACTTATTATACTCCAGAATATAAAACTAAAGATACTGATATTTTAGCAGCATTTCGTATGACACCTCAACCTGGTGTACCTGCAGAAGAAGCTGGTGCTGCAGTAGCTGCTGAATCTTCTACAGGTACATGGACAACTGTATGGACAGATGGTTTAACAAGTTTAGATCGTTATAAAGGTCGTTGCTATGATATTGAACCTGTAGCTGGTGAAGAAAATCAATATATTGCATATGTTGCTTATCCTTTAGATTTATTTGAAGAAGGT";

    #[test]
    fn coding_frames_are_detected_confirmed_and_checked() {
        let product = "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit";
        let mut sub = submission(
            Marker::Coding {
                gene: "rbcL".into(),
                product: product.into(),
            },
            11,
        );
        let p = prepare(&sub, &rows(&[("HV1_rbcL", RBCL)]), &meta(), "aln.fasta");
        let frame = p.records[0].frame.as_ref().unwrap();
        assert_eq!(frame.candidates, vec![2]);
        assert_eq!(frame.detected, Some(2));
        assert!(
            frame
                .translation
                .starts_with("GVGFKAGVKDYRLTYYTPEYKTKDTDILAAFRMTPQPG"),
            "{}",
            frame.translation
        );
        assert!(codes(&p, "HV1_rbcL").contains(&"frame_unconfirmed".into()));
        let f = p.records[0].feature.as_ref().unwrap();
        assert_eq!(
            (f.partial5, f.partial3, f.gene.as_deref()),
            (true, true, Some("rbcL"))
        );
        assert!(f.qualifiers.contains(&("codon_start".into(), "2".into())));
        // Confirming the frame clears the review issue.
        apply(
            &mut sub,
            &SubmissionEdit::SetFrames {
                frames: vec![FrameChoice {
                    record: "HV1_rbcL".into(),
                    codon_start: 2,
                }],
                clear: vec![],
            },
        )
        .unwrap();
        let p = prepare(&sub, &rows(&[("HV1_rbcL", RBCL)]), &meta(), "aln.fasta");
        assert_eq!(p.blocking(), 0, "{:?}", p.issues);
        // A wrong override shows its internal stops.
        apply(
            &mut sub,
            &SubmissionEdit::SetFrames {
                frames: vec![FrameChoice {
                    record: "HV1_rbcL".into(),
                    codon_start: 1,
                }],
                clear: vec![],
            },
        )
        .unwrap();
        let p = prepare(&sub, &rows(&[("HV1_rbcL", RBCL)]), &meta(), "aln.fasta");
        assert!(codes(&p, "HV1_rbcL").contains(&"internal_stop".into()));
        assert_ne!(
            p.records[0].frame.as_ref().unwrap().internal_stops,
            Vec::<usize>::new()
        );
    }

    #[test]
    fn complete_coding_ends_and_codes_matter() {
        // ATG ... TGA: TGA stops in the standard code, codes tryptophan in code 5.
        let seq = "ATGAAATGGTGAAAATAA";
        let sub = submission(
            Marker::Coding {
                gene: String::new(),
                product: "x".into(),
            },
            1,
        );
        let p = prepare(&sub, &rows(&[("HV1_rbcL", seq)]), &meta(), "aln.fasta");
        let frame = p.records[0].frame.as_ref().unwrap();
        assert!(!frame.candidates.contains(&1), "{frame:?}");
        let sub5 = submission(
            Marker::Coding {
                gene: String::new(),
                product: "x".into(),
            },
            5,
        );
        let p = prepare(&sub5, &rows(&[("HV1_rbcL", seq)]), &meta(), "aln.fasta");
        let frame = p.records[0].frame.as_ref().unwrap();
        assert!(frame.candidates.contains(&1), "{frame:?}");
        let mut sub5 = sub5;
        sub5.frames.push(FrameChoice {
            record: "HV1_rbcL".into(),
            codon_start: 1,
        });
        let p = prepare(&sub5, &rows(&[("HV1_rbcL", seq)]), &meta(), "aln.fasta");
        let f = p.records[0].feature.as_ref().unwrap();
        assert_eq!((f.partial5, f.partial3), (false, false));
        assert_eq!(p.records[0].frame.as_ref().unwrap().translation, "MKWWK*");
    }
}
