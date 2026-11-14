//! Commands on files and documents: create, edit, extract, tree and Sanger
//! decisions, numbering duplicate names, history, provenance and recovery.
use basecraft_protocol::Op;
use serde_json::{Value, json};

use super::Kind::{
    Choice, Document, File, Flag, Json, List, NewFile, Number, Operations, Text, Texts,
};
use super::{
    Arg, Call, Command,
    Routes::{All, CommandLine},
    arg, document,
};
use crate::{Error, Result, records, renames, sanger_commands};

/// A file given in order on the command line, named `path` in a live session.
const fn path(help: &'static str) -> Arg {
    Arg::new("path", File, help).positional()
}

/// Where a command writes, `--output` or `-o` on the command line.
const fn output(help: &'static str) -> Arg {
    Arg::new("output", NewFile, help).short('o')
}

pub(super) const COMMANDS: &[Command] = &[
    Command {
        name: "new-sequence",
        help: "Create an independent DNA sequence as a new GenBank file.",
        args: &[
            arg("name", Text, "The sequence's name."),
            arg("sequence", Text, "Plain IUPAC DNA text; whitespace is ignored."),
            output("The new GenBank file."),
        ],
        writes: true,
        routes: All,
        run: new_sequence,
    },
    Command {
        name: "edit",
        help: "Apply typed JSON operations to a document and save it.",
        args: &[
            document("The file."),
            Arg::new("operation", Operations, "One JSON operation, or an array applied in one session before saving.").positional(),
        ],
        writes: true,
        routes: CommandLine,
        run: edit,
    },
    Command {
        name: "save-check",
        help: "Inspect a save without writing. In a live session the returned token is good for one save of this session; a newer check replaces it.",
        args: &[
            document("The document."),
            arg("path", NewFile, "Check a save to this new file instead.").optional(),
        ],
        writes: false,
        routes: All,
        run: save_check,
    },
    Command {
        name: "extract",
        help: "Extract a zero-based, half-open sequence range to a new GenBank file with source ancestry. Optional splice segments are in biological order, followed by an optional reverse complement. Existing destinations are refused.",
        args: &[
            document("The sequence."),
            Arg::new("start", Number, "First position, from 0.").positional(),
            Arg::new("end", Number, "Position after the last one.").positional(),
            output("The new GenBank file."),
            arg("segments", Json(SEGMENTS), "Ordered splice segments as [[start,end],...].").default("[]"),
            arg("reverse", Flag, "Reverse-complement the extract."),
            arg("record", Text, "Check that this is the open (first) record of a file of several. Other records can't be extracted yet.").optional(),
            arg("version", Number, "Refuse unless the document is still at this version.").optional(),
        ],
        writes: true,
        routes: All,
        run: extract,
    },
    Command {
        name: "import-support",
        help: "Add the support values of another tree file to a tree as one undoable edit. The source is recorded as a referenced input, not copied.",
        args: &[
            document("The tree."),
            path("The tree file whose support values to add."),
            arg("name", Text, "Name for the imported support set; empty keeps the source's names.").default(""),
        ],
        writes: true,
        routes: All,
        run: import_support,
    },
    Command {
        name: "tree reroot",
        help: "Root the tree on the branch above the outgroup: one tip, or the smallest clade holding every tip given.",
        args: &[document("The tree."), arg("outgroup", Texts, "An outgroup tip; repeat for a clade.")],
        writes: true,
        routes: All,
        run: tree_reroot,
    },
    Command {
        name: "tree ladderize",
        help: "Order every node's branches by size.",
        args: &[document("The tree."), arg("order", Choice(&["smallest-first", "largest-first"]), "Smallest clades first (on top), or largest first.")],
        writes: true,
        routes: All,
        run: tree_ladderize,
    },
    Command {
        name: "tree rotate",
        help: "Reverse the order of a clade's branches.",
        args: &[document("The tree."), arg("clade", Texts, "The clade: one tip, or tips whose smallest clade it is; repeat.")],
        writes: true,
        routes: All,
        run: tree_rotate,
    },
    Command {
        name: "tree labels",
        help: "Label the tips from the project's specimen table. Tips whose link is missing or ambiguous keep their own name (kept_own_name counts them). One undoable style edit.",
        args: &[document("The tree."), arg("fields", List, "Fields in order: specimen, genus, species, accession, column:NAME.")],
        writes: true,
        routes: All,
        run: tree_labels,
    },
    Command {
        name: "number-duplicates",
        help: "Number duplicate record, row or tip names: `a`, `a` become `a.1`, `a.2`. An alignment or tree is renamed as one undoable edit; the records of another FASTA or GenBank file are renamed in the file itself, which must not be open. Links that named a duplicated name are listed for relinking.",
        args: &[path("The file.")],
        writes: true,
        routes: All,
        run: number_duplicates,
    },
    Command {
        name: "sanger review",
        help: "Start a review of a Sanger read (.ab1) from its instrument calls and save it as a new review file.",
        args: &[Arg::new("read", File, "The read.").positional(), output("The new review file.")],
        writes: true,
        routes: All,
        run: sanger_review,
    },
    Command {
        name: "sanger decide",
        help: "Decide draft columns of a Sanger review: accept the call, reject it, set it back to pending, or give the base to put there. Logged and undoable.",
        args: &[
            document("The review."),
            arg("columns", Text, "Draft columns from 1: `12`, `12,15` or `12-14`."),
            arg("choice", Text, "`accept`, `reject`, `pending`, or a base (IUPAC, `-` for a gap)."),
        ],
        writes: true,
        routes: All,
        run: sanger_decide,
    },
    Command {
        name: "sanger export",
        help: "Export a review's consensus as FASTA. Every column must be decided.",
        args: &[
            document("The review."),
            output("The new FASTA file."),
            arg("label", Text, "The sequence's name in the FASTA file, such as the specimen ID."),
        ],
        writes: true,
        routes: All,
        run: sanger_export,
    },
    Command {
        name: "sanger add",
        help: "Add a review's consensus to an alignment as a new last row, recording the review as its evidence. Every column must be decided. The row is not placed: align afterwards (`run mafft/align`).",
        args: &[
            document("The review."),
            arg("alignment", Document, "The alignment."),
            arg("label", Text, "The row's name, such as the specimen ID."),
        ],
        writes: true,
        routes: All,
        run: sanger_add,
    },
    Command {
        name: "alphabet",
        help: "What an alignment or sequence is written in: the stated alphabet or basecraft's guess, and each sequence's guess.",
        args: &[document("The file.")],
        writes: false,
        routes: All,
        run: alphabet,
    },
    Command {
        name: "index",
        help: "Index a FASTA file (or load its cached index) and print a summary.",
        args: &[path("The FASTA file.")],
        writes: false,
        routes: All,
        run: index,
    },
    Command {
        name: "fetch",
        help: "Residues `start..end` (from 0, half-open) of one record of a FASTA file.",
        args: &[
            path("The FASTA file."),
            Arg::new("record", Text, "The record's name.").positional(),
            Arg::new("start", Number, "First residue, from 0.").positional(),
            Arg::new("end", Number, "Residue after the last one.").positional(),
        ],
        writes: false,
        routes: All,
        run: fetch,
    },
    Command {
        name: "history",
        help: "Readable history of a saved file's exact current contents, including saved Sanger reviews. Writes nothing.",
        args: &[path("The file."), arg("json", Flag, "The whole record as JSON.")],
        writes: false,
        routes: All,
        run: history,
    },
    Command {
        name: "methods",
        help: "The methods text of a saved file; with output, also export the report: methods, history, record, parameters, source map, citations and manifest.",
        args: &[
            path("The file."),
            arg("json", Flag, "The whole record as JSON."),
            arg("output", NewFile, "Export the report into this folder.").optional(),
        ],
        writes: false,
        routes: All,
        run: methods,
    },
    Command {
        name: "provenance inspect",
        help: "Read the provenance publication of a saved result.",
        args: &[path("The result.")],
        writes: false,
        routes: All,
        run: provenance_inspect,
    },
    Command {
        name: "provenance verify",
        help: "Check a saved result against its recorded hashes and retained inputs; runs no tools. Replay is a separate check.",
        args: &[path("The result.")],
        writes: false,
        routes: All,
        run: provenance_verify,
    },
    Command {
        name: "provenance package",
        help: "Copy a verified result with its retained exact inputs into a new folder; runs no tools.",
        args: &[path("The result."), output("The new folder.")],
        writes: true,
        routes: All,
        run: provenance_package,
    },
    Command {
        name: "replay",
        help: "Replay the project's operation log in a scratch session and report problems.",
        args: &[],
        writes: false,
        routes: All,
        run: replay,
    },
    Command {
        name: "records reindex",
        help: "Rebuild the project's analysis indexes from its analysis records, after an index was damaged, deleted or left out of a copy.",
        args: &[],
        writes: true,
        routes: All,
        run: reindex,
    },
    Command {
        name: "recover",
        help: "Reconstruct an unsaved session with all its call sets into a new folder; never overwrites inputs. With a project, the recovered files keep their records in it.",
        args: &[
            Arg::new("session", File, "The session's recovery folder.").positional(),
            output("The new folder."),
            arg("verified_prefix", Flag, "After a crash damaged the journal: recover only the edits before the first damaged line. The journal itself is left unchanged."),
        ],
        writes: true,
        routes: All,
        run: recover,
    },
    Command {
        name: "preferences",
        help: "The validated shared preferences as JSON, without creating files.",
        args: &[],
        writes: false,
        routes: All,
        run: preferences,
    },
    Command {
        name: "schemas",
        help: "Write JSON Schemas for the protocol types into a folder.",
        args: &[Arg::new("dir", NewFile, "The folder.").positional()],
        writes: true,
        routes: CommandLine,
        run: schemas,
    },
];

const SEGMENTS: &str = r#"{"type":"array","items":{"type":"array","items":{"type":"integer","minimum":0},"minItems":2,"maxItems":2}}"#;

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
    if let Ok(tree) = call.session.tree(doc)
        && let displaced = tree.displaced_labels()
        && !displaced.is_empty()
    {
        return Ok(json!({ "displaced_labels": displaced }));
    }
    Ok(Value::Null)
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
    if let Some(version) = call.optional_number("version")? {
        let revision = call.session.revision(doc)?;
        if version != revision {
            return Err(Error::StaleRevision { id: doc, revision });
        }
    }
    let output = call.path("output")?;
    if output.try_exists()? {
        return Err(Error::Invalid(
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
        return Err(Error::Invalid(
            "The source tree has no support values".into(),
        ));
    }
    call.state_with(
        doc,
        json!({"added": imported.added, "matched": imported.matched}),
    )
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
    let fields = call
        .texts("fields")?
        .iter()
        .map(|field| crate::metadata::label_field(field))
        .collect::<Result<Vec<_>>>()?;
    let doc = call.doc("document")?;
    let kept = crate::metadata::label_tips(call.session, doc, &project, fields)?;
    call.state_with(doc, json!({"kept_own_name": kept}))
}

fn number_duplicates(call: &mut Call) -> Result<Value> {
    let path = call.path("path")?;
    let open = super::open_at(call.session, &path)?;
    let doc = match open {
        Some(doc) if call.session.is_alignment(doc) || call.session.is_tree(doc) => Some(doc),
        Some(_) => None,
        None if crate::files::Format::from_path(&path)? == crate::files::Format::Tree => {
            Some(call.open(&path)?)
        }
        None => None,
    };
    let Some(doc) = doc else {
        let Some(found) = renames::duplicate_records(call.session, &path)? else {
            return Ok(json!({"names": []}));
        };
        renames::number_records(call.session, &path, &found)?;
        return numbered(&found);
    };
    let Some(found) = renames::duplicates(call.session, doc)? else {
        return Ok(json!({"names": []}));
    };
    renames::make_unique(call.session, doc, &found)?;
    let mut value = numbered(&found)?;
    value["document"] = json!(doc.0);
    Ok(value)
}

fn numbered(found: &renames::DuplicateNames) -> Result<Value> {
    Ok(json!({
        "names": found.names,
        "renames": found.renames,
        "links": found.links,
        "submissions": found.submissions,
    }))
}

fn sanger_review(call: &mut Call) -> Result<Value> {
    let read = call.path("read")?;
    let output = call.path("output")?;
    let doc = sanger_commands::review_read(call.session, &read)?;
    call.save_new(doc, &output)?;
    let review = call.session.sanger_review(doc)?;
    let extra = json!({"review": output, "columns": review.draft.columns.len(),
        "unresolved": review.unresolved()});
    call.state_with(doc, extra)
}

fn sanger_decide(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let edit = basecraft_protocol::sanger::ReviewOp::Set {
        columns: sanger_commands::columns(call.text("columns")?)?,
        choice: sanger_commands::choice(call.text("choice")?)?,
    };
    sanger_commands::decide(call.session, doc, edit)?;
    let unresolved = call.session.sanger_review(doc)?.unresolved();
    call.state_with(doc, json!({"unresolved": unresolved}))
}

fn sanger_export(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let output = call.path("output")?;
    let label = call.text("label")?.to_owned();
    call.session.export_sanger_consensus(doc, &output, &label)?;
    Ok(json!({"consensus": output}))
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
    let elapsed = started.elapsed();
    let records = &fasta.index.records;
    let residues: u64 = records.iter().map(|r| r.len).sum();
    let ragged = records
        .iter()
        .filter(|r| matches!(r.layout, basecraft_io::fasta::Layout::Ragged(_)))
        .count();
    Ok(json!(format!(
        "{} records, {residues} residues, {ragged} ragged, index built in {:.1} ms",
        records.len(),
        elapsed.as_secs_f64() * 1e3
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
    Ok(json!(String::from_utf8_lossy(&residues)))
}

fn saved_report(call: &Call) -> Result<records::Report> {
    records::DocumentRecord::saved_report(&call.path("path")?, call.session.project())
}

fn history(call: &mut Call) -> Result<Value> {
    let report = saved_report(call)?;
    if call.flag("json")? {
        return Ok(serde_json::to_value(report)?);
    }
    Ok(json!(report.history_text))
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
    Ok(json!(report.methods))
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

fn provenance_package(call: &mut Call) -> Result<Value> {
    let output = call.path("output")?;
    records::package(&call.path("path")?, &output, call.session.project())?;
    Ok(json!({"package": output}))
}

fn replay(call: &mut Call) -> Result<Value> {
    let report = records::replay(call.project()?.root())?;
    if !report.ok() {
        call.fail();
    }
    Ok(
        json!({"sessions": report.sessions, "entries": report.entries,
        "unchecked": report.unchecked, "problems": report.problems}),
    )
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
    if call.flag("verified_prefix")? {
        let (recovered, damage) = records::recover_verified_prefix(&journal, &output, project)?;
        return Ok(json!({"recovered": recovered, "damage": damage}));
    }
    let recovered = records::recover(&journal, &output, project)?;
    Ok(json!({"recovered": recovered}))
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
