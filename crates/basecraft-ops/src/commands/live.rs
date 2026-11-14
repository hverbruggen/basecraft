//! Commands on documents that stay open between calls: open, read, edit,
//! undo, save. Only a live session (MCP, Python) offers them; on the command
//! line every command opens its files and saves what it changed.
use basecraft_protocol::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use super::Kind::{Flag, Json, NewFile, Number, Operations};
use super::{Call, Command, Routes::LiveSession, arg, document};
use crate::{DocId, Error, Result, records::SavePolicy};

pub(super) const COMMANDS: &[Command] = &[
    Command {
        name: "open",
        help: "Open a document within the project.",
        args: &[arg("path", super::Kind::File, "The file, relative to the project.")],
        writes: false,
        routes: LiveSession,
        run: open,
    },
    Command {
        name: "documents",
        help: "List this session's open documents.",
        args: &[],
        writes: false,
        routes: LiveSession,
        run: documents,
    },
    Command {
        name: "state",
        help: "Inspect document dimensions, version and undo state.",
        args: &[document("The document.")],
        writes: false,
        routes: LiveSession,
        run: state,
    },
    Command {
        name: "read",
        help: "Read a residue range (of one row of an alignment), or a tree's tips and style, a species workspace, the metadata or a submission.",
        args: &[
            document("The document."),
            arg("start", Number, "First residue, from 0.").default("0"),
            arg("end", Number, "Residue after the last one.").optional(),
            arg("row", Number, "The alignment row, from 0.").optional(),
        ],
        writes: false,
        routes: LiveSession,
        run: read,
    },
    Command {
        name: "apply",
        help: "Apply typed operations to the inspected document version; logged and undoable.",
        args: &[
            document("The document."),
            arg("version", Number, "The version you inspected."),
            arg("operation", Operations, "One typed operation, or a list applied in order."),
        ],
        writes: true,
        routes: LiveSession,
        run: apply,
    },
    Command {
        name: "undo",
        help: "Undo one edit group.",
        args: &[document("The document.")],
        writes: true,
        routes: LiveSession,
        run: undo,
    },
    Command {
        name: "redo",
        help: "Redo one edit group.",
        args: &[document("The document.")],
        writes: true,
        routes: LiveSession,
        run: redo,
    },
    Command {
        name: "save",
        help: "Save a document, optionally to a new project path. An existing other file is never replaced.",
        args: &[
            document("The document."),
            arg("path", NewFile, "Save to this new file instead.").optional(),
            arg("decision", Json(DECISION), "The decision on a check from save_check: its token, the policy for analyses that used the file, and whether to save although some could not be checked.").optional(),
            arg("drop_newer_fields", Flag, "Save even when a file written by a later basecraft holds fields this version does not know; they are dropped."),
        ],
        writes: true,
        routes: LiveSession,
        run: save,
    },
    Command {
        name: "document record",
        help: "Read history and methods for this session's current revision, including unsaved edits.",
        args: &[document("The document.")],
        writes: false,
        routes: LiveSession,
        run: document_record,
    },
    Command {
        name: "metadata open",
        help: "Open the project's specimen metadata (roster, taxa, identifications, record links, typed collection columns) as a document. With create, write empty tables first if the project has none. Edit it with apply and metadata.edit operations; save it with save.",
        args: &[arg("create", Flag, "Write empty tables first when the project has none.")],
        writes: false,
        routes: LiveSession,
        run: metadata_open,
    },
];

const DECISION: &str = r#"{"type":"object","additionalProperties":false,"required":["token","known_policy","acknowledge_incomplete"],"properties":{"token":{"type":"string"},"known_policy":{"type":"string","enum":["cancel","preserve_copy","break_evidence","remove_analysis"]},"acknowledge_incomplete":{"type":"boolean"}}}"#;

/// A client's decision on a save check. The session, not this copy, owns the
/// check it names.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub token: String,
    pub known_policy: SavePolicy,
    pub acknowledge_incomplete: bool,
}

fn open(call: &mut Call) -> Result<Value> {
    let path = call.path("path")?;
    let doc = call.session.open(&path)?;
    call.state(doc)
}

fn documents(call: &mut Call) -> Result<Value> {
    let docs = super::open_documents(call.session)
        .map(|doc| call.state(doc))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!(docs))
}

fn state(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    call.state(doc)
}

fn read(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let session = &*call.session;
    if session.is_tree(doc) {
        let tree = session.tree(doc)?;
        let tips: Vec<_> = tree
            .nodes()
            .iter()
            .filter(|n| n.is_tip())
            .map(|n| &n.name)
            .collect();
        return Ok(json!({"tips": tips, "style": tree.style()}));
    }
    if session.is_delimitation(doc) {
        return Ok(serde_json::to_value(session.delimitation(doc)?)?);
    }
    if session.is_metadata(doc) {
        return Ok(
            json!({"metadata": session.metadata(doc)?, "notices": session.metadata_notices(doc)?}),
        );
    }
    if session.is_submission(doc) {
        return Ok(serde_json::to_value(session.submission(doc)?)?);
    }
    let start = call.optional_number("start")?.unwrap_or(0);
    let end = call
        .optional_number("end")?
        .ok_or_else(|| Error::Invalid("Provide an exclusive end coordinate".into()))?;
    if end < start || end - start > 1_000_000 || end > session.state(doc)?.len {
        return Err(Error::Invalid(
            "Invalid range or more than one million residues requested".into(),
        ));
    }
    let residues = if session.is_alignment(doc) {
        let row = call
            .optional_number("row")?
            .and_then(|r| usize::try_from(r).ok())
            .ok_or_else(|| Error::Invalid("Provide an alignment row".into()))?;
        if row >= session.alignment(doc)?.row_count() {
            return Err(Error::Invalid("Row outside alignment".into()));
        }
        session.alignment(doc)?.read(row, start, end)
    } else {
        session.doc(doc)?.read(start, end)
    };
    Ok(json!({"residues": String::from_utf8_lossy(&residues)}))
}

fn apply(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let version = call.number("version")?;
    let revision = call.session.revision(doc)?;
    if version != revision {
        return Err(Error::StaleRevision { id: doc, revision });
    }
    let operations = operations(call, "operation")?;
    apply_all(call.session, doc, operations)?;
    call.state(doc)
}

/// The edits of an `Operations` argument.
pub(super) fn operations(call: &Call, name: &'static str) -> Result<Vec<Op>> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Edits {
        One(Box<Op>),
        Many(Vec<Op>),
    }
    Ok(match call.required_json(name)? {
        Edits::One(op) => vec![*op],
        Edits::Many(ops) => ops,
    })
}

/// Apply edits in order. A row or tip rename takes its links along, as in
/// the window; history undo and redo step through the document's history.
pub(super) fn apply_all(
    session: &mut crate::Session,
    doc: DocId,
    operations: Vec<Op>,
) -> Result<()> {
    for operation in operations {
        if matches!(operation, Op::Undo { .. } | Op::Redo { .. }) {
            session.apply_history(doc, &operation)?;
        } else {
            crate::renames::apply_op(session, doc, operation)?;
        }
    }
    Ok(())
}

fn undo(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    call.session.undo(doc)?;
    call.state(doc)
}

fn redo(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    call.session.redo(doc)?;
    call.state(doc)
}

fn save(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let path = call.optional_path("path")?;
    if call.flag("drop_newer_fields")? {
        call.session.drop_newer_fields(doc)?;
    }
    // No overwrite confirmation exists here: an existing other file is refused.
    let warnings = if let Some(choice) = call.json::<Decision>("decision")? {
        let decision = call.session.decide_save(
            doc,
            path.as_deref(),
            &choice.token,
            choice.known_policy,
            choice.acknowledge_incomplete,
        )?;
        call.session
            .save_new_with_decision(doc, path.as_deref(), decision)?
            .warnings
    } else {
        call.session.save_new(doc, path.as_deref())?;
        Vec::new()
    };
    call.state_with(doc, json!({"warnings": warnings}))
}

fn document_record(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    Ok(serde_json::to_value(
        call.session.document_record(doc)?.report()?,
    )?)
}

fn metadata_open(call: &mut Call) -> Result<Value> {
    let create = call.flag("create")?;
    if create {
        call.require_writes()?;
    }
    let doc = crate::metadata::open(call.session, create)?.ok_or_else(|| {
        Error::MissingChoice(
            "no_metadata: This project has no metadata yet; call metadata_open with create".into(),
        )
    })?;
    let notices = call.session.metadata_notices(doc)?.to_vec();
    call.state_with(doc, json!({"notices": notices}))
}
