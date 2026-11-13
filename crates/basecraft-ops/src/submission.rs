//! GenBank marker submissions: the submission document, review against the
//! source alignment and project metadata, the submission package, table2asn
//! validation results and returned accessions.
//!
//! A submission is a project document (`*.submission.json`) edited with
//! `submission.edit` operations, like the delimitation workspace. Exporting
//! writes a package folder; table2asn runs on it as the registered
//! operation `basecraft.table2asn/validate@1` through the shared runner,
//! and [`record_validation`] reads its reports back into the package.
//! Everything here is local preparation: a clean table2asn report is not
//! archive acceptance.
use basecraft_protocol::submission_records::{
    PackageDocument, PackageIssues, PackageRecord, PackageTools, SubmissionPackage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use basecraft_docs::submission::{
    self as model, Issue, Prepared, PreparedRecord, Severity, SourceRow,
};
use basecraft_io::ncbi;
use basecraft_platform::settings::Submitter;
use basecraft_protocol::Op;
use basecraft_protocol::metadata::ProjectMetadata;
use basecraft_protocol::submission::{
    AccessionEntry, Decision, Decision as RowChoice, InclusionPlan, InclusionRule, Marker,
    SUBMISSION_VERSION, SourceAlignment, Submission, SubmissionEdit,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::invalid;
use crate::{DocId, Result, Session};

/// Submission documents are named `<name>.submission.json`.
pub const SUFFIX: &str = ".submission.json";
/// The registered table2asn validation operation.
pub const VALIDATE_OPERATION: &str = "basecraft.table2asn/validate@1";
const PACKAGE_SCHEMA: &str = "basecraft.genbank-submission-package";
const BASE: &str = "submission";
const DISCLAIMER: &str = "Local preparation and validation only. A clean table2asn report shows that these files pass that tool's checks in this version; it is not acceptance by GenBank, whose curators review every submission.";

/// Whether `path` names a submission document.
pub fn is_submission(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.len() > SUFFIX.len() && n.ends_with(SUFFIX))
}

/// Read a submission document.
pub fn load(path: &Path) -> Result<Submission> {
    load_from(path, basecraft_io::Source::open(path)?.bytes())
}

pub(crate) fn load_from(path: &Path, bytes: &[u8]) -> Result<Submission> {
    let sub: Submission = serde_json::from_slice(bytes).map_err(|e| crate::Error::Damaged {
        path: path.to_path_buf(),
        detail: format!("not a submission: {e}"),
    })?;
    if sub.version != SUBMISSION_VERSION {
        return Err(crate::Error::Unsupported(format!(
            "submission format version {} is not supported",
            sub.version
        )));
    }
    model::validate(&sub).map_err(|e| invalid(e.0))?;
    Ok(sub)
}

/// The bytes a submission document is saved as.
pub fn encode(sub: &Submission) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(sub)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The source reference for an alignment file inside the project: its
/// project-relative path and current content hash.
pub fn source_for(project: &Path, alignment: &Path) -> Result<SourceAlignment> {
    let document = crate::metadata::document_key(project, alignment)?;
    let hash = basecraft_io::hash_file(&project.join(&document))?;
    Ok(SourceAlignment { document, hash })
}

/// Write a new submission document for `alignment` at `path` and open it.
/// Refuses to replace an existing file.
pub fn create(session: &mut Session, path: &Path, alignment: &Path) -> Result<DocId> {
    if !is_submission(path) {
        return Err(invalid(format!("submission documents are named *{SUFFIX}")));
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        session.root().join(path)
    };
    if path.exists() {
        return Err(invalid(format!(
            "{} already exists; open it instead",
            path.display()
        )));
    }
    let source = source_for(session.root(), alignment)?;
    // Check the alignment reads before writing anything.
    read_rows(&session.root().join(&source.document))?;
    let sub = Submission {
        source: Some(source),
        ..Submission::default()
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = encode(&sub)?;
    basecraft_io::write_atomic(&path, |w| w.write_all(&bytes))?;
    session.open(&path)
}

/// Apply one submission edit as an undoable operation.
pub fn edit(session: &mut Session, doc: DocId, edit: SubmissionEdit) -> Result<()> {
    session.apply(
        doc,
        Op::SubmissionEdit {
            edit: Box::new(edit),
        },
    )?;
    Ok(())
}

/// History text for an edit.
pub fn describe(edit: &SubmissionEdit) -> String {
    match edit {
        SubmissionEdit::SetAccessionPolicy { .. } => "Submission accession evidence source".into(),
        SubmissionEdit::SetSource { source } => format!("Submission source {}", source.document),
        SubmissionEdit::RenameRecord { from, to } => format!("Submission row {from} renamed {to}"),
        SubmissionEdit::SetRows { rows } => format!("Submission decisions for {} rows", rows.len()),
        SubmissionEdit::ConfirmSelection { .. } => "Confirmed submission row selection".into(),
        SubmissionEdit::SetBatch { .. } => "Submission batch settings".into(),
        SubmissionEdit::SetFrames { frames, clear } => format!(
            "Reading frames: {} set, {} cleared",
            frames.len(),
            clear.len()
        ),
        SubmissionEdit::SetExport { .. } => "Submission export settings".into(),
        SubmissionEdit::SetPublication { .. } | SubmissionEdit::SetDetails { .. } => {
            "Submission authors and title".into()
        }
        SubmissionEdit::ApplyInclusion { plan } => format!(
            "Applied inclusion rule {} to {} rows",
            plan.rule.name, plan.changed
        ),
        SubmissionEdit::RecordExport { .. } => "Recorded submission export identities".into(),
        SubmissionEdit::SetAccessions { accessions } => {
            format!("Recorded {} accessions", accessions.len())
        }
    }
}

/// Preview is read-only. Fingerprint binds the complete target content, source and metadata;
/// it is portable across CLI sessions, but rejects even unrelated target edits.
pub fn plan_inclusion(
    session: &Session,
    doc: DocId,
    rule: &InclusionRule,
) -> Result<InclusionPlan> {
    plan_inclusion_submission(session.root(), checked(session, doc)?, rule)
}

pub fn plan_inclusion_submission(
    project: &Path,
    sub: &Submission,
    rule: &InclusionRule,
) -> Result<InclusionPlan> {
    let source = sub
        .source
        .as_ref()
        .ok_or_else(|| invalid("Choose a source alignment"))?;
    let hash = basecraft_io::hash_file(&project.join(&source.document))?;
    if hash != source.hash {
        return Err(crate::Error::ChangedOnDisk {
            path: project.join(&source.document),
        });
    }
    let metadata_stamp = metadata_evidence(project)?;
    let (meta, metadata_hash) = project_metadata(project)?;
    let rows = read_rows(&project.join(&source.document))?;
    // Detect replacement between hashing and reading; immutable byte input is cached by GUI review separately.
    if basecraft_io::hash_file(&project.join(&source.document))? != hash {
        return Err(crate::Error::ChangedOnDisk {
            path: project.join(&source.document),
        });
    }
    if metadata_evidence(project)? != metadata_stamp {
        return Err(crate::Error::ChangedOnDisk {
            path: crate::metadata::manifest_path(project),
        });
    }
    let records: Vec<_> = rows
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                model::selection(sub, &r.name, &meta, &source.document).0,
            )
        })
        .collect();
    let rows = model::inclusion_rows(sub, rule, &records).map_err(|e| invalid(e.0))?;
    Ok(InclusionPlan {
        fingerprint: basecraft_io::hash_bytes(&serde_json::to_vec(&(
            "submission-inclusion-v1",
            sub,
            hash,
            metadata_hash,
            metadata_stamp,
        ))?),
        matching: rows.iter().filter(|r| r.matched == Some(true)).count(),
        unresolved: rows.iter().filter(|r| r.matched.is_none()).count(),
        preserved: rows.iter().filter(|r| r.preserved_manual).count(),
        changed: rows.iter().filter(|r| r.proposed != r.current).count(),
        included: rows.iter().filter(|r| r.proposed == RowChoice::New).count(),
        rule: rule.clone(),
        rows,
    })
}

/// Called at the Session boundary, including generic GUI/script/MCP apply routes.
/// Recompute the entire reviewed result so edited/forged row proposals are rejected too.
pub(crate) fn verify_inclusion(
    project: &Path,
    sub: &Submission,
    plan: &InclusionPlan,
) -> Result<()> {
    let current = plan_inclusion_submission(project, sub, &plan.rule)?;
    if &current != plan {
        return Err(invalid(
            "Stale inclusion preview: target, source or metadata evidence changed; preview again",
        ));
    }
    Ok(())
}

/// Build the existing owner's typed import-settings edit; GUI and headless saves share it.
pub fn inclusion_preset_edit(
    meta: &ProjectMetadata,
    rule: InclusionRule,
) -> Result<basecraft_protocol::metadata::MetadataEdit> {
    if rule.name.trim().is_empty() {
        return Err(invalid("Give the preset a name"));
    }
    model::inclusion_rows(&Submission::default(), &rule, &[]).map_err(|e| invalid(e.0))?;
    let mut settings = meta.import.clone();
    settings.inclusion_rules.retain(|r| r.name != rule.name);
    settings.inclusion_rules.push(rule);
    Ok(basecraft_protocol::metadata::MetadataEdit::SetImportSettings { settings })
}

/// Remember a rule using the project's existing import-preset owner.
pub fn save_inclusion_preset(session: &mut Session, rule: InclusionRule) -> Result<()> {
    // Validate before creating any metadata document.
    inclusion_preset_edit(&ProjectMetadata::default(), rule.clone())?;
    let doc =
        crate::metadata::open(session, true)?.ok_or_else(|| invalid("No metadata document"))?;
    let edit = inclusion_preset_edit(session.metadata(doc)?, rule)?;
    session.apply(
        doc,
        Op::MetadataEdit {
            edit: Box::new(edit),
        },
    )?;
    session.save(doc, None)?;
    Ok(())
}

fn checked(session: &Session, doc: DocId) -> Result<&Submission> {
    session.submission(doc)
}

/// Read every record of an aligned FASTA file as rows.
pub fn read_rows(path: &Path) -> Result<Vec<SourceRow>> {
    let bytes = std::fs::read(path)?;
    let index = basecraft_io::fasta::FastaIndex::build(&bytes)?;
    if index.records.is_empty() {
        return Err(crate::Error::Empty(path.to_path_buf()));
    }
    index
        .records
        .iter()
        .map(|r| {
            Ok(SourceRow {
                name: r.name.clone(),
                residues: r.read(&bytes, 0, r.len)?,
            })
        })
        .collect()
}

/// Exact input evidence includes externally edited TSV tables, not only the manifest.
fn metadata_evidence(project: &Path) -> Result<Option<String>> {
    let manifest = crate::metadata::manifest_path(project);
    if !manifest.exists() {
        return Ok(None);
    }
    let hashes = crate::metadata::files(&manifest)
        .iter()
        .map(|path| {
            basecraft_io::hash_file(path).map(|hash| {
                (
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    hash,
                )
            })
        })
        .collect::<basecraft_io::Result<Vec<_>>>()?;
    Ok(Some(basecraft_io::hash_bytes(&serde_json::to_vec(&(
        "submission-metadata-inputs-v1",
        hashes,
    ))?)))
}

/// The project metadata as saved, and its manifest's hash; empty metadata
/// when the project has none.
pub fn project_metadata(project: &Path) -> Result<(ProjectMetadata, Option<String>)> {
    let manifest = crate::metadata::manifest_path(project);
    if !manifest.is_file() {
        return Ok((ProjectMetadata::default(), None));
    }
    let before = metadata_evidence(project)?;
    let manifest_hash = basecraft_io::hash_file(&manifest)?;
    let opened = crate::metadata::load(&manifest)?;
    if metadata_evidence(project)? != before {
        return Err(crate::Error::ChangedOnDisk {
            path: manifest.clone(),
        });
    }
    Ok((opened.metadata, Some(manifest_hash)))
}

/// The submitter preference, from the user's settings.
pub fn submitter_preference() -> Result<Submitter> {
    let paths =
        basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
    basecraft_platform::settings::SettingsStore::new(paths.preferences())
        .load()
        .map(|p| p.submitter)
        .map_err(|e| invalid(e.to_string()))
}

/// A review of a submission against its source alignment, the project
/// metadata and the submitter preference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    #[serde(flatten)]
    pub prepared: Prepared,
    /// The source alignment's current hash, when it can be read.
    pub source_hash: Option<String>,
    /// Hash of the metadata manifest used.
    pub metadata_hash: Option<String>,
    /// Pass this fingerprint to the `ConfirmSelection` edit after review.
    pub selection_hash: Option<String>,
    pub selection_confirmed: bool,
}

/// Review a submission: extraction, specimen data, features, frames and
/// every local issue. A changed source alignment prepares nothing until
/// it is selected again.
pub fn review(session: &Session, doc: DocId, submitter: &Submitter) -> Result<Review> {
    let sub = checked(session, doc)?;
    review_submission(session.root(), sub, submitter)
}

/// Immutable parsed inputs, reused only after exact content hashes match.
#[derive(Default)]
pub struct ReviewCache {
    source: Option<(SourceAlignment, std::sync::Arc<Vec<SourceRow>>)>,
    metadata: Option<(Option<String>, ProjectMetadata)>,
    prepared: Option<(String, Prepared)>,
}

/// [`review`] for a submission held outside a session.
pub fn review_submission(
    project: &Path,
    sub: &Submission,
    submitter: &Submitter,
) -> Result<Review> {
    review_submission_cached(project, sub, submitter, &mut ReviewCache::default())
}

pub fn review_submission_cached(
    project: &Path,
    sub: &Submission,
    submitter: &Submitter,
    cache: &mut ReviewCache,
) -> Result<Review> {
    review_submission_cached_with_stage(project, sub, submitter, cache, |_| {})
}

/// Same cached review with factual, optional stage reporting; no progress percentages.
pub fn review_submission_cached_with_stage(
    project: &Path,
    sub: &Submission,
    submitter: &Submitter,
    cache: &mut ReviewCache,
    mut stage: impl FnMut(&str),
) -> Result<Review> {
    stage("Reading source sequences and specimen metadata");
    let manifest = crate::metadata::manifest_path(project);
    let metadata_stamp = metadata_evidence(project)?;
    let metadata_hash = if manifest.exists() {
        Some(basecraft_io::hash_file(&manifest)?)
    } else {
        None
    };
    if cache
        .metadata
        .as_ref()
        .is_none_or(|(hash, _)| hash != &metadata_stamp)
    {
        let (metadata, read_hash) = project_metadata(project)?;
        if read_hash != metadata_hash || metadata_evidence(project)? != metadata_stamp {
            return Err(crate::Error::ChangedOnDisk {
                path: manifest.clone(),
            });
        }
        cache.metadata = Some((metadata_stamp.clone(), metadata));
    }
    let meta = cache
        .metadata
        .as_ref()
        .map(|(_, metadata)| metadata)
        .ok_or_else(|| invalid("No metadata snapshot"))?;
    let mut issues = Vec::new();
    let mut source_hash = None;
    let mut rows = std::sync::Arc::new(Vec::new());
    if let Some(source) = &sub.source {
        let path = project.join(&source.document);
        match basecraft_io::hash_file(&path) {
            Ok(hash) if hash == source.hash => {
                if cache
                    .source
                    .as_ref()
                    .is_none_or(|(cached, _)| cached != source)
                {
                    let parsed = read_rows(&path)?;
                    if basecraft_io::hash_file(&path)? != hash {
                        return Err(crate::Error::ChangedOnDisk { path: path.clone() });
                    }
                    cache.source = Some((source.clone(), std::sync::Arc::new(parsed)));
                }
                if let Some((_, parsed)) = &cache.source {
                    rows = parsed.clone();
                }
                source_hash = Some(hash);
            }
            Ok(hash) => {
                source_hash = Some(hash);
                issues.push(Issue {
                    severity: Severity::Blocking,
                    source: "basecraft".into(),
                    code: "source_changed".into(),
                    message: format!("{} changed since it was chosen; choose it again to prepare the submission from its current content", source.document),
                    record: None,
                    specimen: None,
                    tool_severity: None,
                });
            }
            Err(e) => {
                issues.push(local(
                    Severity::Blocking,
                    "source_unreadable",
                    format!("{} can't be read: {e}", source.document),
                ));
            }
        }
    }
    let document = sub.source.as_ref().map_or("", |s| s.document.as_str());
    let mut sequence_settings = sub.clone();
    sequence_settings.publication = basecraft_protocol::submission::Publication::default();
    sequence_settings.submission_details = None;
    sequence_settings.publication_differs = false;
    sequence_settings.selection_confirmation = None;
    sequence_settings.exported.clear();
    sequence_settings.marker_drafts.clear();
    let prepared_key = basecraft_io::hash_bytes(&serde_json::to_vec(&(
        &sequence_settings,
        &source_hash,
        &metadata_stamp,
    ))?);
    let mut prepared = if let Some((key, prepared)) = &cache.prepared
        && key == &prepared_key
    {
        stage("Reusing sequence annotations and reading frames");
        prepared.clone()
    } else {
        stage("Preparing sequence annotations and reading frames");
        let prepared = model::prepare(sub, &rows, meta, document);
        cache.prepared = Some((prepared_key, prepared.clone()));
        prepared
    };
    stage("Checking submission details and contact");
    model::citation_issues(sub, &mut prepared.issues);
    for missing in submitter.missing() {
        prepared.issues.push(local(
            Severity::Blocking,
            "submitter",
            format!(
                "Submitter {} is not set (Data submission > Submitter)",
                missing.replace('_', " ")
            ),
        ));
    }
    ascii_issues(sub, &prepared, submitter, &mut issues);
    issues.append(&mut prepared.issues);
    prepared.issues = issues;
    // Bind confirmation to the exact source plus effective selection and
    // evidence, not unrelated batch/contact changes. Version the hash input.
    let selection_hash = if sub
        .source
        .as_ref()
        .is_some_and(|s| Some(&s.hash) == source_hash.as_ref())
        && !rows.is_empty()
    {
        let selection: Vec<_> = prepared
            .records
            .iter()
            .map(|r| {
                (
                    &r.record,
                    &r.decision,
                    &r.selection_reason,
                    &r.accession_candidates,
                )
            })
            .collect();
        Some(basecraft_io::hash_bytes(&serde_json::to_vec(&(
            "submission-selection-v1",
            &sub.source,
            selection,
        ))?))
    } else {
        None
    };
    let selection_confirmed =
        selection_hash.is_some() && selection_hash == sub.selection_confirmation;
    if !selection_confirmed {
        prepared.issues.push(local(
            Severity::Blocking,
            "selection_unconfirmed",
            "Review the included and excluded rows, then confirm the selection before export",
        ));
    }
    Ok(Review {
        prepared,
        source_hash,
        metadata_hash,
        selection_hash,
        selection_confirmed,
    })
}

fn local(severity: Severity, code: &str, message: impl Into<String>) -> Issue {
    Issue {
        severity,
        source: "basecraft".into(),
        code: code.into(),
        message: message.into(),
        record: None,
        specimen: None,
        tool_severity: None,
    }
}

/// NCBI submission files are ASCII. Non-ASCII text is flagged, never
/// transliterated silently.
fn ascii_issues(
    sub: &Submission,
    prepared: &Prepared,
    submitter: &Submitter,
    issues: &mut Vec<Issue>,
) {
    let mut flag = |what: String, text: &str, record: Option<&PreparedRecord>| {
        if !text.is_ascii() {
            issues.push(Issue {
                record: record.map(|r| r.record.clone()),
                specimen: record.and_then(|r| r.specimen.clone()),
                ..local(
                    Severity::Blocking,
                    "non_ascii",
                    format!("{what} {text:?} has non-ASCII characters; NCBI files are ASCII. Enter an ASCII spelling"),
                )
            });
        }
    };
    for (name, value) in submitter.fields() {
        flag(format!("Submitter {}", name.replace('_', " ")), value, None);
    }
    for citation in [sub.submission_citation(), sub.publication_citation()] {
        flag("Title".into(), &citation.title, None);
        for author in &citation.authors {
            flag(
                "Author".into(),
                &format!("{} {} {}", author.first, author.middle, author.last),
                None,
            );
        }
    }
    for record in prepared.exported() {
        if let Some(organism) = &record.organism {
            flag("Organism".into(), organism, Some(record));
        }
        for (name, value) in &record.qualifiers {
            flag(name.clone(), value, Some(record));
        }
        if let Some(feature) = &record.feature {
            for (name, value) in &feature.qualifiers {
                flag(name.clone(), value, Some(record));
            }
        }
    }
}

// The package.

pub use basecraft_protocol::submission_records::PackageSummary;

fn tsv(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn table(header: &[&str], rows: &[Vec<String>]) -> Vec<u8> {
    let mut out = header.join("\t");
    out.push('\n');
    for row in rows {
        out.push_str(&row.iter().map(|v| tsv(v)).collect::<Vec<_>>().join("\t"));
        out.push('\n');
    }
    out.into_bytes()
}

fn decision_text(decision: &Decision) -> &'static str {
    match decision {
        Decision::New => "new",
        Decision::Existing { .. } => "existing",
        Decision::Exclude => "exclude",
    }
}

fn contact(submitter: &Submitter) -> ncbi::Contact {
    ncbi::Contact {
        first: submitter.first_name.clone(),
        middle: submitter.middle_initials.clone(),
        last: submitter.last_name.clone(),
        email: submitter.email.clone(),
        phone: submitter.phone.clone(),
        institution: submitter.institution.clone(),
        department: submitter.department.clone(),
        street: submitter.street.clone(),
        city: submitter.city.clone(),
        state: submitter.state.clone(),
        postal_code: submitter.postal_code.clone(),
        country: submitter.country.clone(),
    }
}

/// The table2asn input files, in order: FASTA, source table, feature
/// table, template.
fn genbank_files(
    sub: &Submission,
    prepared: &Prepared,
    submitter: &Submitter,
) -> Result<[Vec<u8>; 4]> {
    let records: Vec<&PreparedRecord> = prepared.exported().collect();
    let mut modifiers = vec![(
        "mol_type".to_string(),
        sub.batch.mol_type.as_str().to_string(),
    )];
    if let Some(organelle) = sub.batch.organelle {
        modifiers.push(("location".into(), organelle.as_str().into()));
    }
    if let (Marker::Coding { .. }, Some(code)) = (&sub.batch.marker, sub.batch.genetic_code) {
        let name = if sub
            .batch
            .organelle
            .is_some_and(basecraft_protocol::submission::Organelle::mitochondrial)
        {
            "mgcode"
        } else {
            "gcode"
        };
        modifiers.push((name.into(), code.to_string()));
    }
    let mut fsa = Vec::new();
    ncbi::write_fsa(
        &mut fsa,
        &records
            .iter()
            .map(|r| ncbi::FsaRecord {
                id: r.export_id.as_deref().unwrap_or_default(),
                modifiers: &modifiers,
                residues: r.residues.as_bytes(),
            })
            .collect::<Vec<_>>(),
    )?;
    let sources: Vec<Vec<(String, String)>> = records
        .iter()
        .map(|r| {
            let mut q = Vec::new();
            if let Some(organism) = &r.organism {
                q.push(("organism".to_string(), organism.clone()));
            }
            q.extend(r.qualifiers.iter().cloned());
            q
        })
        .collect();
    let mut src = Vec::new();
    ncbi::write_src(
        &mut src,
        &records
            .iter()
            .zip(&sources)
            .map(|(r, q)| (r.export_id.as_deref().unwrap_or_default(), q.as_slice()))
            .collect::<Vec<_>>(),
    )?;
    let gene_qualifiers: Vec<Vec<(String, String)>> = records
        .iter()
        .map(|r| {
            r.feature
                .as_ref()
                .and_then(|f| f.gene.clone())
                .map(|g| vec![("gene".to_string(), g)])
                .unwrap_or_default()
        })
        .collect();
    let mut tbl_records = Vec::new();
    for (r, gene) in records.iter().zip(&gene_qualifiers) {
        let mut features = Vec::new();
        if let Some(f) = &r.feature {
            let end = r.residues.len() as u64;
            if !gene.is_empty() {
                features.push(ncbi::TableFeature {
                    kind: "gene",
                    start: 1,
                    end,
                    partial5: f.partial5,
                    partial3: f.partial3,
                    qualifiers: gene,
                });
            }
            features.push(ncbi::TableFeature {
                kind: &f.kind,
                start: 1,
                end,
                partial5: f.partial5,
                partial3: f.partial3,
                qualifiers: &f.qualifiers,
            });
        }
        tbl_records.push((r.export_id.as_deref().unwrap_or_default(), features));
    }
    let mut tbl = Vec::new();
    ncbi::write_tbl(&mut tbl, &tbl_records)?;
    let persons = |citation: &basecraft_protocol::submission::Publication| {
        citation
            .authors
            .iter()
            .map(ncbi::Person::from)
            .collect::<Vec<_>>()
    };
    let mut sbt = Vec::new();
    ncbi::write_sbt_details(
        &mut sbt,
        &contact(submitter),
        &persons(sub.submission_citation()),
        &sub.submission_citation().title,
        &persons(sub.publication_citation()),
        &sub.publication_citation().title,
    )?;
    Ok([fsa, src, tbl, sbt])
}

const GENBANK_NAMES: [&str; 4] = ["fsa", "src", "tbl", "sbt"];

fn input_hash(files: &[Vec<u8>; 4]) -> String {
    let mut hasher = basecraft_io::Hasher::new();
    for (name, bytes) in GENBANK_NAMES.iter().zip(files) {
        hasher.update(name.as_bytes());
        hasher.update(basecraft_io::hash_bytes(bytes).as_bytes());
    }
    hasher.finish()
}

/// The folder a validation run of `input_hash` keeps its reports in.
pub fn validation_dir(package: &Path, input_hash: &str) -> PathBuf {
    let short: String = input_hash
        .trim_start_matches("b3:")
        .chars()
        .take(16)
        .collect();
    package.join("validation").join(short)
}

fn records_table(prepared: &Prepared) -> Vec<u8> {
    let rows: Vec<Vec<String>> = prepared
        .records
        .iter()
        .map(|r| {
            let f = r.feature.as_ref();
            vec![
                r.export_id.clone().unwrap_or_default(),
                r.record.clone(),
                (r.row + 1).to_string(),
                decision_text(&r.decision).into(),
                r.accession.clone().unwrap_or_default(),
                r.specimen.clone().unwrap_or_default(),
                r.organism.clone().unwrap_or_default(),
                r.research_name.clone().unwrap_or_default(),
                if r.export_id.is_some() {
                    r.residues.len().to_string()
                } else {
                    String::new()
                },
                f.map(|f| f.kind.clone()).unwrap_or_default(),
                f.map(|f| if f.partial5 { "partial" } else { "complete" }.to_string())
                    .unwrap_or_default(),
                f.map(|f| if f.partial3 { "partial" } else { "complete" }.to_string())
                    .unwrap_or_default(),
                r.frame
                    .as_ref()
                    .and_then(|x| x.effective)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
                if r.export_id.is_some() {
                    r.trimmed.0.to_string()
                } else {
                    String::new()
                },
                if r.export_id.is_some() {
                    r.trimmed.1.to_string()
                } else {
                    String::new()
                },
            ]
        })
        .collect();
    table(
        &[
            "export_id",
            "record",
            "row",
            "decision",
            "accession",
            "specimen_id",
            "organism",
            "research_name",
            "length",
            "feature",
            "five_prime",
            "three_prime",
            "codon_start",
            "trimmed_5",
            "trimmed_3",
        ],
        &rows,
    )
}

fn specimens_table(prepared: &Prepared) -> Vec<u8> {
    let mut columns: Vec<String> = Vec::new();
    for r in prepared.exported() {
        for (q, _) in &r.qualifiers {
            if !columns.contains(q) {
                columns.push(q.clone());
            }
        }
    }
    let rows: Vec<Vec<String>> = prepared
        .exported()
        .map(|r| {
            let mut row = vec![
                r.export_id.clone().unwrap_or_default(),
                r.specimen.clone().unwrap_or_default(),
                r.organism.clone().unwrap_or_default(),
                r.research_name.clone().unwrap_or_default(),
            ];
            for c in &columns {
                row.push(
                    r.qualifiers
                        .iter()
                        .find(|(q, _)| q == c)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default(),
                );
            }
            row
        })
        .collect();
    let mut header = vec!["export_id", "specimen_id", "organism", "research_name"];
    header.extend(columns.iter().map(String::as_str));
    table(&header, &rows)
}

fn coordinate_table(prepared: &Prepared) -> Vec<u8> {
    let mut rows = Vec::new();
    for r in prepared.exported() {
        for s in &r.segments {
            rows.push(vec![
                r.export_id.clone().unwrap_or_default(),
                r.record.clone(),
                (r.row + 1).to_string(),
                (s.export_start + 1).to_string(),
                s.export_end.to_string(),
                (s.column_start + 1).to_string(),
                (s.column_start + s.export_end - s.export_start).to_string(),
            ]);
        }
    }
    table(
        &[
            "export_id",
            "record",
            "row",
            "export_start",
            "export_end",
            "alignment_start",
            "alignment_end",
        ],
        &rows,
    )
}

/// Issues as a table, one per line.
pub fn issues_table(issues: &[Issue]) -> Vec<u8> {
    let rows: Vec<Vec<String>> = issues
        .iter()
        .map(|i| {
            vec![
                i.severity.as_str().into(),
                i.source.clone(),
                i.tool_severity.clone().unwrap_or_default(),
                i.code.clone(),
                i.record.clone().unwrap_or_default(),
                i.specimen.clone().unwrap_or_default(),
                i.message.clone(),
            ]
        })
        .collect();
    table(
        &[
            "severity",
            "source",
            "tool_severity",
            "code",
            "record",
            "specimen_id",
            "message",
        ],
        &rows,
    )
}

fn count(issues: &[Issue], severity: Severity) -> usize {
    issues.iter().filter(|i| i.severity == severity).count()
}

fn readme(
    summary: &PackageSummary,
    sub: &Submission,
    issues: &[Issue],
    validation: Option<&ValidationSummary>,
) -> Vec<u8> {
    let mut text = String::new();
    text.push_str("GenBank marker submission package (Basecraft)\n\n");
    text.push_str(DISCLAIMER);
    text.push_str("\n\n");
    let _ = writeln!(text, "Status: {}", summary.status);
    let _ = writeln!(text, "Records: {}", summary.records);
    if let Some(source) = &sub.source {
        let _ = writeln!(
            text,
            "Source alignment: {} ({})",
            source.document, source.hash
        );
    }
    let _ = writeln!(text, "Input hash: {}\n", summary.input_hash);
    text.push_str("Files\n");
    text.push_str("  genbank/submission.fsa  sequences with batch source modifiers\n");
    text.push_str("  genbank/submission.src  source qualifiers per record\n");
    text.push_str("  genbank/submission.tbl  five-column feature table\n");
    text.push_str("  genbank/submission.sbt  submitter, authors and title\n");
    text.push_str("  genbank/submission.sqn  table2asn output to send (after validation)\n");
    text.push_str("  genbank/submission.gbf  GenBank flatfile preview (after validation)\n");
    text.push_str("  records.tsv, specimens.tsv, coordinate-map.tsv  records, their specimens and alignment columns\n");
    text.push_str("  validation/  local issues, and table2asn reports per input hash\n\n");
    match validation {
        Some(v) => {
            let _ = writeln!(
                text,
                "Validation: table2asn {} ({}): {} errors, {} warnings. Reports in {}\n",
                v.tool_version, v.status, v.errors, v.warnings, v.directory
            );
        }
        None => text.push_str("Validation: not yet run for these exact files. Run table2asn validation in Basecraft (Data submission > Validate) or `basecraft submission validate`.\n\n"),
    }
    text.push_str("Handoff\n");
    text.push_str(
        "  1. Resolve the actions below and validate again until table2asn reports no errors.\n",
    );
    text.push_str(
        "  2. Read genbank/submission.gbf to check the records as GenBank will show them.\n",
    );
    text.push_str("  3. Submit genbank/submission.sqn as NCBI currently instructs for table2asn files (by email to gb-sub@ncbi.nlm.nih.gov at the time of writing; check NCBI's table2asn page first).\n");
    text.push_str("  4. Import the accession list NCBI returns (Data submission > Accessions or `basecraft submission accessions`).\n\n");
    let open: Vec<&Issue> = issues
        .iter()
        .filter(|i| matches!(i.severity, Severity::Blocking | Severity::Review))
        .collect();
    let _ = writeln!(text, "Unresolved actions ({})", open.len());
    for issue in open {
        let _ = writeln!(
            text,
            "  [{}] {}{}",
            issue.severity.as_str(),
            issue
                .record
                .as_ref()
                .map(|r| format!("{r}: "))
                .unwrap_or_default(),
            issue.message
        );
    }
    text.into_bytes()
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    basecraft_io::write_atomic(path, |w| w.write_all(bytes))?;
    Ok(basecraft_io::hash_bytes(bytes))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Write the submission package to `directory` (inside the project).
/// Regenerating with the same inputs writes identical files. The package
/// records the validation run for its exact input hash, if there is one;
/// an `.sqn` or `.gbf` from other inputs is removed.
pub fn export(
    session: &mut Session,
    doc: DocId,
    directory: &Path,
    submitter: &Submitter,
) -> Result<PackageSummary> {
    let sub = checked(session, doc)?.clone();
    // Its records would be read in no frame and exported with no code.
    if matches!(sub.batch.marker, Marker::Coding { .. }) && sub.batch.genetic_code.is_none() {
        return Err(invalid(
            "Choose the genetic code for this coding marker before export",
        ));
    }
    let directory = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        session.root().join(directory)
    };
    let root = session.root().canonicalize()?;
    let review = review_submission(&root, &sub, submitter)?;
    if !review.selection_confirmed {
        return Err(invalid(
            "review and confirm the current row selection before export",
        ));
    }
    std::fs::create_dir_all(&directory)?;
    let directory = directory.canonicalize()?;
    if !directory.starts_with(&root) {
        return Err(invalid("the package folder must be inside the project"));
    }
    let manifest = directory.join("package.json");
    if directory.read_dir()?.next().is_some() && !manifest.is_file() {
        return Err(invalid(format!(
            "{} has other files; choose an empty folder or an earlier package",
            directory.display()
        )));
    }
    if manifest.is_file() {
        let existing: Value =
            serde_json::from_slice(&std::fs::read(&manifest)?).unwrap_or(Value::Null);
        if existing["schema"] != PACKAGE_SCHEMA {
            return Err(invalid(format!(
                "{} is not a Basecraft submission package",
                directory.display()
            )));
        }
    }
    let genbank = genbank_files(&sub, &review.prepared, submitter)?;
    let hash = input_hash(&genbank);
    let mut files = BTreeMap::new();
    for (ext, bytes) in GENBANK_NAMES.iter().zip(&genbank) {
        let path = directory.join("genbank").join(format!("{BASE}.{ext}"));
        files.insert(relative(&directory, &path), write_file(&path, bytes)?);
    }
    for (name, bytes) in [
        ("records.tsv", records_table(&review.prepared)),
        ("specimens.tsv", specimens_table(&review.prepared)),
        ("coordinate-map.tsv", coordinate_table(&review.prepared)),
        (
            "validation/local-issues.tsv",
            issues_table(&review.prepared.issues),
        ),
    ] {
        files.insert(name.into(), write_file(&directory.join(name), &bytes)?);
    }
    let validation = read_validation(&directory, &hash)?;
    for ext in ["sqn", "gbf"] {
        let path = directory.join("genbank").join(format!("{BASE}.{ext}"));
        let current = validation.as_ref().and_then(|v| v.copies.get(ext));
        match current {
            Some(expected) if basecraft_io::hash_file(&path).ok().as_ref() == Some(expected) => {
                files.insert(relative(&directory, &path), expected.clone());
            }
            _ if path.exists() => std::fs::remove_file(&path)?,
            _ => {}
        }
    }
    let blocking = count(&review.prepared.issues, Severity::Blocking);
    let status = status(blocking, validation.as_ref());
    let summary = PackageSummary {
        directory: relative(&root, &directory),
        input_hash: hash.clone(),
        records: review.prepared.exported().count(),
        blocking,
        review: count(&review.prepared.issues, Severity::Review),
        status,
        files: files.clone(),
    };
    files.insert(
        "README.txt".into(),
        write_file(
            &directory.join("README.txt"),
            &readme(&summary, &sub, &review.prepared.issues, validation.as_ref()),
        )?,
    );
    let package = serde_json::to_value(SubmissionPackage {
        schema: PACKAGE_SCHEMA.into(),
        version: 1,
        destination: "genbank".into(),
        note: DISCLAIMER.into(),
        status: summary.status.clone(),
        submission: PackageDocument {
            document: session.doc_key(doc)?.into(),
            hash: basecraft_io::hash_bytes(&encode(&sub)?),
        },
        source: sub.source.clone(),
        metadata_manifest_hash: review.metadata_hash.clone(),
        batch: sub.batch.clone(),
        export: sub.export.clone(),
        input_hash: hash,
        records: review
            .prepared
            .exported()
            .map(|r| PackageRecord {
                export_id: r.export_id.clone(),
                record: r.record.clone(),
                row: r.row + 1,
                specimen: r.specimen.clone(),
            })
            .collect(),
        issues: PackageIssues {
            blocking: summary.blocking,
            review: summary.review,
        },
        files,
        tools: PackageTools {
            table2asn: validation.as_ref().map(|v| v.tool_version.clone()),
        },
        validation,
    })?;
    let mut bytes = serde_json::to_vec_pretty(&package)?;
    bytes.push(b'\n');
    session.export(doc, &manifest, "genbank-submission-package", |w| {
        w.write_all(&bytes)
    })?;
    edit(
        session,
        doc,
        SubmissionEdit::RecordExport {
            mapping: basecraft_protocol::submission::ExportMapping {
                source: sub.source.clone(),
                records: review
                    .prepared
                    .exported()
                    .filter_map(|r| Some((r.export_id.clone()?, r.record.clone())))
                    .collect(),
            },
        },
    )?;
    session.save(doc, None).map_err(|error| invalid(format!("package written but export identities were not saved: {error}; keep this document open and retry saving before importing accessions")))?;
    Ok(summary)
}

fn status(blocking: usize, validation: Option<&ValidationSummary>) -> String {
    match (blocking, validation) {
        (0, Some(v)) if v.status == "passed" => "validated",
        (_, Some(v)) if v.status == "errors" => "validation_errors",
        (0, _) => "prepared",
        _ => "draft",
    }
    .into()
}

// Validation.

pub use basecraft_protocol::submission_records::ValidationSummary;

fn read_validation(package: &Path, input_hash: &str) -> Result<Option<ValidationSummary>> {
    let path = validation_dir(package, input_hash).join("summary.json");
    if !path.is_file() {
        return Ok(None);
    }
    let summary: ValidationSummary = serde_json::from_slice(&std::fs::read(&path)?)?;
    Ok((summary.input_hash == input_hash).then_some(summary))
}

/// The package's recorded input hash, checked against its current files.
fn package_inputs(package: &Path) -> Result<(Value, String)> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(package.join("package.json"))?)?;
    if manifest["schema"] != PACKAGE_SCHEMA {
        return Err(invalid("not a Basecraft submission package"));
    }
    let hash = manifest["input_hash"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let mut files: [Vec<u8>; 4] = Default::default();
    for (slot, ext) in files.iter_mut().zip(GENBANK_NAMES) {
        *slot = std::fs::read(package.join("genbank").join(format!("{BASE}.{ext}")))?;
    }
    if input_hash(&files) != hash {
        return Err(invalid(
            "the package's genbank files changed since export; export the package again",
        ));
    }
    Ok((manifest, hash))
}

/// The operation request that validates a package with table2asn, with a
/// new report folder for its input hash. Paths are project-relative.
pub fn validation_request(project: &Path, package: &Path) -> Result<Value> {
    let root = project.canonicalize()?;
    let package = if package.is_absolute() {
        package.to_path_buf()
    } else {
        root.join(package)
    }
    .canonicalize()?;
    let (_, hash) = package_inputs(&package)?;
    let dir = validation_dir(&package, &hash);
    std::fs::create_dir_all(&dir)?;
    let destination = (1..=999)
        .map(|n| {
            dir.join(if n == 1 {
                "table2asn".to_string()
            } else {
                format!("table2asn-{n}")
            })
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join("table2asn"));
    let input = |ext: &str| json!([{"kind": "file", "path": relative(&root, &package.join("genbank").join(format!("{BASE}.{ext}")))}]);
    Ok(json!({
        "operation": VALIDATE_OPERATION,
        "inputs": {"sequences": input("fsa"), "sources": input("src"), "features": input("tbl"), "template": input("sbt")},
        "parameters": {},
        "destinations": {"reports": relative(&root, &destination)},
    }))
}

// The ops layer cannot depend on plugin-host. Consume its existing execution,
// input ledgers and publication verifier here, without introducing a run store.
fn verified_reports(
    root: &Path,
    package: &Path,
    hash: &str,
    outcome: &Value,
) -> Result<Option<PathBuf>> {
    let read = |path: &Path| -> Result<Value> {
        Ok(serde_json::from_slice(
            &crate::records::provenance::read_record(path)?,
        )?)
    };
    let run = Path::new(
        outcome["evidence"]
            .as_str()
            .ok_or_else(|| invalid("validation has no registered run evidence"))?,
    );
    let run_id = outcome["run_id"].as_str().unwrap_or_default();
    if run_id.is_empty()
        || run.file_name().and_then(|n| n.to_str()) != Some(run_id)
        || outcome["operation"] != VALIDATE_OPERATION
        || read(&run.join("runtime.json"))? != *outcome
    {
        return Err(invalid(
            "validation outcome differs from its registered run",
        ));
    }
    let execution: basecraft_protocol::records::RunRecord =
        serde_json::from_value(read(&run.join("execution.json"))?)?;
    if execution.schema != basecraft_protocol::records::RUN_RECORD_SCHEMA
        || execution.operation != VALIDATE_OPERATION
        || outcome["pins"]["package_digest"] != execution.package_digest.as_deref().unwrap_or("")
        || outcome["pins"]["descriptor_digest"]
            != execution.descriptor_digest.as_deref().unwrap_or("")
        || outcome["effective_parameters"] != execution.effective_parameters
        || execution.inputs.len() != 4
    {
        return Err(invalid("validation execution identity does not match"));
    }
    let request = read(&run.join("request.json"))?;
    for (role, ext) in [
        ("sequences", "fsa"),
        ("sources", "src"),
        ("features", "tbl"),
        ("template", "sbt"),
    ] {
        let current = package.join("genbank").join(format!("{BASE}.{ext}"));
        let evidence = execution
            .inputs
            .get(role)
            .filter(|e| e.len() == 1)
            .ok_or_else(|| invalid("validation has incomplete submitted input evidence"))?;
        let evidence = &evidence[0];
        let expected = basecraft_io::hash_file(&current)?;
        let original = evidence
            .original_path
            .as_deref()
            .ok_or_else(|| invalid("validation input has no source path"))?;
        let ledger =
            crate::records::input_evidence::read_input(&run.join(&evidence.run_directory))?;
        if run.join(original).canonicalize()? != current.canonicalize()?
            || evidence.source_hash.as_deref() != Some(expected.as_str())
            || evidence.submitted_hash != expected
            || evidence.transformation != "identity-v1"
            || ledger.state == basecraft_protocol::input_records::EvidenceState::Broken
            || ledger.submitted_hash != expected
            || ledger.transformation != "identity-v1"
        {
            return Err(invalid(
                "stale or unrelated validation inputs; validate the exported package again",
            ));
        }
    }
    let destination = request["destinations"]["reports"]
        .as_str()
        .ok_or_else(|| invalid("validation has no reports destination"))?;
    let destination = root.join(destination);
    if destination.parent().and_then(|p| p.canonicalize().ok())
        != Some(validation_dir(package, hash).canonicalize()?)
    {
        return Err(invalid(
            "validation belongs to a different package generation",
        ));
    }
    let artifacts = outcome["artifacts"]
        .as_array()
        .ok_or_else(|| invalid("missing validation artifacts"))?;
    if artifacts.is_empty() {
        return Ok(None);
    }
    let reports = destination.canonicalize()?;
    for artifact in artifacts {
        let path = root.join(
            artifact["path"]
                .as_str()
                .ok_or_else(|| invalid("missing validation artifact path"))?,
        );
        if artifact["role"] != "reports"
            || basecraft_io::hash_file(&path)? != artifact["digest"].as_str().unwrap_or_default()
            || crate::records::provenance::bundle_for_result(&path, None).canonicalize()?
                != run.canonicalize()?
        {
            return Err(invalid(
                "validation artifact or publication evidence differs",
            ));
        }
        // The submission's folder is the project the caller chose.
        let project = crate::project::Project::new(root)?;
        let issues = crate::records::provenance::verify(&path, Some(&project))?;
        if !issues.is_empty() {
            return Err(crate::Error::Damaged {
                path: path.clone(),
                detail: format!(
                    "validation publication verification failed: {}",
                    issues.join("; ")
                ),
            });
        }
    }
    // Only read files actually published by this run, including optional reports.
    for name in [
        "table2asn.log",
        "submission.val",
        "submission.sqn",
        "submission.gbf",
    ] {
        let path = reports.join(name);
        if path.exists()
            && !artifacts.iter().any(|a| {
                a["path"]
                    .as_str()
                    .is_some_and(|p| root.join(p).canonicalize().ok() == path.canonicalize().ok())
            })
        {
            return Err(invalid("validation folder contains an unpublished output"));
        }
    }
    Ok(Some(reports))
}

/// Read a finished table2asn job's reports into the package: issues mapped
/// to records and specimens, the summary, the `.sqn` and `.gbf` copied
/// into `genbank/`, and the package manifest and README updated.
/// `outcome` is the operation outcome as JSON. An unavailable tool or a
/// failed run is recorded as such, never as a sequence error.
pub fn record_validation(
    project: &Path,
    package: &Path,
    outcome: &Value,
) -> Result<ValidationSummary> {
    let root = project.canonicalize()?;
    let package = if package.is_absolute() {
        package.to_path_buf()
    } else {
        root.join(package)
    }
    .canonicalize()?;
    let (mut manifest, hash) = package_inputs(&package)?;
    let dir = validation_dir(&package, &hash);
    let data = &outcome["data"];
    let reports = verified_reports(&root, &package, &hash, outcome)?;
    let mut summary = ValidationSummary {
        input_hash: hash.clone(),
        status: "failed".into(),
        tool_version: data["version"].as_str().unwrap_or("unknown").into(),
        run_id: outcome["run_id"].as_str().unwrap_or_default().into(),
        directory: relative(&package, &dir),
        errors: 0,
        warnings: 0,
        infos: 0,
        message: String::new(),
        copies: BTreeMap::new(),
        issues: Vec::new(),
    };
    let state = outcome["state"].as_str().unwrap_or_default();
    if data["status"] == "unavailable" {
        summary.status = "unavailable".into();
        summary.message = data["message"]
            .as_str()
            .unwrap_or("table2asn is not available")
            .into();
    } else if state != "completed" {
        summary.message = outcome["errors"][0]["message"]
            .as_str()
            .unwrap_or("the validation job did not complete")
            .into();
    } else if let Some(reports) = reports {
        summary.directory = relative(&package, &reports);
        let val = reports.join(format!("{BASE}.val"));
        let sqn = reports.join(format!("{BASE}.sqn"));
        let gbf = reports.join(format!("{BASE}.gbf"));
        // table2asn writes no .val when it has nothing to report. A run
        // counts only when it exited cleanly and wrote the .sqn and the
        // flatfile preview, which it makes after validating.
        let quiet = !val.is_file() && data["exit_code"] == 0 && gbf.is_file();
        if !val.is_file() && !quiet {
            summary.message =
                "table2asn produced no validation report or .sqn; the run is not a pass".into();
        } else {
            let ids: BTreeMap<String, (String, Option<String>)> = manifest["records"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    Some((
                        r["export_id"].as_str()?.to_string(),
                        (
                            r["record"].as_str()?.to_string(),
                            r["specimen"].as_str().map(str::to_string),
                        ),
                    ))
                })
                .collect();
            let report = if quiet {
                String::new()
            } else {
                std::fs::read_to_string(&val)?
            };
            for message in ncbi::parse_val(&report) {
                let severity = match message.severity.as_str() {
                    "Error" | "Reject" | "Fatal" => {
                        summary.errors += 1;
                        Severity::Blocking
                    }
                    "Warning" => {
                        summary.warnings += 1;
                        Severity::Review
                    }
                    _ => {
                        summary.infos += 1;
                        Severity::Info
                    }
                };
                let target = message.seq_id.as_ref().and_then(|id| ids.get(id));
                summary.issues.push(Issue {
                    severity,
                    source: "table2asn".into(),
                    code: message.code,
                    message: message.message,
                    record: target.map(|t| t.0.clone()),
                    specimen: target.and_then(|t| t.1.clone()),
                    tool_severity: Some(message.severity),
                });
            }
            let complete = data["exit_code"] == 0
                && outcome["errors"].as_array().is_none_or(Vec::is_empty)
                && sqn.is_file()
                && gbf.is_file()
                && report
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .is_none_or(|line| {
                        ["Error: ", "Reject: ", "Fatal: ", "Warning: ", "Info: "]
                            .iter()
                            .any(|prefix| line.starts_with(prefix))
                    });
            summary.status = if !complete {
                "failed"
            } else if summary.errors == 0 {
                "passed"
            } else {
                "errors"
            }
            .into();
            summary.message = format!(
                "table2asn {}: {} errors, {} warnings",
                summary.tool_version, summary.errors, summary.warnings
            );
            if !complete {
                summary.message = format!(
                    "table2asn did not exit successfully with complete recognized outputs (exit {}); retain reports and retry validation",
                    data["exit_code"]
                );
            }
            for ext in ["sqn", "gbf"].into_iter().filter(|_| complete) {
                let from = reports.join(format!("{BASE}.{ext}"));
                if from.is_file() {
                    let bytes = std::fs::read(&from)?;
                    let to = package.join("genbank").join(format!("{BASE}.{ext}"));
                    summary.copies.insert(ext.into(), write_file(&to, &bytes)?);
                }
            }
        }
    } else {
        summary.message = "the validation job published no reports".into();
    }
    if summary.status == "unavailable" {
        summary.issues.push(local(
            Severity::Unavailable,
            "table2asn_unavailable",
            summary.message.clone(),
        ));
    } else if summary.status == "failed" {
        summary.issues.push(local(
            Severity::Unavailable,
            "table2asn_failed",
            summary.message.clone(),
        ));
    }
    std::fs::create_dir_all(&dir)?;
    write_file(&dir.join("issues.tsv"), &issues_table(&summary.issues))?;
    let mut bytes = serde_json::to_vec_pretty(&summary)?;
    bytes.push(b'\n');
    write_file(&dir.join("summary.json"), &bytes)?;
    // The manifest and README now describe this run.
    let blocking =
        usize::try_from(manifest["issues"]["blocking"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    manifest["status"] = json!(status(blocking, Some(&summary)));
    manifest["validation"] = serde_json::to_value(&summary)?;
    manifest["tools"]["table2asn"] = json!(summary.tool_version);
    for (ext, hash) in &summary.copies {
        manifest["files"][format!("genbank/{BASE}.{ext}")] = json!(hash);
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    write_file(&package.join("package.json"), &bytes)?;
    let readme_path = package.join("README.txt");
    if let Ok(text) = std::fs::read_to_string(&readme_path) {
        let line = text
            .lines()
            .find(|l| l.starts_with("Validation: "))
            .unwrap_or_default()
            .to_string();
        let status_line = text
            .lines()
            .find(|l| l.starts_with("Status: "))
            .unwrap_or_default()
            .to_string();
        let updated = text
            .replacen(
                &line,
                &format!(
                    "Validation: table2asn {} ({}): {} errors, {} warnings. Reports in {}",
                    summary.tool_version,
                    summary.status,
                    summary.errors,
                    summary.warnings,
                    summary.directory
                ),
                1,
            )
            .replacen(
                &status_line,
                &format!(
                    "Status: {}",
                    manifest["status"].as_str().unwrap_or_default()
                ),
                1,
            );
        write_file(&readme_path, updated.as_bytes())?;
    }
    Ok(summary)
}

/// The validation recorded for a package's current input hash, if any.
pub fn current_validation(project: &Path, package: &Path) -> Result<Option<ValidationSummary>> {
    let package = if package.is_absolute() {
        package.to_path_buf()
    } else {
        project.join(package)
    };
    if !package.join("package.json").is_file() {
        return Ok(None);
    }
    match package_inputs(&package) {
        Ok((_, hash)) => read_validation(&package, &hash),
        Err(_) => Ok(None),
    }
}

// Accessions.

/// Accessions read from the archive's list, matched to records.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccessionPlan {
    pub accessions: Vec<AccessionEntry>,
    /// Lines that name no record of this submission, or no accession.
    pub unmatched: Vec<String>,
}

/// Read an accession list as NCBI returns it: lines holding a sequence ID
/// (the export ID or the record name) and an accession, separated by tabs,
/// spaces or commas. Header and blank lines are skipped.
pub fn plan_accessions(
    session: &Session,
    doc: DocId,
    text: &str,
    _submitter: &Submitter,
) -> Result<AccessionPlan> {
    let sub = checked(session, doc)?;
    let mut ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for mapping in sub.exported.iter().filter(|m| m.source == sub.source) {
        for (id, record) in &mapping.records {
            ids.entry(id.clone()).or_default().insert(record.clone());
        }
    }
    if ids.is_empty() {
        return Err(invalid(
            "export this submission before importing returned accessions",
        ));
    }
    let originals: BTreeSet<&String> = ids.values().flatten().collect();
    let mut plan = AccessionPlan::default();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let fields: Vec<&str> = line
            .split(|c: char| c == '\t' || c == ',' || c.is_whitespace())
            .filter(|f| !f.is_empty())
            .collect();
        if fields.is_empty() {
            continue;
        }
        // Archive SeqIDs take precedence over original-name fallback. Never
        // combine the namespaces in a map: original names can equal another ID.
        let exported: BTreeSet<&String> = fields
            .iter()
            .filter_map(|f| ids.get(*f))
            .flatten()
            .collect();
        let named: BTreeSet<&String> = fields
            .iter()
            .filter_map(|f| originals.iter().find(|r| r.as_str() == *f).copied())
            .collect();
        let candidates = if exported.is_empty() {
            &named
        } else {
            &exported
        };
        // Assign field roles before interpreting accession-shaped text: an
        // explicitly included submitted ID can itself look like an accession.
        let identity_fields: BTreeSet<usize> = fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                let matches = if exported.is_empty() {
                    originals.iter().any(|name| name.as_str() == *field)
                } else {
                    ids.contains_key(*field)
                };
                matches.then_some(index)
            })
            .collect();
        let record = (identity_fields.len() == 1 && candidates.len() == 1)
            .then(|| candidates.first().copied())
            .flatten();
        let returned: Vec<&str> = fields
            .iter()
            .enumerate()
            .filter(|(index, field)| {
                !identity_fields.contains(index) && model::valid_accession(field)
            })
            .map(|(_, field)| *field)
            .collect();
        let accession = (returned.len() == 1).then(|| returned[0]);
        match (record, accession) {
            (Some(record), Some(accession)) if seen.insert(record.clone()) => {
                plan.accessions.push(AccessionEntry {
                    record: record.clone(),
                    accession: accession.to_string(),
                });
            }
            (None, None)
                if identity_fields.is_empty()
                    && returned.is_empty()
                    && plan.accessions.is_empty()
                    && plan.unmatched.is_empty() => {}
            _ => plan.unmatched.push(line.trim().to_string()),
        }
    }
    Ok(plan)
}

/// A publication table of accessions: one row per record with one.
pub fn accession_table(session: &Session, doc: DocId, submitter: &Submitter) -> Result<Vec<u8>> {
    let review = review(session, doc, submitter)?;
    let (meta, _) = project_metadata(session.root())?;
    let rows: Vec<Vec<String>> = review
        .prepared
        .records
        .iter()
        .filter_map(|r| {
            let accession = r.accession.clone()?;
            let organism = r.organism.clone().or_else(|| {
                r.specimen
                    .as_ref()
                    .and_then(|s| basecraft_docs::metadata::insdc_source(&meta, s).ok())
                    .and_then(|q| {
                        q.qualifiers
                            .into_iter()
                            .find(|(k, _)| k == "organism")
                            .map(|(_, v)| v)
                    })
            });
            Some(vec![
                r.record.clone(),
                r.specimen.clone().unwrap_or_default(),
                organism.unwrap_or_default(),
                r.export_id.clone().unwrap_or_default(),
                decision_text(&r.decision).into(),
                accession,
            ])
        })
        .collect();
    Ok(table(
        &[
            "record",
            "specimen_id",
            "organism",
            "export_id",
            "decision",
            "accession",
        ],
        &rows,
    ))
}
