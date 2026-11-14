//! Commands on files and documents: create, edit, extract, tree and Sanger
//! decisions, numbering duplicate names, history, provenance and recovery.
use basecraft_protocol::Op;
use serde_json::{Value, json};

use super::Kind::{
    Choice, Document, File, Flag, Json, List, NewFile, Number, Operations, Text, Texts,
};
use super::{Arg, Call, Command, Routes::All, Routes::CommandLine, arg, document};
use crate::{Error, Result, records, renames, sanger_commands};

/// A file given in order on the command line, named `path` in a live session.
const fn path(help: &'static str) -> Arg {
    Arg::new("path", File, help).positional()
}

/// Where a command writes, `--output` or `-o` on the command line.
const fn output(help: &'static str) -> Arg {
    Arg::new("output", NewFile, help).short('o')
}

/// An argument given in order on the command line.
const fn given(name: &'static str, kind: super::Kind, help: &'static str) -> Arg {
    Arg::new(name, kind, help).positional()
}

#[rustfmt::skip]
pub(super) const COMMANDS: &[Command] = &[
    Command { name: "new-sequence", writes: true, routes: All, run: new_sequence,
        help: "Create an independent DNA sequence as a new GenBank file.",
        args: &[arg("name", Text, "The sequence's name."),
            arg("sequence", Text, "Plain IUPAC DNA text; whitespace is ignored."),
            output("The new GenBank file.")] },
    Command { name: "edit", writes: true, routes: CommandLine, run: edit,
        help: "Apply typed JSON operations to a document and save it.",
        args: &[document("The file."),
            given("operation", Operations, "One JSON operation, or an array applied in one session \
                    before saving.")] },
    Command { name: "save-check", writes: false, routes: All, run: save_check,
        help: "Inspect a save without writing. In a live session the returned token is good for \
                one save of this session; a newer check replaces it.",
        args: &[document("The document."), arg("path", NewFile, "Check a save to this new file \
                instead.").optional()] },
    Command { name: "extract", writes: true, routes: All, run: extract,
        help: "Extract a zero-based, half-open sequence range to a new GenBank file with source \
                ancestry. Optional splice segments are in biological order, followed by an \
                optional reverse complement. Existing destinations are refused.",
        args: &[document("The sequence."), given("start", Number, "First position, from 0."),
            given("end", Number, "Position after the last one."), output("The new GenBank file."),
            arg("segments", Json(SEGMENTS), "Ordered splice segments as [[start,end],...].").default("[]"),
            arg("reverse", Flag, "Reverse-complement the extract."),
            arg("record", Text, "Check that this is the open (first) record of a file of several. \
                    Other records can't be extracted yet.").optional(),
            arg("version", Number, "Refuse unless the document is still at this version; a live \
                    session must give it.").optional()] },
    Command { name: "import-support", writes: true, routes: All, run: import_support,
        help: "Add the support values of another tree file to a tree as one undoable edit. The \
                source is recorded as a referenced input, not copied.",
        args: &[document("The tree."), path("The tree file whose support values to add."),
            arg("name", Text, "Name for the imported support set; empty keeps the source's names.").default("")] },
    Command { name: "tree reroot", writes: true, routes: All, run: tree_reroot,
        help: "Root the tree on the branch above the outgroup: one tip, or the smallest clade \
                holding every tip given.",
        args: &[document("The tree."), arg("outgroup", Texts, "An outgroup tip; repeat for a \
                clade.")] },
    Command { name: "tree ladderize", writes: true, routes: All, run: tree_ladderize,
        help: "Order every node's branches by size.",
        args: &[document("The tree."),
            arg("order", Choice(&["smallest-first", "largest-first"]), "Smallest clades first (on \
                    top), or largest first.")] },
    Command { name: "tree rotate", writes: true, routes: All, run: tree_rotate,
        help: "Reverse the order of a clade's branches.",
        args: &[document("The tree."), arg("clade", Texts, "The clade: one tip, or tips whose \
                smallest clade it is; repeat.")] },
    Command { name: "tree labels", writes: true, routes: All, run: tree_labels,
        help: "Label the tips from the project's specimen table. Tips whose link is missing or \
                ambiguous keep their own name (kept_own_name counts them). One undoable style \
                edit.",
        args: &[document("The tree."), arg("fields", List, "Fields in order: specimen, genus, \
                species, accession, column:NAME.")] },
    Command { name: "number-duplicates", writes: true, routes: All, run: number_duplicates,
        help: "Number duplicate record, row or tip names: `a`, `a` become `a.1`, `a.2`. An \
                alignment or tree is renamed as one edit in its history, which a live session can \
                undo. The records of another FASTA or GenBank file are renamed in the file \
                itself, recorded in its history; refused while it has unsaved edits, and an open \
                copy keeps the old names until opened again. Links that named a duplicated name \
                are listed for relinking.",
        args: &[path("The file.")] },
    Command { name: "sanger review", writes: true, routes: All, run: sanger_review,
        help: "Start a review of a Sanger read (.ab1) from its instrument calls and save it as a \
                new review file.",
        args: &[given("read", File, "The read."), output("The new review file.")] },
    Command { name: "sanger decide", writes: true, routes: All, run: sanger_decide,
        help: "Decide draft columns of a Sanger review: accept the call, reject it, set it back to \
                pending, or give the base to put there. Logged and undoable.",
        args: &[document("The review."), arg("columns", Text, "Draft columns from 1: `12`, `12,15` \
                or `12-14`."),
            arg("choice", Text, "`accept`, `reject`, `pending`, or a base (IUPAC, `-` for a gap).")] },
    Command { name: "sanger export", writes: true, routes: All, run: sanger_export,
        help: "Export a review's consensus as FASTA. Every column must be decided.",
        args: &[document("The review."), output("The new FASTA file."),
            arg("label", Text, "The sequence's name in the FASTA file, such as the specimen ID.")] },
    Command { name: "sanger add", writes: true, routes: All, run: sanger_add,
        help: "Add a review's consensus to an alignment as a new last row, recording the review as \
                its evidence. Every column must be decided. The row is not placed: align \
                afterwards (`run mafft/align`).",
        args: &[document("The review."), arg("alignment", Document, "The alignment."),
            arg("label", Text, "The row's name, such as the specimen ID.")] },
    Command { name: "alphabet", writes: false, routes: All, run: alphabet,
        help: "What an alignment or sequence is written in: the stated alphabet or basecraft's \
                guess, and each sequence's guess.",
        args: &[document("The file.")] },
    Command { name: "index", writes: false, routes: All, run: index,
        help: "Index a FASTA file (or load its cached index) and print a summary.",
        args: &[path("The FASTA file.")] },
    Command { name: "fetch", writes: false, routes: All, run: fetch,
        help: "Residues `start..end` (from 0, half-open) of one record of a FASTA file.",
        args: &[path("The FASTA file."), given("record", Text, "The record's name."),
            given("start", Number, "First residue, from 0."), given("end", Number, "Residue after \
                    the last one.")] },
    Command { name: "history", writes: false, routes: All, run: history,
        help: "Readable history of a saved file's exact current contents, including saved Sanger \
                reviews. Writes nothing.",
        args: &[path("The file."), arg("json", Flag, "The whole record as JSON.")] },
    Command { name: "methods", writes: false, routes: All, run: methods,
        help: "The methods text of a saved file; with output, also export the report the History \
                window exports: methods, history, record, parameters, source map, citations and \
                manifest.",
        args: &[path("The file."), arg("json", Flag, "The whole record as JSON."),
            arg("output", NewFile, "Export the report into this folder.").optional()] },
    Command { name: "provenance inspect", writes: false, routes: All, run: provenance_inspect,
        help: "Read the provenance publication of a saved result.",
        args: &[path("The result.")] },
    Command { name: "provenance verify", writes: false, routes: All, run: provenance_verify,
        help: "Check a saved result against its recorded hashes and retained inputs; runs no \
                tools. Replay is a separate check.",
        args: &[path("The result.")] },
    Command { name: "replay", writes: false, routes: All, run: replay,
        help: "Replay the project's operation log in a scratch session and report problems.",
        args: &[] },
    Command { name: "records reindex", writes: true, routes: All, run: reindex,
        help: "Rebuild the project's analysis indexes from its analysis records, after an index \
                was damaged, deleted or left out of a copy.",
        args: &[] },
    Command { name: "recover", writes: true, routes: All, run: recover,
        help: "Reconstruct an unsaved session with all its call sets into a new folder; never \
                overwrites inputs. With a project, the recovered files keep their records in it.",
        args: &[given("session", File, "The session's recovery folder."), output("The new folder."),
            arg("verified_prefix", Flag, "After a crash damaged the journal: recover only the \
                    edits before the first damaged line. The journal itself is left unchanged.")] },
    Command { name: "preferences", writes: false, routes: All, run: preferences,
        help: "The validated shared preferences as JSON, without creating files.",
        args: &[] },
    Command { name: "schemas", writes: true, routes: CommandLine, run: schemas,
        help: "Write JSON Schemas for the protocol types into a folder.",
        args: &[given("dir", NewFile, "The folder.")] },
];

const SEGMENTS: &str = r#"{"type": "array", "items": {"type": "array", "minItems": 2, "maxItems": 2,
    "items": {"type": "integer", "minimum": 0}}}"#;

fn new_sequence(call: &mut Call) -> Result<Value> {
    let output = call.path("output")?;
    let (name, sequence) = (
        call.text("name")?.to_owned(),
        call.text("sequence")?.to_owned(),
    );
    let doc = call.session.create_sequence(&name, &sequence)?;
    call.save_new(doc, &output)?;
    call.state(doc)
}

fn edit(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let operations = super::live::operations(call, "operation")?;
    super::live::apply_all(call.session, doc, operations)?;
    // A reroot can leave labels on other groups; place them with tree.place_label.
    let displaced = call
        .session
        .tree(doc)
        .map(basecraft_docs::TreeDoc::displaced_labels);
    match displaced {
        Ok(displaced) if !displaced.is_empty() => {
            Ok(serde_json::to_value(super::Fields::from([(
                "displaced_labels",
                displaced,
            )]))?)
        }
        _ => Ok(Value::Null),
    }
}

fn save_check(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let path = call.optional_path("path")?;
    Ok(serde_json::to_value(
        call.session.check_save(doc, path.as_deref())?,
    )?)
}

fn extract(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let revision = call.session.revision(doc)?;
    // In a live session the document can change between calls, so the client
    // names the version it inspected; a command-line run opens it fresh.
    match call.optional_number("version")? {
        None if call.is_live() => return Err(Error::MissingArgument("version")),
        Some(version) if version != revision => {
            return Err(Error::StaleRevision { id: doc, revision });
        }
        _ => {}
    }
    let output = call.path("output")?;
    if output.try_exists()? {
        return Err(Error::OutputConflict(
            "Extraction destination already exists; choose a new file".into(),
        ));
    }
    let segments: Vec<(u64, u64)> = call.json("segments")?.unwrap_or_default();
    let range = (call.number("start")?, call.number("end")?);
    let reverse = call.flag("reverse")?;
    let record = call.optional_text("record")?.map(str::to_owned);
    call.session
        .extract(doc, range, &segments, reverse, &output, record.as_deref())?;
    Ok(json!({"output": output, "hash": basecraft_io::hash_file(&output)?}))
}

fn import_support(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let source = call.path("path")?;
    let name = call.optional_text("name")?.unwrap_or("").to_owned();
    let imported = call.session.import_support(doc, &source, &name)?;
    if imported.added.is_empty() {
        return Err(Error::Empty(source));
    }
    call.state_with(doc, &imported)
}

/// Apply one tree edit and return the tree's state, which lists the labels a
/// reroot left on other groups.
fn tree_edit(call: &mut Call, op: Op) -> Result<Value> {
    let doc = call.doc("document")?;
    renames::apply_op(call.session, doc, op)?;
    call.state(doc)
}

fn tree_reroot(call: &mut Call) -> Result<Value> {
    let clade = call.texts("outgroup")?;
    tree_edit(call, Op::TreeReroot { clade })
}

fn tree_ladderize(call: &mut Call) -> Result<Value> {
    let small_first = call.text("order")? == "smallest-first";
    tree_edit(call, Op::TreeLadderize { small_first })
}

fn tree_rotate(call: &mut Call) -> Result<Value> {
    let clade = call.texts("clade")?;
    tree_edit(call, Op::TreeRotate { clade })
}

fn tree_labels(call: &mut Call) -> Result<Value> {
    let project = call.project()?;
    let fields = call.texts("fields")?;
    let fields = fields
        .iter()
        .map(|f| crate::metadata::label_field(f))
        .collect::<Result<_>>()?;
    let doc = call.doc("document")?;
    let kept = crate::metadata::label_tips(call.session, doc, &project, fields)?;
    call.state_with(doc, &super::Fields::from([("kept_own_name", kept)]))
}

/// D29: number duplicate names, as the window offers when it finds them.
fn number_duplicates(call: &mut Call) -> Result<Value> {
    use crate::files::Kind;
    let path = call.path("path")?;
    // An alignment or tree is numbered as one edit, recorded in its history
    // like every other edit; only other record files are renamed on disk.
    let doc = match super::open_at(call.session, &path)? {
        Some(doc) if call.session.is_alignment(doc) || call.session.is_tree(doc) => Some(doc),
        Some(_) => None,
        None => match crate::files::kind(&path)? {
            Kind::Alignment | Kind::Tree => Some(call.open(&path)?),
            _ => None,
        },
    };
    let Some(doc) = doc else {
        // Other record files are renamed in the file itself, a run recorded
        // in its history. An open copy keeps the old names, so it is not one
        // the command line saves afterwards.
        let found = renames::duplicate_records(call.session, &path)?.unwrap_or_default();
        if found.names.is_empty() {
            return Ok(serde_json::to_value(found)?);
        }
        let doc = match super::open_at(call.session, &path)? {
            Some(doc) => doc,
            None => call.session.open(&path)?,
        };
        let found = renames::number_open_records(call.session, doc)?;
        return Ok(serde_json::to_value(found)?);
    };
    let found = renames::duplicates(call.session, doc)?.unwrap_or_default();
    if !found.names.is_empty() {
        renames::make_unique(call.session, doc, &found)?;
    }
    call.state_with(doc, &found)
}

fn sanger_review(call: &mut Call) -> Result<Value> {
    let read = call.path("read")?;
    let output = call.path("output")?;
    let doc = sanger_commands::review_read(call.session, &read)?;
    call.save_new(doc, &output)?;
    let review = call.session.sanger_review(doc)?;
    let extra = json!({"review": output, "columns": review.draft.columns.len(),
        "unresolved": review.unresolved()});
    call.state_with(doc, &extra)
}

fn sanger_decide(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let edit = basecraft_protocol::sanger::ReviewOp::Set {
        columns: sanger_commands::columns(call.text("columns")?)?,
        choice: sanger_commands::choice(call.text("choice")?)?,
    };
    sanger_commands::decide(call.session, doc, edit)?;
    let unresolved = call.session.sanger_review(doc)?.unresolved();
    call.state_with(doc, &super::Fields::from([("unresolved", unresolved)]))
}

fn sanger_export(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let output = call.path("output")?;
    let label = call.text("label")?.to_owned();
    call.session.export_sanger_consensus(doc, &output, &label)?;
    Ok(serde_json::to_value(super::Fields::from([(
        "consensus",
        output,
    )]))?)
}

fn sanger_add(call: &mut Call) -> Result<Value> {
    let review = call.doc("document")?;
    let alignment = call.doc("alignment")?;
    let label = call.text("label")?.to_owned();
    sanger_commands::add_consensus(call.session, review, alignment, &label)?;
    call.state(alignment)
}

fn alphabet(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let alphabet = call.session.alphabet(doc)?;
    let sequences = call.session.sequence_alphabets(doc)?;
    Ok(json!({ "alphabet": alphabet, "sequences": sequences }))
}

fn index(call: &mut Call) -> Result<Value> {
    let started = std::time::Instant::now();
    let fasta = basecraft_io::fasta::IndexedFasta::open(&call.path("path")?)?;
    let elapsed = started.elapsed().as_secs_f64() * 1e3;
    let records = &fasta.index.records;
    let residues: u64 = records.iter().map(|r| r.len).sum();
    let ragged = records
        .iter()
        .filter(|r| matches!(r.layout, basecraft_io::fasta::Layout::Ragged(_)))
        .count();
    let count = records.len();
    Ok(Value::from(format!(
        "{count} records, {residues} residues, {ragged} ragged, index built in {elapsed:.1} ms"
    )))
}

fn fetch(call: &mut Call) -> Result<Value> {
    let fasta = basecraft_io::fasta::IndexedFasta::open(&call.path("path")?)?;
    let name = call.text("record")?;
    let record = fasta
        .index
        .get(name)
        .ok_or_else(|| basecraft_io::Error::NoSuchRecord(name.to_owned()))?;
    let residues = record.read(fasta.bytes(), call.number("start")?, call.number("end")?)?;
    Ok(Value::from(String::from_utf8_lossy(&residues)))
}

fn saved_report(call: &Call) -> Result<records::Report> {
    records::DocumentRecord::saved_report(&call.path("path")?, call.session.project())
}

fn history(call: &mut Call) -> Result<Value> {
    let report = saved_report(call)?;
    if call.flag("json")? {
        return Ok(serde_json::to_value(report)?);
    }
    Ok(Value::from(report.history_text))
}

/// D30: one report export, the files the History window exports.
fn methods(call: &mut Call) -> Result<Value> {
    let report = saved_report(call)?;
    if let Some(output) = call.optional_path("output")? {
        call.require_writes()?;
        records::ReportExt::export(&report, &output)?;
    }
    if call.flag("json")? {
        return Ok(serde_json::to_value(report)?);
    }
    Ok(Value::from(report.methods))
}

fn provenance_inspect(call: &mut Call) -> Result<Value> {
    let publication = records::inspect(&call.path("path")?, call.session.project())?;
    Ok(serde_json::to_value(publication)?)
}

fn provenance_verify(call: &mut Call) -> Result<Value> {
    let issues = records::verify(&call.path("path")?, call.session.project())?;
    if !issues.is_empty() {
        call.fail();
    }
    Ok(json!({"verified": issues.is_empty(), "issues": issues}))
}

fn replay(call: &mut Call) -> Result<Value> {
    let report = records::replay(call.project()?.root())?;
    if !report.ok() {
        call.fail();
    }
    Ok(serde_json::to_value(report)?)
}

fn reindex(call: &mut Call) -> Result<Value> {
    Ok(serde_json::to_value(records::reindex(
        call.project()?.root(),
    )?)?)
}

fn recover(call: &mut Call) -> Result<Value> {
    let journal = call.path("session")?;
    let output = call.path("output")?;
    let project = call.session.project();
    let (recovered, damage) = if call.flag("verified_prefix")? {
        records::recover_verified_prefix(&journal, &output, project)?
    } else {
        (records::recover(&journal, &output, project)?, None)
    };
    Ok(json!({"recovered": recovered, "damage": damage}))
}

fn preferences(_: &mut Call) -> Result<Value> {
    let paths = basecraft_platform::paths::AppPaths::discover()?;
    let preferences =
        basecraft_platform::settings::SettingsStore::new(paths.preferences()).load()?;
    Ok(serde_json::to_value(preferences)?)
}

fn schemas(call: &mut Call) -> Result<Value> {
    basecraft_protocol::schemas::write_all(&call.path("dir")?)?;
    Ok(Value::Null)
}
