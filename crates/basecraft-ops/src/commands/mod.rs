//! The commands scripts and assistants give: one table the command line, the
//! MCP server and the Python `Session` are made from (brief rule 1.3).
//!
//! A command has a name, typed arguments, help and a function. The CLI makes
//! `basecraft <name>` from it; the MCP server makes a tool named by the words
//! of the name joined with `_` (`sanger decide` is `sanger_decide`), with a
//! JSON Schema of the arguments; Python calls the tools. A command added here
//! is offered on every route at once.
//!
//! The function gets a [`Call`]: the session and the arguments. The route
//! decides what a document or a path means. On the command line a document is
//! a file the command opens, and the CLI saves every document the command
//! changed; paths are relative to the working folder. In a live session (MCP,
//! Python) a document is the number of an open document, paths are relative
//! to the project and stay inside it, and nothing is saved until the client
//! saves.
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{DocId, Error, Result, Session};

mod documents;
mod live;
mod project;

/// One command of the table.
pub struct Command {
    /// Words separated by spaces: `sanger decide`.
    pub name: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
    /// Changes a file or a document. A read-only MCP server refuses it.
    pub writes: bool,
    pub routes: Routes,
    pub run: fn(&mut Call) -> Result<Value>,
}

/// Where a command is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routes {
    All,
    /// Only on the command line: it works on files from one start to its end.
    CommandLine,
    /// Only in a live session: it works on documents that stay open between
    /// calls, such as `undo` or `save`.
    LiveSession,
}

/// One argument of a command.
#[derive(Clone, Copy, Debug)]
pub struct Arg {
    /// The MCP property, and on the command line `--name` with `-` for `_`.
    pub name: &'static str,
    pub kind: Kind,
    pub help: &'static str,
    pub required: bool,
    /// On the command line, given in order without `--name`.
    pub positional: bool,
    /// On the command line, also `-c`.
    pub short: Option<char>,
    /// The value when the argument is left out.
    pub default: Option<&'static str>,
}

/// What an argument holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A document: on the command line a file the command opens, in a live
    /// session the number of an open document.
    Document,
    /// A file or folder that exists.
    File,
    /// A file or folder the command writes.
    NewFile,
    /// Several existing files.
    Files,
    Text,
    /// Several texts: on the command line the option repeated.
    Texts,
    /// Several texts: on the command line separated by commas.
    List,
    /// A whole number of zero or more.
    Number,
    /// Off unless given.
    Flag,
    /// One of these words.
    Choice(&'static [&'static str]),
    /// JSON with this JSON Schema. On the command line, JSON text, `@FILE` or
    /// `-` for standard input.
    Json(&'static str),
    /// One typed edit, or a list of them (`basecraft_protocol::Op`).
    Operations,
}

impl Arg {
    /// A required argument; a flag is never required.
    pub const fn new(name: &'static str, kind: Kind, help: &'static str) -> Self {
        Self {
            name,
            kind,
            help,
            required: !matches!(kind, Kind::Flag),
            positional: false,
            short: None,
            default: None,
        }
    }
    pub const fn positional(mut self) -> Self {
        self.positional = true;
        self
    }
    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }
    pub const fn short(mut self, short: char) -> Self {
        self.short = Some(short);
        self
    }
    pub const fn default(mut self, value: &'static str) -> Self {
        self.default = Some(value);
        self.required = false;
        self
    }
}

/// The document a command works on, positional on the command line.
const fn document(help: &'static str) -> Arg {
    Arg::new("document", Kind::Document, help).positional()
}

/// A required argument given as `--name` on the command line.
const fn arg(name: &'static str, kind: Kind, help: &'static str) -> Arg {
    Arg::new(name, kind, help)
}

/// The first words that group commands on the command line, with their help.
pub const GROUPS: &[(&str, &str)] = &[
    (
        "delimitation",
        "Species workspaces and saved species hypotheses.",
    ),
    (
        "tree",
        "Tree edits by name: reroot on an outgroup, ladderize, rotate a clade, label tips.",
    ),
    (
        "sanger",
        "Sanger reads: start a review from a read's calls, decide columns, export or add the consensus.",
    ),
    (
        "provenance",
        "Inspect, verify or package the readable provenance of saved results.",
    ),
    (
        "records",
        "Maintain the records basecraft keeps in a project's `.basecraft` folder.",
    ),
    (
        "metadata",
        "The project's specimen metadata: roster, identifications, record links and collection columns; imports and the .ods template.",
    ),
    (
        "submission",
        "GenBank marker submissions: row decisions, reading frames, the package, table2asn validation and accessions.",
    ),
];

/// Every command, in the order help lists them.
pub fn all() -> impl Iterator<Item = &'static Command> {
    live::COMMANDS
        .iter()
        .chain(documents::COMMANDS)
        .chain(project::COMMANDS)
}

/// The command `name` names: its words, or the MCP tool name.
pub fn find<'a>(commands: &[&'a Command], name: &str) -> Option<&'a Command> {
    commands
        .iter()
        .copied()
        .find(|command| command.name == name || tool_name(command.name) == name)
}

/// The MCP tool name of a command: its words joined with `_`.
pub fn tool_name(name: &str) -> String {
    name.replace([' ', '-'], "_")
}

/// The answers the command line gives in advance to the questions a save can
/// ask, from its `--save-known-policy`, `--acknowledge-incomplete-evidence`
/// and `--drop-newer-fields` options. Every save of the run uses them.
#[derive(Clone, Copy, Debug, Default)]
pub struct SaveAnswers {
    pub known_policy: crate::records::SavePolicy,
    pub acknowledge_incomplete: bool,
    pub drop_newer_fields: bool,
}

impl SaveAnswers {
    /// Save `doc` to its own file, or to the new file `path`, with these
    /// answers. Returns the warnings of a save that went ahead.
    pub fn save(
        self,
        session: &mut Session,
        doc: DocId,
        path: Option<&Path>,
    ) -> Result<Vec<crate::session::SaveWarning>> {
        use crate::records::SavePolicy;
        if self.drop_newer_fields {
            session.drop_newer_fields(doc)?;
        }
        let new = path.is_some();
        if !self.acknowledge_incomplete && self.known_policy == SavePolicy::Cancel {
            if new {
                session.save_new(doc, path)?;
            } else {
                session.save(doc, None)?;
            }
            return Ok(Vec::new());
        }
        let check = session.check_save(doc, path)?;
        let decision = session.decide_save(
            doc,
            path,
            check.token(),
            self.known_policy,
            self.acknowledge_incomplete,
        )?;
        let outcome = if new {
            session.save_new_with_decision(doc, path, decision)?
        } else {
            session.save_with_decision(doc, None, decision)?
        };
        Ok(outcome.warnings)
    }
}

/// One run of a command: the session, the arguments and the route's rules.
pub struct Call<'a> {
    pub session: &'a mut Session,
    command: &'static str,
    args: Map<String, Value>,
    live: bool,
    writable: bool,
    recent: &'a [PathBuf],
    answers: SaveAnswers,
    warnings: Vec<crate::session::SaveWarning>,
    edited: Vec<DocId>,
    failed: bool,
}

impl<'a> Call<'a> {
    /// A run from the command line. `recent` lists the recent projects, to
    /// name the one that holds a file used without the project.
    pub fn command_line(
        session: &'a mut Session,
        command: &Command,
        args: Map<String, Value>,
        recent: &'a [PathBuf],
        answers: SaveAnswers,
    ) -> Self {
        let mut call = Self::new(session, command, args, false, true, recent);
        call.answers = answers;
        call
    }

    /// A run in a live session, writable or not.
    pub fn live(
        session: &'a mut Session,
        command: &Command,
        args: Map<String, Value>,
        writable: bool,
        recent: &'a [PathBuf],
    ) -> Self {
        Self::new(session, command, args, true, writable, recent)
    }

    fn new(
        session: &'a mut Session,
        command: &Command,
        args: Map<String, Value>,
        live: bool,
        writable: bool,
        recent: &'a [PathBuf],
    ) -> Self {
        Self {
            session,
            command: command.name,
            args,
            live,
            writable,
            recent,
            answers: SaveAnswers::default(),
            warnings: Vec::new(),
            edited: Vec::new(),
            failed: false,
        }
    }

    /// Save the new document `doc` to the new file `path`. On the command line
    /// the save uses the run's answers; in a live session it asks nothing and
    /// refuses where a question would be needed.
    pub fn save_new(&mut self, doc: DocId, path: &Path) -> Result<()> {
        let warnings = self.answers.save(self.session, doc, Some(path))?;
        self.warnings.extend(warnings);
        Ok(())
    }

    /// The command works on `doc`: on the command line it is saved to its
    /// file afterwards if it changed. Document arguments and files the command
    /// opens through [`Self::open`] are included already. A document a shared
    /// function edits only to save it elsewhere, such as a combined workspace
    /// made from its first source, must not be listed.
    pub fn edits(&mut self, doc: DocId) {
        if !self.edited.contains(&doc) {
            self.edited.push(doc);
        }
    }

    /// Save each document the command works on that changed, with the run's
    /// answers: what the command line does after a command.
    pub fn save_edited(&mut self) -> Result<()> {
        for doc in self.edited.clone() {
            if self.session.needs_save(doc)? {
                let warnings = self.answers.save(self.session, doc, None)?;
                self.warnings.extend(warnings);
            }
        }
        Ok(())
    }

    /// The warnings of the saves that went ahead.
    pub fn warnings(&self) -> &[crate::session::SaveWarning] {
        &self.warnings
    }

    /// The command ran and its answer is no, such as a verification that
    /// found problems. The command line exits with a failure status.
    pub fn fail(&mut self) {
        self.failed = true;
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Refuse when the session may not write. For commands that write only
    /// with some arguments.
    pub fn require_writes(&self) -> Result<()> {
        if self.writable {
            return Ok(());
        }
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "This server is read-only; start it with --allow-writes to enable edits",
        )))
    }

    fn value(&self, name: &'static str) -> Option<&Value> {
        self.args.get(name).filter(|value| !value.is_null())
    }

    pub fn has(&self, name: &'static str) -> bool {
        self.value(name).is_some()
    }

    pub fn text(&self, name: &'static str) -> Result<&str> {
        self.optional_text(name)?.ok_or_else(|| missing(name))
    }

    pub fn optional_text(&self, name: &'static str) -> Result<Option<&str>> {
        self.value(name)
            .map(|value| value.as_str().ok_or_else(|| wrong(name, "text")))
            .transpose()
    }

    /// Several texts; none when left out.
    pub fn texts(&self, name: &'static str) -> Result<Vec<String>> {
        self.json::<Vec<String>>(name)
            .map(Option::unwrap_or_default)
    }

    pub fn number(&self, name: &'static str) -> Result<u64> {
        self.optional_number(name)?.ok_or_else(|| missing(name))
    }

    pub fn optional_number(&self, name: &'static str) -> Result<Option<u64>> {
        self.value(name)
            .map(|value| value.as_u64().ok_or_else(|| wrong(name, "a whole number")))
            .transpose()
    }

    pub fn flag(&self, name: &'static str) -> Result<bool> {
        self.value(name).map_or(Ok(false), |value| {
            value.as_bool().ok_or_else(|| wrong(name, "true or false"))
        })
    }

    /// A JSON argument read as `T`; `None` when left out.
    pub fn json<T: DeserializeOwned>(&self, name: &'static str) -> Result<Option<T>> {
        self.value(name)
            .map(|value| {
                serde_json::from_value(value.clone())
                    .map_err(|error| wrong(name, &format!("as its schema describes: {error}")))
            })
            .transpose()
    }

    pub fn required_json<T: DeserializeOwned>(&self, name: &'static str) -> Result<T> {
        self.json(name)?.ok_or_else(|| missing(name))
    }

    /// The document argument `name`. On the command line the file is opened
    /// once per run.
    pub fn doc(&mut self, name: &'static str) -> Result<DocId> {
        let value = self.value(name).ok_or_else(|| missing(name))?;
        if self.live {
            let id = value
                .as_u64()
                .and_then(|id| usize::try_from(id).ok())
                .ok_or_else(|| wrong(name, "a document number"))?;
            if !self.session.is_open(DocId(id)) {
                return Err(Error::UnknownDocument { id: DocId(id) });
            }
            return Ok(DocId(id));
        }
        let text = value
            .as_str()
            .ok_or_else(|| wrong(name, "a file"))?
            .to_owned();
        let path = self.resolve(&text)?;
        self.open(&path)
    }

    /// The document open at `path` (canonical), opening it when none is. The
    /// command works on it ([`Self::edits`]).
    pub fn open(&mut self, path: &Path) -> Result<DocId> {
        let doc = match open_at(self.session, path)? {
            Some(doc) => doc,
            None => self.session.open(path)?,
        };
        self.edits(doc);
        Ok(doc)
    }

    pub fn path(&self, name: &'static str) -> Result<PathBuf> {
        self.optional_path(name)?.ok_or_else(|| missing(name))
    }

    pub fn optional_path(&self, name: &'static str) -> Result<Option<PathBuf>> {
        self.optional_text(name)?
            .map(|text| self.resolve(text))
            .transpose()
    }

    pub fn paths(&self, name: &'static str) -> Result<Vec<PathBuf>> {
        self.texts(name)?
            .iter()
            .map(|text| self.resolve(text))
            .collect()
    }

    /// A path as the route reads it. One that exists is made canonical; a new
    /// one keeps its name in its parent's canonical folder. In a live session
    /// the path is relative to the project and must stay inside it.
    pub fn resolve(&self, text: &str) -> Result<PathBuf> {
        let path = if self.live {
            self.session.root().join(text)
        } else {
            std::path::absolute(text)?
        };
        let resolved = if path.try_exists()? {
            path.canonicalize()?
        } else {
            let name = path
                .file_name()
                .ok_or_else(|| Error::Invalid(format!("{text} has no file name")))?;
            match path.parent().map(Path::canonicalize) {
                Some(Ok(parent)) => parent.join(name),
                Some(Err(_)) | None if !self.live => path.clone(),
                Some(Err(error)) => return Err(error.into()),
                None => return Err(Error::Invalid(format!("{text} has no folder"))),
            }
        };
        if self.live && !resolved.starts_with(self.session.root()) {
            // D16: name the recent project that holds it, without choosing it.
            let holder = crate::project::Project::recent_holding(
                &resolved,
                self.session.project(),
                self.recent,
            )
            .map(|p| {
                format!(
                    "; it is in the recent project {}, so start the session there to use it",
                    p.root().display()
                )
            })
            .unwrap_or_default();
            return Err(Error::Invalid(format!(
                "Path lies outside the configured project{holder}"
            )));
        }
        Ok(resolved)
    }

    /// The project the session works in; the command refuses without one.
    pub fn project(&self) -> Result<crate::project::Project> {
        self.session
            .project()
            .cloned()
            .ok_or(Error::NoProject(self.command))
    }

    /// What a client sees of document `doc`: its number, file, kind, version
    /// and undo state, and what the kind adds.
    pub fn state(&self, doc: DocId) -> Result<Value> {
        state(self.session, doc)
    }

    /// The state of `doc` with `extra`'s fields added.
    pub fn state_with(&self, doc: DocId, extra: Value) -> Result<Value> {
        let mut state = self.state(doc)?;
        if let (Some(state), Value::Object(extra)) = (state.as_object_mut(), extra) {
            state.extend(extra);
        }
        Ok(state)
    }
}

/// Every document open in `session`.
pub fn open_documents(session: &Session) -> impl Iterator<Item = DocId> + '_ {
    (0..).map(DocId).take_while(|doc| session.is_open(*doc))
}

/// The document open at `path` (canonical) in `session`, if any.
pub fn open_at(session: &Session, path: &Path) -> Result<Option<DocId>> {
    for doc in open_documents(session) {
        let file = session.root().join(session.doc_key(doc)?);
        if file.canonicalize().unwrap_or(file) == path {
            return Ok(Some(doc));
        }
    }
    Ok(None)
}

fn missing(name: &'static str) -> Error {
    Error::MissingArgument(name)
}

fn wrong(name: &str, expected: &str) -> Error {
    Error::WrongArgument {
        name: name.to_owned(),
        expected: expected.to_owned(),
    }
}

/// What a client sees of document `doc`.
pub fn state(session: &Session, doc: DocId) -> Result<Value> {
    use basecraft_protocol::facts::Alphabet;
    let kind = if session.sanger_review(doc).is_ok() {
        "sanger_review"
    } else if session.is_alignment(doc) {
        "alignment"
    } else if session.is_tree(doc) {
        "tree"
    } else if session.is_delimitation(doc) {
        "delimitation"
    } else if session.is_metadata(doc) {
        "metadata"
    } else if session.is_submission(doc) {
        "submission"
    } else {
        "sequence"
    };
    let mut state = json!({
        "document": doc.0,
        "path": session.doc_key(doc)?,
        "kind": kind,
        "version": session.revision(doc)?,
        "generation": session.generation(doc)?,
        "state": session.state(doc)?,
        "can_undo": session.can_undo(doc)?,
        "can_redo": session.can_redo(doc)?,
    });
    if let Ok(alphabet) = session.alphabet(doc) {
        // D12: per-sequence values when the sequences are not of one kind.
        if matches!(alphabet.value, Alphabet::Mixed | Alphabet::Undetermined) {
            state["sequence_alphabets"] = json!(session.sequence_alphabets(doc)?);
        }
        state["alphabet"] = json!(alphabet);
    }
    if let Ok(tree) = session.tree(doc) {
        state["displaced_labels"] = json!(tree.displaced_labels());
    }
    if let Ok(review) = session.sanger_review(doc) {
        state["call_sets"] = json!(review.call_sets());
        state["selected_call_set"] = json!(review.draft_sha256);
        state["decisions"] = json!(
            (0..review.draft.columns.len())
                .map(|i| review.decision(i))
                .collect::<Vec<_>>()
        );
    }
    Ok(state)
}

/// JSON a person wrote: the text, `@FILE` for a file's contents, or `-` for
/// standard input. Large reviewed plans exceed the system's argument limits.
pub fn json_text(text: &str) -> Result<Value> {
    let text = if text == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else if let Some(path) = text.strip_prefix('@') {
        std::fs::read_to_string(path)?
    } else {
        text.to_owned()
    };
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_every_argument_is_described() {
        let commands: Vec<_> = all().collect();
        let mut tools: Vec<_> = commands.iter().map(|c| tool_name(c.name)).collect();
        tools.sort();
        let count = tools.len();
        tools.dedup();
        assert_eq!(tools.len(), count, "two commands share a tool name");
        for command in commands {
            assert!(!command.help.is_empty(), "{} has no help", command.name);
            for arg in command.args {
                assert!(
                    !arg.help.is_empty(),
                    "{} {} has no help",
                    command.name,
                    arg.name
                );
            }
        }
    }
}
