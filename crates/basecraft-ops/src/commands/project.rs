//! Commands on the project: its specimen metadata, GenBank marker
//! submissions and species workspaces. Each needs the project.
use std::collections::{BTreeMap, BTreeSet};

use basecraft_protocol::metadata::{FieldTarget, HeaderPattern, MetadataEdit, SpreadsheetMapping};
use basecraft_protocol::submission::{FrameChoice, SubmissionEdit};
use serde_json::{Value, json};

use super::Kind::{File, Files, Flag, Json, List, NewFile, Text, Texts};
use super::{Arg, Call, Command, Routes::All, arg, document};
use crate::metadata::{self, HeaderImport, ImportOptions, SheetImport};
use crate::{DocId, Error, Result, delimitation, submission};

const fn file(help: &'static str) -> Arg {
    Arg::new("path", File, help).positional()
}

/// An argument given in order on the command line.
const fn given(name: &'static str, kind: super::Kind, help: &'static str) -> Arg {
    Arg::new(name, kind, help).positional()
}

const OBJECT: &str = r#"{"type":"object"}"#;

#[rustfmt::skip]
pub(super) const COMMANDS: &[Command] = &[
    Command { name: "metadata init", writes: true, routes: All, run: metadata_init,
        help: "Create empty specimen metadata tables in the project's `metadata/` folder.",
        args: &[] },
    Command { name: "metadata show", writes: false, routes: All, run: metadata_show,
        help: "The project's specimen metadata: roster, taxa, identifications and links, with notices from reading it.",
        args: &[] },
    Command { name: "metadata import-headers", writes: false, routes: All, run: import_headers,
        help: "Plan an import from structured FASTA headers, or with apply, apply the plan as one undoable edit.",
        args: &[file("The FASTA file, inside the project."), arg("separator", Text, "The field separator, such as `|`."),
            arg("fields", List, "What each field holds: specimen, taxon, record, ignore (or -), or a column name such as eventDate."),
            arg("save_pattern", Text, "Remember the pattern under this name.").optional(),
            IMPORT_NEW, IMPORT_OVERWRITE, IMPORT_APPLY] },
    Command { name: "metadata import-sheet", writes: false, routes: All, run: import_sheet,
        help: "Plan an import from a spreadsheet (.ods, .xlsx, .csv or .tsv), or with apply, apply the plan as one undoable edit. Returns the plan with the mapping used.",
        args: &[file("The spreadsheet."),
            arg("map", Texts, "HEADER=TARGET, repeatable. TARGET is specimen, record, taxon, ignore or a column name. Without any, the mapping is guessed from the headers.").optional(),
            arg("sequences", File, "The sequence file whose records the `record` column names.").optional(),
            arg("save_mapping", Text, "Remember the mapping under this name.").optional(),
            IMPORT_NEW, IMPORT_OVERWRITE, IMPORT_APPLY] },
    Command { name: "metadata template", writes: true, routes: All, run: metadata_template,
        help: "Write an .ods template, every cell text, pre-filled for each record of a sequence file in the project.",
        args: &[file("The sequence file."), arg("output", NewFile, "The new .ods file.")] },
    Command { name: "metadata status", writes: false, routes: All, run: metadata_status,
        help: "List a sequence file's records that are not linked to a specimen.",
        args: &[file("The sequence file.")] },
    Command { name: "metadata insdc", writes: false, routes: All, run: metadata_insdc,
        help: "Derive a specimen's INSDC source qualifiers from its Darwin Core columns and current identification, with the required ones still missing.",
        args: &[given("specimen", Text, "The specimen's roster ID.")] },
    Command { name: "metadata consumer", writes: false, routes: All, run: metadata_consumer,
        help: "Inspect project partitions or metadata labels, or plan explicit workspace roster links or result promotion. request: {action: partitions} | {action: link_records, document: project path, links: {record ID: roster ID}} | {action: labels, document, records: [scientific ID], fields: [specimen|genus|species|accession|{column:{name}}]} | {action: link_workspace, workspace: project path, links: {workspace ID: roster ID}} | {action: promote_partition, workspace, hypothesis, id}. A plan returns its target and typed operation; apply makes the edit. Missing or conflicting roster links refuse promotion.",
        args: &[given("request", Json(OBJECT), "The request."), arg("apply", Flag, "Apply the planned edit to its target.")] },
    Command { name: "submission new", writes: true, routes: All, run: submission_new,
        help: "Create a GenBank marker submission document (NAME.submission.json) for an aligned FASTA inside the project, and open it. Edit it with submission edit (see schemas/submission-edit.schema.json).",
        args: &[given("path", NewFile, "The new submission file."), arg("alignment", File, "The aligned FASTA file.")] },
    Command { name: "submission show", writes: false, routes: All, run: submission_show,
        help: "The submission document as JSON.",
        args: &[document("The submission.")] },
    Command { name: "submission review", writes: false, routes: All, run: submission_review,
        help: "Prepare a submission's records from its source alignment and the project metadata and list every local issue: extraction (ungapped, ambiguity kept, terminal unknowns, masked columns, export IDs), specimen source qualifiers, organism names, features, and reading frames detected per record (confirm them with submission frames). Accession-aware row choices and evidence are local unverified heuristics. Before export, confirm the selection with the edit {action: confirm_selection, selection_hash: <returned selection_hash>}; source, selection or evidence changes need confirming again. Blocking issues keep the package from being ready.",
        args: &[document("The submission.")] },
    Command { name: "submission edit", writes: true, routes: All, run: submission_edit,
        help: "Apply one submission.edit ({action: set_rows | set_batch | set_frames | set_export | set_publication | set_source | set_accessions | confirm_selection | apply_inclusion, ...}) as one undoable edit.",
        args: &[document("The submission."),
            given("edit", Json(OBJECT), "The edit, such as {\"action\":\"set_rows\",\"rows\":[{\"record\":\"x\",\"decision\":\"exclude\"}]}.")] },
    Command { name: "submission frames", writes: true, routes: All, run: submission_frames,
        help: "Confirm reading frames: the detected frame of every record that has exactly one, or set RECORD=CODON_START. Returns the frames.",
        args: &[document("The submission."), arg("confirm_detected", Flag, "Confirm every uniquely detected frame not yet confirmed."),
            arg("set", Texts, "RECORD=1|2|3, repeatable.").optional()] },
    Command { name: "submission export", writes: true, routes: All, run: submission_export,
        help: "Write the submission package (genbank/ .fsa .src .tbl .sbt, records, specimens, coordinate map, README, package.json) to a folder in the project. Needs the current selection confirmed. Same inputs give identical files. Returns the input hash and status.",
        args: &[document("The submission."), arg("output", NewFile, "The package folder.")] },
    Command { name: "submission validation-request", writes: true, routes: All, run: validation_request,
        help: "The OperationRequest that validates an exported package with table2asn. Run it as the operation basecraft.table2asn/validate@1, then pass its outcome to submission record-validation.",
        args: &[given("package", File, "The package folder.")] },
    Command { name: "submission record-validation", writes: true, routes: All, run: record_validation,
        help: "Read a finished table2asn run's reports into the package: issues mapped to records and specimens, status (passed, errors, unavailable, failed), .sqn and .gbf copied to genbank/. Local validation only, not archive acceptance.",
        args: &[given("package", File, "The package folder."), arg("outcome", Json(OBJECT), "The run's outcome.")] },
    Command { name: "submission accessions", writes: false, routes: All, run: submission_accessions,
        help: "Match the accession list NCBI returned (lines with a sequence ID and an accession) to the submission's records; with apply, record them as one undoable edit.",
        args: &[document("The submission."), given("list", File, "The accession list file.").optional(),
            arg("text", Text, "The accession list itself, instead of a file.").optional(),
            arg("apply", Flag, "Record the matched accessions.")] },
    Command { name: "submission accession-table", writes: true, routes: All, run: accession_table,
        help: "Write a publication table of records, specimens, organisms and accessions.",
        args: &[document("The submission."), arg("output", NewFile, "The new table file.")] },
    Command { name: "submission submitter", writes: false, routes: All, run: submitter,
        help: "The submitter contact preference used for .sbt files and the required fields still missing; with set (fields such as first_name, last_name, email, institution, city, country), update it.",
        args: &[arg("set", Json(OBJECT), "Fields to change.").optional()] },
    Command { name: "submission inclusion-preview", writes: false, routes: All, run: inclusion_preview,
        help: "Preview identifier inclusion with a typed rule (whole identifier or one-based literal field; exact, contains, starts, ends, wildcard or regex). Manual decisions are kept by default; missing or empty fields are unresolved. Apply the whole returned plan with the edit {action: apply_inclusion, plan: ...}; stale or changed plans are refused. Matching is not accession verification. Writes nothing.",
        args: &[document("The submission."), given("rule", Json(OBJECT), "The rule.")] },
    Command { name: "submission inclusion-presets", writes: false, routes: All, run: inclusion_presets,
        help: "The project's named inclusion rules; with save, first save a named rule in the shared metadata import settings.",
        args: &[arg("save", Json(OBJECT), "A named rule to save.").optional()] },
    Command { name: "delimitation linked-alignment", writes: false, routes: All, run: linked_alignment,
        help: "The species workspace's unverified alignment hint: path, a string or null, keeping an explicit empty string. Does not read or check that path.",
        args: &[document("The species workspace.")] },
    Command { name: "delimitation candidates", writes: false, routes: All, run: candidates,
        help: "Saved species hypotheses in recommendation order; writes nothing.",
        args: &[given("paths", Files, "The hypothesis files.")] },
    Command { name: "delimitation name", writes: false, routes: All, run: combined_name,
        help: "Suggest a free combined SPART filename beside the last input; writes nothing.",
        args: &[given("paths", Files, "The hypothesis files.")] },
    Command { name: "delimitation combine", writes: true, routes: All, run: combine,
        help: "Combine saved hypotheses into a new SPART or SPART XML file. Without selection all are kept; a selection maps input paths to hypothesis IDs, and inputs it leaves out keep none. An existing output or companion is refused.",
        args: &[given("paths", Files, "Two or more hypothesis files."), arg("output", NewFile, "The new file."),
            arg("selection", Json(SELECTION), "Input path to the hypothesis IDs to keep.").optional()] },
    Command { name: "delimitation display-tree", writes: true, routes: All, run: display_tree,
        help: "Attach a tree file (one tree, Newick or NEXUS) as the display tree of a species workspace; the file is kept as its evidence.",
        args: &[document("The species workspace."), arg("tree", File, "The tree file.")] },
];

const SELECTION: &str =
    r#"{"type":"object","additionalProperties":{"type":"array","items":{"type":"string"}}}"#;

const IMPORT_NEW: Arg = arg(
    "no_new_specimens",
    Flag,
    "Leave rows for specimens the roster lacks unmatched.",
);
const IMPORT_OVERWRITE: Arg = arg(
    "overwrite",
    Flag,
    "Replace existing values and links that differ.",
);
const IMPORT_APPLY: Arg = arg(
    "apply",
    Flag,
    "Apply the plan instead of only returning it.",
);

/// The project's metadata document, opened when it is not. With `create`,
/// empty tables are written first when the project has none.
fn metadata_doc(call: &mut Call, create: bool) -> Result<DocId> {
    let project = call.project()?;
    if create && !metadata::manifest_path(project.root()).is_file() {
        call.require_writes()?;
    }
    let doc = metadata::open(call.session, create)?.ok_or(Error::NoMetadata)?;
    call.edits(doc);
    Ok(doc)
}

/// A project file's project-relative name.
fn project_key(call: &Call, path: &std::path::Path) -> Result<String> {
    metadata::document_key(call.project()?.root(), path)
}

fn metadata_init(call: &mut Call) -> Result<Value> {
    let manifest = metadata::create(call.project()?.root())?;
    Ok(json!({"manifest": manifest}))
}

fn metadata_show(call: &mut Call) -> Result<Value> {
    let doc = metadata_doc(call, false)?;
    Ok(json!({"metadata": call.session.metadata(doc)?,
        "notices": call.session.metadata_notices(doc)?}))
}

/// A field or column target by its name.
fn target(name: &str) -> Result<FieldTarget> {
    Ok(match name.trim() {
        "specimen" | "specimen_id" => FieldTarget::Specimen,
        "record" => FieldTarget::Record,
        "taxon" | "organism" => FieldTarget::Taxon,
        "ignore" | "-" | "" => FieldTarget::Ignore,
        column if metadata::valid_column_name(column) => FieldTarget::Column {
            column: column.into(),
        },
        other => {
            let expected = "specimen, taxon, record, ignore or a column name";
            return Err(super::wrong(other, expected));
        }
    })
}

fn import_options(call: &Call) -> Result<ImportOptions> {
    Ok(ImportOptions {
        create_specimens: !call.flag("no_new_specimens")?,
        overwrite: call.flag("overwrite")?,
    })
}

/// Return the plan; with `apply`, apply it and the remembered settings as one
/// undo step.
fn finish(
    call: &mut Call,
    doc: DocId,
    plan: metadata::Plan,
    remember: Option<MetadataEdit>,
) -> Result<Value> {
    let value = serde_json::to_value(&plan)?;
    if !call.flag("apply")? {
        return Ok(value);
    }
    call.require_writes()?;
    call.session.begin_group();
    let applied = (|| -> Result<()> {
        if !plan.changes.is_empty() {
            metadata::apply_plan(call.session, doc, plan.changes)?;
        }
        if let Some(edit) = remember {
            let edit = Box::new(edit);
            call.session
                .apply(doc, basecraft_protocol::Op::MetadataEdit { edit })?;
        }
        Ok(())
    })();
    call.session.end_group();
    applied?;
    Ok(value)
}

fn import_headers(call: &mut Call) -> Result<Value> {
    let document = project_key(call, &call.path("path")?)?;
    let doc = metadata_doc(call, true)?;
    let save = call.optional_text("save_pattern")?.map(str::to_owned);
    let pattern = HeaderPattern {
        name: save.clone().unwrap_or_else(|| "CLI".into()),
        separator: call.text("separator")?.to_owned(),
        fields: call
            .texts("fields")?
            .iter()
            .map(|f| target(f))
            .collect::<Result<_>>()?,
    };
    let request = HeaderImport {
        document,
        pattern: pattern.clone(),
        options: import_options(call)?,
    };
    let plan = metadata::plan_header_import(call.session, doc, &request)?;
    let remember = save.map(|name| {
        let mut settings = call.session.metadata(doc).map(|m| m.import.clone())?;
        settings.header_patterns.retain(|p| p.name != name);
        settings.header_patterns.push(pattern);
        Ok::<_, Error>(MetadataEdit::SetImportSettings { settings })
    });
    let remember = remember.transpose()?;
    finish(call, doc, plan, remember)
}

fn import_sheet(call: &mut Call) -> Result<Value> {
    let path = call.path("path")?;
    let document = call
        .optional_path("sequences")?
        .map(|p| project_key(call, &p))
        .transpose()?;
    let doc = metadata_doc(call, true)?;
    let mapping = call.texts("map")?;
    let mapping = if mapping.is_empty() {
        None
    } else {
        let pairs = mapping.iter().map(|m| {
            let (header, name) = m
                .split_once('=')
                .ok_or_else(|| super::wrong("map", "HEADER=TARGET"))?;
            Ok((header.to_owned(), target(name)?))
        });
        Some(pairs.collect::<Result<BTreeMap<_, _>>>()?)
    };
    let request = SheetImport {
        path: path.to_string_lossy().into_owned(),
        mapping,
        document,
        options: import_options(call)?,
    };
    let plan = metadata::plan_sheet_import(call.session, doc, &request)?;
    let remember = match call.optional_text("save_mapping")? {
        Some(name) => {
            let mut settings = call.session.metadata(doc)?.import.clone();
            settings.spreadsheet_mappings.retain(|m| m.name != name);
            settings.spreadsheet_mappings.push(SpreadsheetMapping {
                name: name.to_owned(),
                columns: plan.mapping.clone(),
            });
            Some(MetadataEdit::SetImportSettings { settings })
        }
        None => None,
    };
    finish(call, doc, plan, remember)
}

fn metadata_template(call: &mut Call) -> Result<Value> {
    let document = project_key(call, &call.path("path")?)?;
    let output = call.path("output")?;
    let doc = metadata_doc(call, false)?;
    let records = metadata::template_for(call.session, doc, &document, &output)?;
    Ok(json!({"records": records, "output": output}))
}

fn metadata_status(call: &mut Call) -> Result<Value> {
    let document = project_key(call, &call.path("path")?)?;
    let doc = metadata_doc(call, false)?;
    Ok(serde_json::to_value(metadata::link_status(
        call.session,
        doc,
        &document,
    )?)?)
}

fn metadata_insdc(call: &mut Call) -> Result<Value> {
    let doc = metadata_doc(call, false)?;
    metadata::insdc(call.session, doc, call.text("specimen")?)
}

fn metadata_consumer(call: &mut Call) -> Result<Value> {
    let doc = metadata_doc(call, false)?;
    let request = call.required_json("request")?;
    let plan = metadata::consumer_plan(call.session, doc, &request)?;
    if !call.flag("apply")? {
        return Ok(plan);
    }
    call.require_writes()?;
    let operation = plan
        .get("operation")
        .ok_or_else(|| super::wrong("apply", "left out for a request that only reads"))?;
    let target = plan["target"]
        .as_str()
        .ok_or(Error::MissingArgument("the plan's target"))?;
    let target = if target == call.session.doc_key(doc)? {
        doc
    } else {
        let path = call.project()?.root().join(target);
        call.open(&path.canonicalize()?)?
    };
    call.session
        .apply(target, serde_json::from_value(operation.clone())?)?;
    Ok(plan)
}

fn submission_new(call: &mut Call) -> Result<Value> {
    call.project()?;
    let path = call.path("path")?;
    let alignment = call.path("alignment")?;
    let doc = submission::create(call.session, &path, &alignment)?;
    call.state(doc)
}

/// The submission argument, refusing another kind of document.
fn submission_doc(call: &mut Call) -> Result<DocId> {
    call.project()?;
    let doc = call.doc("document")?;
    call.session.submission(doc)?;
    Ok(doc)
}

fn submission_show(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    Ok(serde_json::to_value(call.session.submission(doc)?)?)
}

fn submission_review(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let submitter = submission::submitter_preference()?;
    Ok(serde_json::to_value(submission::review(
        call.session,
        doc,
        &submitter,
    )?)?)
}

fn submission_edit(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let edit: SubmissionEdit = call.required_json("edit")?;
    submission::edit(call.session, doc, edit)?;
    Ok(serde_json::to_value(call.session.submission(doc)?)?)
}

fn submission_frames(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let mut frames = Vec::new();
    if call.flag("confirm_detected")? {
        let submitter = submission::submitter_preference()?;
        let review = submission::review(call.session, doc, &submitter)?;
        for record in &review.prepared.records {
            if let Some(frame) = &record.frame
                && let (None, Some(detected)) = (frame.chosen, frame.detected)
            {
                frames.push(FrameChoice {
                    record: record.record.clone(),
                    codon_start: detected,
                });
            }
        }
    }
    for item in call.texts("set")? {
        let (record, start) = item
            .rsplit_once('=')
            .ok_or_else(|| super::wrong("set", "RECORD=CODON_START"))?;
        let codon_start = start
            .parse()
            .map_err(|_| super::wrong("set", "RECORD=1, RECORD=2 or RECORD=3"))?;
        frames.push(FrameChoice {
            record: record.into(),
            codon_start,
        });
    }
    if !frames.is_empty() {
        let clear = Vec::new();
        submission::edit(
            call.session,
            doc,
            SubmissionEdit::SetFrames { frames, clear },
        )?;
    }
    Ok(serde_json::to_value(&call.session.submission(doc)?.frames)?)
}

fn submission_export(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let output = call.path("output")?;
    let submitter = submission::submitter_preference()?;
    Ok(serde_json::to_value(submission::export(
        call.session,
        doc,
        &output,
        &submitter,
    )?)?)
}

fn validation_request(call: &mut Call) -> Result<Value> {
    submission::validation_request(call.project()?.root(), &call.path("package")?)
}

fn record_validation(call: &mut Call) -> Result<Value> {
    let outcome: Value = call.required_json("outcome")?;
    Ok(serde_json::to_value(submission::record_validation(
        call.project()?.root(),
        &call.path("package")?,
        &outcome,
    )?)?)
}

fn submission_accessions(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let text = match (call.optional_path("list")?, call.optional_text("text")?) {
        (Some(list), None) => std::fs::read_to_string(list)?,
        (None, Some(text)) => text.to_owned(),
        _ => return Err(Error::MissingArgument("list or text, one of them")),
    };
    let submitter = submission::submitter_preference()?;
    let plan = submission::plan_accessions(call.session, doc, &text, &submitter)?;
    let value = serde_json::to_value(&plan)?;
    if call.flag("apply")? && !plan.accessions.is_empty() {
        call.require_writes()?;
        let accessions = plan.accessions;
        submission::edit(
            call.session,
            doc,
            SubmissionEdit::SetAccessions { accessions },
        )?;
    }
    Ok(value)
}

fn accession_table(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let output = call.path("output")?;
    let submitter = submission::submitter_preference()?;
    let table = submission::accession_table(call.session, doc, &submitter)?;
    basecraft_io::write_atomic(&output, |w| w.write_all(&table))?;
    Ok(json!({"output": output}))
}

fn submitter(call: &mut Call) -> Result<Value> {
    let paths = basecraft_platform::paths::AppPaths::discover()?;
    let store = basecraft_platform::settings::SettingsStore::new(paths.preferences());
    let mut submitter = store.load()?.submitter;
    if call.has("set") {
        call.require_writes()?;
        let set: serde_json::Map<String, Value> = call.required_json("set")?;
        let mut value = serde_json::to_value(&submitter)?;
        for (name, field) in set {
            if value.get(&name).is_none() {
                return Err(super::wrong(&name, "a submitter field"));
            }
            value[name] = field;
        }
        submitter = serde_json::from_value(value)?;
        let change = basecraft_platform::settings::Change::Submitter(Box::new(submitter.clone()));
        store.update(change)?;
    }
    Ok(json!({"submitter": submitter, "missing": submitter.missing()}))
}

fn inclusion_preview(call: &mut Call) -> Result<Value> {
    let doc = submission_doc(call)?;
    let rule = call.required_json("rule")?;
    Ok(serde_json::to_value(submission::plan_inclusion(
        call.session,
        doc,
        &rule,
    )?)?)
}

fn inclusion_presets(call: &mut Call) -> Result<Value> {
    let project = call.project()?;
    if call.has("save") {
        call.require_writes()?;
        submission::save_inclusion_preset(call.session, call.required_json("save")?)?;
    }
    let (meta, _) = submission::project_metadata(project.root())?;
    Ok(serde_json::to_value(meta.import.inclusion_rules)?)
}

fn linked_alignment(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    Ok(json!({"path": delimitation::linked_alignment(call.session, doc)?}))
}

fn candidates(call: &mut Call) -> Result<Value> {
    let paths = call.paths("paths")?;
    let found = delimitation::combine_candidates(call.session.project(), &paths)?;
    Ok(serde_json::to_value(found)?)
}

fn combined_name(call: &mut Call) -> Result<Value> {
    Ok(json!({"path": delimitation::combined_path(&call.paths("paths")?)}))
}

fn combine(call: &mut Call) -> Result<Value> {
    let paths = call.paths("paths")?;
    let output = call.path("output")?;
    let mut keep = None;
    if let Some(selection) = call.json::<BTreeMap<String, BTreeSet<String>>>("selection")? {
        let mut selected = delimitation::CombineSelection::new();
        for (path, ids) in selection {
            // Relative and absolute spellings of one input collide only once
            // resolved, so resolve before comparing.
            if selected.insert(call.resolve(&path)?, ids).is_some() {
                let expected = "one entry per input, not several path aliases of one";
                return Err(super::wrong("selection", expected));
            }
        }
        keep = Some(selected);
    }
    let doc = delimitation::combine_to(call.session, &paths, &output, keep.as_ref())?;
    Ok(json!({"output": output, "document": doc.0}))
}

fn display_tree(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let tree = call.path("tree")?;
    delimitation::attach_display_tree(call.session, doc, &tree)?;
    let shown = call.session.delimitation(doc)?.view.display_tree.clone();
    call.state_with(doc, &super::Fields::from([("display_tree", shown)]))
}
