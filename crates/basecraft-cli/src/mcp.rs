//! Initial local MCP stdio server, implementing the 2025-11-25 protocol.
//! Documents remain in one headless session. This does not attach to a GUI.
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Actor, Op};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, btree_map::Entry},
    io::Write,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Who drives this MCP session. Edits are recorded with the matching actor
/// kind; the client name is also the initiator of operation runs.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum ClientKind {
    /// An assistant (the default for external MCP clients).
    #[default]
    Agent,
    /// A script, such as the Python `Session`.
    Script,
}

impl ClientKind {
    fn actor(self, client: &str) -> Actor {
        match self {
            Self::Agent => Actor::Agent {
                model: client.into(),
            },
            Self::Script => Actor::Script {
                client: client.into(),
            },
        }
    }
}

struct Server {
    session: Session,
    documents: HashMap<usize, DocId>,
    writable: bool,
    initialized: bool,
}

fn text<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing string {name}").into())
}

impl Server {
    fn path(&self, path: &str) -> basecraft_ops::Result<PathBuf> {
        let path = self.session.root().join(path);
        let resolved = if path.try_exists()? {
            path.canonicalize()?
        } else {
            path.parent()
                .ok_or_else(|| basecraft_ops::Error::Invalid("Missing parent".into()))?
                .canonicalize()?
                .join(
                    path.file_name()
                        .ok_or_else(|| basecraft_ops::Error::Invalid("Missing filename".into()))?,
                )
        };
        if !resolved.starts_with(self.session.root()) {
            return Err(basecraft_ops::Error::Invalid(
                "Path lies outside the configured project".into(),
            ));
        }
        Ok(resolved)
    }

    fn doc(&self, args: &Value) -> Result<DocId> {
        let id = args["document"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("Invalid document ID")?;
        self.documents
            .get(&id)
            .copied()
            .ok_or_else(|| basecraft_ops::Error::UnknownDocument { id: DocId(id) }.into())
    }

    fn state(&self, doc: DocId) -> Result<Value> {
        let kind = if self.session.sanger_review(doc).is_ok() {
            "sanger_review"
        } else if self.session.is_alignment(doc) {
            "alignment"
        } else if self.session.is_tree(doc) {
            "tree"
        } else if self.session.is_delimitation(doc) {
            "delimitation"
        } else if self.session.is_metadata(doc) {
            "metadata"
        } else if self.session.is_submission(doc) {
            "submission"
        } else {
            "sequence"
        };
        let version = self.session.revision(doc)?;
        let mut state = json!({"document":doc.0,"path":self.session.doc_key(doc)?,"kind":kind,"version":version,"generation":self.session.generation(doc)?,
            "state":self.session.state(doc)?,"can_undo":self.session.can_undo(doc)?,"can_redo":self.session.can_redo(doc)?});
        if let Ok(review) = self.session.sanger_review(doc) {
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

    fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
        let creates = (name == "metadata_open" && args["create"].as_bool() == Some(true))
            || (name == "submission_submitter" && args.get("set").is_some())
            || (name == "submission_inclusion_presets" && args.get("save").is_some());
        if (matches!(
            name,
            "apply"
                | "save"
                | "delimitation_combine"
                | "import_support"
                | "extract"
                | "undo"
                | "redo"
                | "metadata_template"
                | "submission_create"
                | "sequence_create"
                | "submission_export"
                | "submission_validation_request"
                | "submission_record_validation"
                | "submission_accession_table"
        ) || creates)
            && !self.writable
        {
            if name == "save" {
                return Err(basecraft_ops::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "This server is read-only; start it with --allow-writes to enable edits",
                ))
                .into());
            }
            return Err(
                "permission_denied: This server is read-only; start it with --allow-writes to enable edits".into(),
            );
        }
        match name {
            "open" => {
                let path = self.path(text(args, "path")?)?;
                let doc = self.session.open(&path)?;
                self.documents.insert(doc.0, doc);
                self.state(doc)
            }
            "sequence_create" => {
                let doc = self
                    .session
                    .create_sequence(text(args, "name")?, text(args, "sequence")?)?;
                self.documents.insert(doc.0, doc);
                self.state(doc)
            }
            "documents" => {
                let mut docs: Vec<_> = self.documents.values().copied().collect();
                docs.sort_by_key(|d| d.0);
                Ok(json!(
                    docs.into_iter()
                        .map(|d| self.state(d))
                        .collect::<Result<Vec<_>>>()?
                ))
            }
            "state" => self.state(self.doc(args)?),
            "document_record" => Ok(serde_json::to_value(
                self.session.document_record(self.doc(args)?)?.report()?,
            )?),
            // The same report as `basecraft history --json`, including saved Sanger reviews.
            "file_record" => Ok(serde_json::to_value(
                basecraft_ops::records::DocumentRecord::saved_report(
                    &self.path(text(args, "path")?)?,
                    self.session.project(),
                )?,
            )?),
            // The same readers as `basecraft provenance inspect|verify` and `replay`.
            "provenance_inspect" => Ok(serde_json::to_value(basecraft_ops::records::inspect(
                &self.path(text(args, "path")?)?,
                self.session.project(),
            )?)?),
            "provenance_verify" => {
                let issues = basecraft_ops::records::verify(
                    &self.path(text(args, "path")?)?,
                    self.session.project(),
                )?;
                Ok(json!({"verified":issues.is_empty(),"issues":issues}))
            }
            "replay" => {
                let report = basecraft_ops::records::replay(self.session.root())?;
                Ok(
                    json!({"sessions":report.sessions,"entries":report.entries,"problems":report.problems}),
                )
            }
            "read" => {
                let doc = self.doc(args)?;
                if self.session.is_tree(doc) {
                    return Ok(
                        json!({"tips":self.session.tree(doc)?.nodes().iter().filter(|n|n.is_tip()).map(|n|&n.name).collect::<Vec<_>>(), "style":self.session.tree(doc)?.style()}),
                    );
                }
                if self.session.is_delimitation(doc) {
                    return Ok(serde_json::to_value(self.session.delimitation(doc)?)?);
                }
                if self.session.is_metadata(doc) {
                    return Ok(
                        json!({"metadata":self.session.metadata(doc)?,"notices":self.session.metadata_notices(doc)?}),
                    );
                }
                if self.session.is_submission(doc) {
                    return Ok(serde_json::to_value(self.session.submission(doc)?)?);
                }
                let start = args["start"].as_u64().unwrap_or(0);
                let end = args["end"]
                    .as_u64()
                    .ok_or("Provide an exclusive end coordinate")?;
                if end < start || end - start > 1_000_000 || end > self.session.state(doc)?.len {
                    return Err("Invalid range or more than one million residues requested".into());
                }
                let residues = if self.session.is_alignment(doc) {
                    let row = args["row"]
                        .as_u64()
                        .and_then(|r| usize::try_from(r).ok())
                        .ok_or("Provide an alignment row")?;
                    if row >= self.session.alignment(doc)?.row_count() {
                        return Err("Row outside alignment".into());
                    }
                    self.session.alignment(doc)?.read(row, start, end)
                } else {
                    self.session.doc(doc)?.read(start, end)
                };
                Ok(json!({"residues":String::from_utf8_lossy(&residues)}))
            }
            "apply" => {
                let doc = self.doc(args)?;
                let version = args["version"]
                    .as_u64()
                    .ok_or("Provide the document version you inspected")?;
                if self.state(doc)?["version"] != version {
                    return Err(
                        "stale_revision: Document version changed; inspect it again before editing"
                            .into(),
                    );
                }
                let op: Op = serde_json::from_value(args["operation"].clone())?;
                self.session.apply(doc, op)?;
                self.state(doc)
            }
            "extract" => {
                let doc = self.doc(args)?;
                let version = args["version"]
                    .as_u64()
                    .ok_or("Provide the document version you inspected")?;
                if version != self.session.revision(doc)? {
                    return Err("stale_revision: Document version changed; inspect it again before extraction".into());
                }
                let target = self.path(text(args, "path")?)?;
                if target.try_exists()? {
                    return Err("Extraction destination already exists; choose a new file".into());
                }
                let start = args["start"].as_u64().ok_or("Provide start")?;
                let end = args["end"].as_u64().ok_or("Provide end")?;
                let segments: Vec<(u64, u64)> = serde_json::from_value(
                    args.get("segments").cloned().unwrap_or_else(|| json!([])),
                )?;
                self.session.extract(
                    doc,
                    (start, end),
                    &segments,
                    args["reverse"].as_bool().unwrap_or(false),
                    &target,
                )?;
                Ok(json!({"output":target,"hash":basecraft_io::hash_file(&target)?}))
            }
            "delimitation_linked_alignment" => Ok(json!({
                "path": basecraft_ops::delimitation::linked_alignment(&self.session, self.doc(args)?)?
            })),
            "delimitation_candidates" | "delimitation_name" | "delimitation_combine" => {
                let paths: Vec<String> = serde_json::from_value(
                    args.get("paths").cloned().ok_or("Provide input paths")?,
                )?;
                let paths = paths
                    .iter()
                    .map(|path| self.path(path))
                    .collect::<basecraft_ops::Result<Vec<_>>>()?;
                if name == "delimitation_candidates" {
                    return Ok(serde_json::to_value(
                        basecraft_ops::delimitation::combine_candidates(&paths)?,
                    )?);
                }
                if name == "delimitation_name" {
                    return Ok(json!({"path": basecraft_ops::delimitation::combined_path(&paths)}));
                }
                let output = self.path(text(args, "output")?)?;
                let keep: Option<basecraft_ops::delimitation::CombineSelection> = args
                    .get("selection")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?;
                let keep = keep
                    .map(|keep| -> Result<basecraft_ops::delimitation::CombineSelection> {
                        let mut selected = basecraft_ops::delimitation::CombineSelection::new();
                        for (path, ids) in keep {
                            let text = path.to_str().ok_or("Selection path must be UTF-8")?;
                            self.path(text)?;
                            // Joining relative and absolute spellings can collide
                            // before shared canonicalization sees either alias.
                            match selected.entry(self.session.root().join(path)) {
                                Entry::Vacant(entry) => { entry.insert(ids); }
                                Entry::Occupied(_) => return Err(
                                    "Hypothesis selection names the same input through multiple path aliases".into()
                                ),
                            }
                        }
                        Ok(selected)
                    })
                    .transpose()?;
                let doc = basecraft_ops::delimitation::combine_to(
                    &mut self.session,
                    &paths,
                    &output,
                    keep.as_ref(),
                )?;
                self.documents.insert(doc.0, doc);
                Ok(json!({"output":output,"document":doc.0}))
            }
            "import_support" => {
                let doc = self.doc(args)?;
                let source = self.path(text(args, "path")?)?;
                let name = args.get("name").and_then(Value::as_str).unwrap_or("");
                let imported = self.session.import_support(doc, &source, name)?;
                let mut state = self.state(doc)?;
                state["added"] = json!(imported.added);
                state["matched"] = json!(imported.matched);
                Ok(state)
            }
            "undo" | "redo" => {
                let doc = self.doc(args)?;
                if name == "undo" {
                    self.session.undo(doc)?;
                } else {
                    self.session.redo(doc)?;
                }
                self.state(doc)
            }
            "save" | "save_check" => {
                let doc = self.doc(args)?;
                let path = args
                    .get("path")
                    .map(|value| {
                        let p = value.as_str().ok_or_else(|| {
                            basecraft_ops::Error::Invalid("path must be a string".into())
                        })?;
                        self.path(p)
                    })
                    .transpose()?;
                if name == "save_check" {
                    return Ok(serde_json::to_value(
                        self.session.check_save(doc, path.as_deref())?,
                    )?);
                }
                // No overwrite confirmation exists here: retain save_new's rule.
                let warnings = if let Some(value) = args.get("decision") {
                    let choice: crate::save::Decision = serde_json::from_value(value.clone())
                        .map_err(basecraft_ops::Error::Json)?;
                    let decision = self.session.decide_save(
                        doc,
                        path.as_deref(),
                        &choice.token,
                        choice.known_policy,
                        choice.acknowledge_incomplete,
                    )?;
                    self.session
                        .save_new_with_decision(doc, path.as_deref(), decision)?
                        .warnings
                } else {
                    self.session.save_new(doc, path.as_deref())?;
                    Vec::new()
                };
                let mut state = self.state(doc)?;
                state["warnings"] = json!(warnings);
                Ok(state)
            }
            // Project specimen metadata: the same planners as `basecraft metadata`.
            "metadata_open" => {
                let create = args["create"].as_bool().unwrap_or(false);
                if let Some(doc) = self
                    .documents
                    .values()
                    .copied()
                    .find(|d| self.session.is_metadata(*d))
                {
                    return self.state(doc);
                }
                let doc = basecraft_ops::metadata::open(&mut self.session, create)?.ok_or(
                    "no_metadata: This project has no metadata yet; call metadata_open with create",
                )?;
                self.documents.insert(doc.0, doc);
                let mut state = self.state(doc)?;
                state["notices"] = json!(self.session.metadata_notices(doc)?);
                Ok(state)
            }
            "metadata_plan_headers" => {
                let doc = self.doc(args)?;
                let mut request: basecraft_ops::metadata::HeaderImport =
                    serde_json::from_value(args["request"].clone())?;
                request.document = self.relative(&request.document)?;
                Ok(serde_json::to_value(
                    basecraft_ops::metadata::plan_header_import(&self.session, doc, &request)?,
                )?)
            }
            "metadata_plan_sheet" => {
                let doc = self.doc(args)?;
                let mut request: basecraft_ops::metadata::SheetImport =
                    serde_json::from_value(args["request"].clone())?;
                request.path = self.path(&request.path)?.to_string_lossy().into_owned();
                request.document = request
                    .document
                    .as_deref()
                    .map(|d| self.relative(d))
                    .transpose()?;
                Ok(serde_json::to_value(
                    basecraft_ops::metadata::plan_sheet_import(&self.session, doc, &request)?,
                )?)
            }
            "metadata_template" => {
                let doc = self.doc(args)?;
                let file = self.relative(text(args, "file")?)?;
                let output = self.path(text(args, "output")?)?;
                let records =
                    basecraft_ops::metadata::template_for(&self.session, doc, &file, &output)?;
                Ok(json!({"records":records,"output":output}))
            }
            "metadata_status" => {
                let doc = self.doc(args)?;
                let file = self.relative(text(args, "file")?)?;
                Ok(serde_json::to_value(basecraft_ops::metadata::link_status(
                    &self.session,
                    doc,
                    &file,
                )?)?)
            }
            "metadata_consumer" => {
                let doc = self.doc(args)?;
                let request = serde_json::from_value(args["request"].clone())?;
                Ok(basecraft_ops::metadata::consumer_plan(
                    &self.session,
                    doc,
                    &request,
                )?)
            }
            "metadata_insdc" => {
                let doc = self.doc(args)?;
                Ok(basecraft_ops::metadata::insdc(
                    &self.session,
                    doc,
                    text(args, "specimen")?,
                )?)
            }
            // GenBank marker submissions: the same functions as `basecraft submission`.
            "submission_create" => {
                let path = self.path(text(args, "path")?)?;
                let alignment = self.path(text(args, "alignment")?)?;
                let doc = basecraft_ops::submission::create(&mut self.session, &path, &alignment)?;
                self.documents.insert(doc.0, doc);
                self.state(doc)
            }
            "submission_inclusion_preview" => {
                let doc = self.doc(args)?;
                let rule =
                    serde_json::from_value(args.get("rule").cloned().ok_or("missing rule")?)?;
                Ok(serde_json::to_value(
                    basecraft_ops::submission::plan_inclusion(&self.session, doc, &rule)?,
                )?)
            }
            "submission_inclusion_presets" => {
                if let Some(rule) = args.get("save") {
                    basecraft_ops::submission::save_inclusion_preset(
                        &mut self.session,
                        serde_json::from_value(rule.clone())?,
                    )?;
                }
                let (meta, _) = basecraft_ops::submission::project_metadata(self.session.root())?;
                Ok(serde_json::to_value(meta.import.inclusion_rules)?)
            }
            "submission_review" => {
                let doc = self.doc(args)?;
                let submitter = basecraft_ops::submission::submitter_preference()?;
                Ok(serde_json::to_value(basecraft_ops::submission::review(
                    &self.session,
                    doc,
                    &submitter,
                )?)?)
            }
            "submission_export" => {
                let doc = self.doc(args)?;
                let output = self.path(text(args, "output")?)?;
                let submitter = basecraft_ops::submission::submitter_preference()?;
                let summary =
                    basecraft_ops::submission::export(&mut self.session, doc, &output, &submitter)?;
                Ok(serde_json::to_value(summary)?)
            }
            "submission_validation_request" => {
                let package = self.path(text(args, "package")?)?;
                Ok(basecraft_ops::submission::validation_request(
                    self.session.root(),
                    &package,
                )?)
            }
            "submission_record_validation" => {
                let package = self.path(text(args, "package")?)?;
                Ok(serde_json::to_value(
                    basecraft_ops::submission::record_validation(
                        self.session.root(),
                        &package,
                        &args["outcome"],
                    )?,
                )?)
            }
            "submission_plan_accessions" => {
                let doc = self.doc(args)?;
                let submitter = basecraft_ops::submission::submitter_preference()?;
                Ok(serde_json::to_value(
                    basecraft_ops::submission::plan_accessions(
                        &self.session,
                        doc,
                        text(args, "text")?,
                        &submitter,
                    )?,
                )?)
            }
            "submission_accession_table" => {
                let doc = self.doc(args)?;
                let output = self.path(text(args, "output")?)?;
                let submitter = basecraft_ops::submission::submitter_preference()?;
                let table =
                    basecraft_ops::submission::accession_table(&self.session, doc, &submitter)?;
                basecraft_io::write_atomic(&output, |w| w.write_all(&table))?;
                Ok(json!({"output":output}))
            }
            "submission_submitter" => {
                let paths = basecraft_platform::paths::AppPaths::discover()?;
                let store = basecraft_platform::settings::SettingsStore::new(paths.preferences());
                let mut submitter = store.load()?.submitter;
                if let Some(set) = args.get("set") {
                    let mut value = serde_json::to_value(&submitter)?;
                    for (name, field) in set.as_object().ok_or("set must be an object")? {
                        if value.get(name).is_none() {
                            return Err(format!("{name} is not a submitter field").into());
                        }
                        value[name] = field.clone();
                    }
                    submitter = serde_json::from_value(value)?;
                    store.update(basecraft_platform::settings::Change::Submitter(Box::new(
                        submitter.clone(),
                    )))?;
                }
                Ok(json!({"submitter":submitter,"missing":submitter.missing()}))
            }
            _ => Err("Unknown tool".into()),
        }
    }

    /// A project file's project-relative key, refusing paths outside it.
    fn relative(&self, path: &str) -> Result<String> {
        let path = self.path(path)?;
        Ok(basecraft_ops::metadata::document_key(
            self.session.root(),
            &path,
        )?)
    }
}

/// Keep save refusals typed while retaining the existing MCP error envelope.
fn call_error(name: &str, error: &(dyn std::error::Error + 'static)) -> Value {
    if let Some(error) = error
        .downcast_ref::<basecraft_ops::Error>()
        .filter(|_| matches!(name, "save" | "save_check"))
    {
        let mut value = crate::save::error_value(error);
        value["schema"] = json!(1);
        value["operation"] = json!(name);
        value["errors"][0]["operation"] = json!(name);
        return value;
    }
    basecraft_plugin_host::jobs::error_value(name, core_error_code(error), error.to_string())
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut definition = json!({"name":name,"description":description,"inputSchema":{"type":"object","required":required,"additionalProperties":false}});
    definition["inputSchema"]["properties"] = properties;
    definition
}

fn tools() -> Value {
    let doc = json!({"type":"integer","minimum":0});
    let string = json!({"type":"string"});
    let mut operation = serde_json::to_value(schemars::schema_for!(Op)).expect("schema serializes");
    let definitions = operation.as_object_mut().and_then(|o| o.remove("$defs"));
    let mut apply = tool(
        "apply",
        "Apply one typed operation to the inspected document version; logged and undoable.",
        json!({"document":doc,"version":{"type":"integer","minimum":0},"operation":operation}),
        &["document", "version", "operation"],
    );
    if let Some(definitions) = definitions {
        apply["inputSchema"]["$defs"] = definitions;
    }
    json!({"tools":[
        tool("open","Open a document within the project.",json!({"path":string}), &["path"]),
        tool("documents","List this headless session's open documents.",json!({}), &[]),
        tool("document_record","Read history and methods for this headless session current revision, including unsaved edits.",json!({"document":doc}), &["document"]),
        tool("state","Inspect document dimensions, version and undo state.",json!({"document":doc}), &["document"]),
        tool("read","Read a residue range or list tree tips.",json!({"document":doc,"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"row":{"type":"integer","minimum":0}}), &["document"]),
        apply,
        tool("undo","Undo one edit group.",json!({"document":doc}), &["document"]),
        tool("redo","Redo one edit group.",json!({"document":doc}), &["document"]),
        tool("save","Save a document, optionally to a new project path. An existing other file is never replaced.",json!({"document":doc,"path":string,"decision":{
            "type":"object","additionalProperties":false,
            "required":["token","known_policy","acknowledge_incomplete"],
            "properties":{"token":{"type":"string"},"known_policy":{"type":"string","enum":["cancel","preserve_copy","break_evidence","remove_analysis"]},"acknowledge_incomplete":{"type":"boolean"}}
        }}), &["document"]),
        tool("save_check","Inspect a save without writing. Returns a single-attempt token owned by this live Session; newer checks supersede it. Known policy and incomplete acknowledgement are separate.",json!({"document":doc,"path":string}), &["document"]),
        tool("file_record","Read history and methods for a saved project file as it is on disk, including saved Sanger reviews. Same output as `basecraft history --json`; writes nothing.",json!({"path":string}), &["path"]),
        tool("extract","Extract the inspected sequence revision to a new GenBank file with source ancestry. Coordinates are zero-based, half-open; optional splice segments are in biological order, followed by optional reverse complement. Existing destinations are refused.",json!({"document":doc,"version":{"type":"integer","minimum":0},"path":string,"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"segments":{"type":"array","items":{"type":"array","items":{"type":"integer","minimum":0},"minItems":2,"maxItems":2}},"reverse":{"type":"boolean"}}), &["document","version","path","start","end"]),
        tool("delimitation_linked_alignment","Inspect the loaded species workspace's unverified alignment hint. Returns path:string|null, preserving an explicit empty string; does not read or validate that path or authorize analysis.",json!({"document":doc}), &["document"]),
        tool("delimitation_candidates","Inspect saved species hypotheses in recommendation order, without writing records.",json!({"paths":{"type":"array","items":{"type":"string"}}}), &["paths"]),
        tool("delimitation_name","Suggest a free combined SPART filename beside the last input; writes nothing.",json!({"paths":{"type":"array","items":{"type":"string"}}}), &["paths"]),
        tool("delimitation_combine","Combine explicit saved hypotheses into a new file. Omitted selection keeps all; supplied selection maps input paths to hypothesis IDs, with missing paths keeping none. Existing output or companion is refused. Requires writes.",json!({"paths":{"type":"array","items":{"type":"string"},"minItems":2},"output":string,"selection":{"type":"object","additionalProperties":{"type":"array","items":{"type":"string"}}}}), &["paths","output"]),
        tool("import_support","Add the support values of another project tree file to an open tree as one undoable edit. The source is recorded as a referenced input. An empty or omitted name keeps the source's set names.",json!({"document":doc,"path":string,"name":string}), &["document","path"]),
        tool("provenance_inspect","Read the provenance publication of a saved project result.",json!({"path":string}), &["path"]),
        tool("provenance_verify","Check a saved result against its recorded hashes and retained inputs; runs no tools.",json!({"path":string}), &["path"]),
        tool("replay","Replay the project's operation log in a scratch session and report problems.",json!({}), &[]),
        tool("sequence_create", "Create an independent unsaved DNA sequence from plain IUPAC text. Save to a new .gb destination with save; edit with apply. Requires writes.", json!({"name":string,"sequence":string}), &["name","sequence"]),
        tool("metadata_open","Open the project's specimen metadata (roster, taxa, identifications, record links, typed collection columns) as a document. With create, write empty tables first if the project has none. Edit it with apply and metadata.edit operations; save it with save.",json!({"create":{"type":"boolean"}}), &[]),
        tool("metadata_plan_headers","Plan, without applying, an import from structured FASTA headers. request: {document: project FASTA path, pattern: {name, separator, fields: [{role: specimen|taxon|record|ignore|column, column?}]}, options?: {create_specimens, overwrite}}. Returns {changes, issues, rows, matched}; apply the reviewed changes with apply {op: metadata.edit, params: {edit: {action: apply, changes}}}.",json!({"document":doc,"request":{"type":"object"}}), &["document","request"]),
        tool("metadata_plan_sheet","Plan, without applying, an import from a spreadsheet (.ods, .xlsx, .csv, .tsv). request: {path, mapping?: {header: {role, column?}} (guessed when omitted), document?: FASTA whose records the record column names, options?}. Returns the plan with the mapping used.",json!({"document":doc,"request":{"type":"object"}}), &["document","request"]),
        tool("metadata_template","Write an .ods template, every cell text, pre-filled for each record of a project FASTA file.",json!({"document":doc,"file":string,"output":string}), &["document","file","output"]),
        tool("metadata_status","List a FASTA file's records that have no stored link to a specimen.",json!({"document":doc,"file":string}), &["document","file"]),
        tool("metadata_consumer","Inspect project partitions or metadata labels, or plan explicit workspace roster links/result promotion. request: {action: partitions} | {action: link_records, document: project path, links: {record ID: roster ID}} | {action: labels, document, records: [scientific ID], fields: [specimen|genus|species|accession|{column:{name}}]} | {action: link_workspace, workspace: project path, links: {workspace ID: roster ID}} | {action: promote_partition, workspace, hypothesis, id}. Plans return target and typed operation, applied through generic apply to the opened target at its current version, then save. Missing/conflicting roster links refuse promotion. Planning is read-only.",json!({"document":doc,"request":{"type":"object"}}), &["document","request"]),
        tool("metadata_insdc","Derive a specimen's INSDC source qualifiers from its Darwin Core columns and current identification, with the required ones still missing.",json!({"document":doc,"specimen":string}), &["document","specimen"]),
        tool("submission_create","Create a GenBank marker submission document (path ending .submission.json) for an aligned FASTA inside the project, and open it. Edit it with apply and submission.edit operations ({action: set_rows | set_batch | set_frames | set_export | set_publication | set_source | set_accessions | confirm_selection, ...}; see schemas/submission-edit.schema.json) and save it with save.",json!({"path":string,"alignment":string}), &["path","alignment"]),
        tool("submission_inclusion_preview", "Preview identifier inclusion using a typed rule (whole identifier or one-based literal field, exact/contains/starts/ends/wildcard/regex). Manual decisions are preserved by default. Missing/empty fields are unresolved. Apply the complete returned plan with submission.edit {action: apply_inclusion, plan: ...}; stale or modified plans are rejected atomically. Matching is not accession verification.", json!({"document":doc,"rule":{"type":"object"}}), &["document","rule"]),
        tool("submission_inclusion_presets", "Read project inclusion presets, or save a typed named rule using the shared metadata import settings.", json!({"save":{"type":"object"}}), &[]),
        tool("submission_review","Prepare a submission's records from its source alignment and the project metadata and list every local issue: extraction (ungapped, ambiguity kept, terminal unknowns, masked columns, export IDs), specimen source qualifiers, organism names, features, and reading frames detected per record for review (confirm them with set_frames). Accession-aware row choices and evidence are local unverified heuristics. Review included/excluded rows, then apply submission.edit with {action: confirm_selection, selection_hash: <returned selection_hash>} before export. Bulk include/exclude uses set_rows. Source/selection/evidence changes require confirmation again. Blocking issues keep the package from being ready.",json!({"document":doc}), &["document"]),
        tool("submission_export","Write the submission package (genbank/ .fsa .src .tbl .sbt, records, specimens, coordinate map, README, package.json) to a folder in the project. Requires confirmation of the current selection fingerprint from submission_review (apply submission.edit confirm_selection). Same inputs give identical files. Returns the input hash and status.",json!({"document":doc,"output":string}), &["document","output"]),
        tool("submission_validation_request","The OperationRequest that validates an exported package with table2asn. Submit it with the basecraft.table2asn.validate.v1 tool, wait with jobs_result, then pass the outcome to submission_record_validation.",json!({"package":string}), &["package"]),
        tool("submission_record_validation","Read a finished table2asn job's reports into the package: issues mapped to records and specimens, status (passed, errors, unavailable, failed), .sqn and .gbf copied to genbank/. Local validation only, not archive acceptance.",json!({"package":string,"outcome":{"type":"object"}}), &["package","outcome"]),
        tool("submission_plan_accessions","Match an accession list returned by NCBI (lines with a sequence ID and an accession) to the submission's records. Record the result with apply {op: submission.edit, params: {edit: {action: set_accessions, accessions}}}.",json!({"document":doc,"text":string}), &["document","text"]),
        tool("submission_accession_table","Write a publication table of records, specimens, organisms and accessions.",json!({"document":doc,"output":string}), &["document","output"]),
        tool("submission_submitter","Read the submitter contact preference used for .sbt files and the required fields still missing; with set (an object of fields such as first_name, last_name, email, institution, city, country), update it.",json!({"set":{"type":"object"}}), &[]),
    ]})
}

fn operation_tools(
    registry: &basecraft_plugin_host::registry::Registry,
    policy: &basecraft_plugin_host::policy::SessionPolicy,
) -> Result<Vec<Value>> {
    let mut definitions = tools()["tools"]
        .as_array()
        .ok_or("invalid built-in tool metadata")?
        .clone();
    for (name, description) in [
        (
            "basecraft.host.jobs_get.v1",
            "Inspect a job owned by this session.",
        ),
        (
            "basecraft.host.jobs_result.v1",
            "Read a terminal job result without executing it again.",
        ),
        (
            "basecraft.host.jobs_cancel.v1",
            "Explicitly cancel an owned job.",
        ),
        (
            "basecraft.host.jobs_repair.v1",
            "Retry retained partial publication; never reruns the plugin.",
        ),
    ] {
        let mut metadata = tool(
            name,
            description,
            json!({"job":{"type":"string"}}),
            &["job"],
        );
        metadata["outputSchema"] = outcome_schema()?;
        definitions.push(metadata);
    }
    definitions.push(tool(
        "basecraft.host.registry_refresh.v1",
        "Refresh installed metadata without running plugins.",
        json!({}),
        &[],
    ));
    definitions.push(tool(
        "basecraft.host.operations_list.v1",
        "List every installed operation with descriptor, pins, enabled state, origin and registry diagnostics, including operations this session cannot run. Same output as `basecraft operations list --json`.",
        json!({}),
        &[],
    ));
    definitions.push(tool(
        "basecraft.host.operations_describe.v1",
        "Describe one installed operation by ID (plugin/name@major). Same output as `basecraft operations describe`.",
        json!({"operation":{"type":"string"}}),
        &["operation"],
    ));
    for operation in registry
        .operations
        .values()
        .filter(|operation| policy.permits(operation) && operation.package.interpreter.is_some())
    {
        let mut schema = serde_json::to_value(schemars::schema_for!(
            basecraft_protocol::operation::OperationRequest
        ))?;
        schema["properties"]["operation"] = json!({"const":operation.id});
        schema["properties"]["parameters"] =
            basecraft_plugin_host::contract::embedded_parameters(&operation.descriptor.parameters)?;
        schema["required"] = json!([]);
        let mut metadata = json!({"name":basecraft_plugin_host::contract::mcp_name(&operation.package.manifest.id,&operation.descriptor)?,
            "description":operation.descriptor.description,"inputSchema":schema,"outputSchema":operation_outcome_schema(&operation.id, &operation.descriptor.result)?,
            "annotations":{"readOnlyHint":!operation.descriptor.effects.document_write && !operation.descriptor.effects.artifact_write,
                "destructiveHint":operation.descriptor.effects.document_write,"openWorldHint":true}});
        metadata["_meta"] = json!({"basecraft":{"pins":operation.pins(&registry.generation),"runtime":"unverified",
            "enabled":operation.package.enabled,"origin":operation.package.origin}});
        definitions.push(metadata);
    }
    Ok(definitions)
}

fn operation_outcome_schema(id: &str, result: &Value) -> Result<Value> {
    let mut envelope = outcome_schema()?;
    let definitions = envelope.as_object_mut().and_then(|o| o.remove("$defs"));
    let result =
        basecraft_plugin_host::contract::embedded_schema(result, "#/allOf/1/then/properties/data")?;
    let mut schema = json!({"type":"object", "properties":{"operation":{"const":id}},
        "allOf":[envelope, {"if":{"required":["state"],"properties":{"state":{"const":"completed"}}},
            "then":{"required":["data"],"properties":{"data":result}}}]});
    if let Some(definitions) = definitions {
        schema["$defs"] = definitions;
    }
    Ok(schema)
}

fn outcome_schema() -> Result<Value> {
    let mut outcome = serde_json::to_value(schemars::schema_for!(
        basecraft_protocol::operation::OperationOutcome
    ))?;
    let definitions = outcome.as_object_mut().and_then(|o| o.remove("$defs"));
    let mut schema = json!({"type":"object","anyOf":[outcome,{"type":"object","required":["schema","operation","state","errors"],
        "properties":{"schema":{"const":1},"operation":{"type":"string"},"state":{"const":"failed"},"errors":{"type":"array","items":{"$ref":"#/$defs/OperationError"}}}}]});
    if let Some(definitions) = definitions {
        schema["$defs"] = definitions;
    }
    Ok(schema)
}

// Keep core error kinds intact instead of classifying their display text.
fn core_error_code(
    error: &(dyn std::error::Error + 'static),
) -> basecraft_protocol::operation::OperationErrorCode {
    if let Some(error) = error.downcast_ref::<basecraft_ops::Error>() {
        return basecraft_plugin_host::jobs::ops_error_code(error);
    }
    if let Some(error) = error.downcast_ref::<basecraft_plugin_host::Error>() {
        return basecraft_plugin_host::jobs::error_code(error);
    }
    basecraft_plugin_host::jobs::error_code(&basecraft_plugin_host::Error::Protocol(
        error.to_string(),
    ))
}

fn structured(value: Value, failed: bool) -> Value {
    let mut result = json!({"content":[{"type":"text","text":value.to_string()}],"isError":failed});
    result["structuredContent"] = value;
    result
}

/// Read one newline-terminated MCP stdio frame, refusing frames over
/// `MAX_FRAME_BYTES`. The Python `Session` client enforces the same limit.
fn read_frame(input: &mut impl std::io::BufRead) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::{BufRead, Read};
    let mut bytes = Vec::new();
    let count = input
        .take((basecraft_protocol::operation::MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if count > basecraft_protocol::operation::MAX_FRAME_BYTES {
        return Err(std::io::Error::other("stdio frame exceeds byte limit"));
    }
    Ok(Some(bytes))
}

pub fn serve(root: &Path, writable: bool, kind: ClientKind, model: &str) -> Result<()> {
    use basecraft_plugin_host::{
        jobs::JobManager,
        policy::SessionPolicy,
        registry::{Registry, RegistryConfig},
    };
    use basecraft_protocol::operation::{OperationErrorCode, OperationRequest, RuntimeState};
    let project = basecraft_ops::project::Project::new(root)?;
    let mut session = Session::recoverable(project.root())?.in_project(Some(project));
    session.set_actor(kind.actor(model));
    let policy = SessionPolicy::new(root, writable)?;
    let config = RegistryConfig::discover()?;
    let mut registry = Registry::load(&config)?;
    let mut jobs = JobManager::default();
    let mut server = Server {
        session,
        documents: HashMap::new(),
        writable,
        initialized: false,
    };
    basecraft_platform::interrupt::install()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            match read_frame(&mut input) {
                Ok(Some(line)) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut output = std::io::stdout().lock();
    loop {
        jobs.poll_session(&mut server.session);
        if basecraft_platform::interrupt::interrupted() {
            break;
        }
        let line = match receiver.recv_timeout(std::time::Duration::from_millis(25)) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                writeln!(
                    output,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})
                )?;
                output.flush()?;
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        let parsed: std::result::Result<Value, _> = serde_json::from_slice(&line);
        let response = match parsed {
            Err(error) => {
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})
            }
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                if request["jsonrpc"] != "2.0"
                    || !request["method"].is_string()
                    || (!id.is_null() && !id.is_string() && !id.is_i64() && !id.is_u64())
                {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid JSON-RPC request"}})
                } else if request.get("id").is_none() {
                    // Submission has no bounded wait request. Cancellation of a
                    // completed request never cancels its accepted scientific job.
                    continue;
                } else {
                    let method = request["method"].as_str().unwrap_or("");
                    let mut unknown = None;
                    let result: std::result::Result<Value, (i64, String)> = (|| match method {
                        "initialize" => {
                            server.initialized = true;
                            Ok(
                                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"basecraft","version":env!("CARGO_PKG_VERSION")},"instructions":"Independent headless project session. Installed operation calls return owned jobs. Use jobs_get/result/cancel; request cancellation does not cancel accepted jobs. Writes require --allow-writes."}),
                            )
                        }
                        "ping" => Ok(json!({})),
                        _ if !server.initialized => Err((-32002, "Initialize first".into())),
                        "tools/list" => {
                            let definitions = operation_tools(&registry, &policy)
                                .map_err(|e| (-32603, e.to_string()))?;
                            let generation = basecraft_io::hash_bytes(
                                &serde_json::to_vec(&definitions)
                                    .map_err(|e| (-32603, e.to_string()))?,
                            );
                            let offset = if let Some(cursor) = request["params"]["cursor"].as_str()
                            {
                                let (pin, offset) = cursor
                                    .rsplit_once(':')
                                    .ok_or((-32602, "Invalid cursor".into()))?;
                                if pin != generation {
                                    return Err((
                                        -32602,
                                        "Stale tool generation; list again".into(),
                                    ));
                                }
                                offset
                                    .parse::<usize>()
                                    .map_err(|_| (-32602, "Invalid cursor offset".into()))?
                            } else {
                                0
                            };
                            if offset > definitions.len() {
                                return Err((-32602, "Cursor offset out of range".into()));
                            }
                            let end = offset.saturating_add(16).min(definitions.len());
                            let mut page = json!({"tools":&definitions[offset..end]});
                            if end < definitions.len() {
                                page["nextCursor"] = json!(format!("{generation}:{end}"));
                            }
                            Ok(page)
                        }
                        "tools/call" => {
                            let name = request["params"]["name"]
                                .as_str()
                                .ok_or((-32602, "Missing tool name".into()))?;
                            let args = request["params"]
                                .get("arguments")
                                .cloned()
                                .unwrap_or_else(|| json!({}));
                            if !args.is_object() {
                                return Err((-32602, "Tool arguments must be an object".into()));
                            }
                            if name == "basecraft.host.registry_refresh.v1" {
                                let previous = operation_tools(&registry, &policy)
                                    .map_err(|e| (-32603, e.to_string()))?;
                                registry =
                                    Registry::load(&config).map_err(|e| (-32603, e.to_string()))?;
                                if previous
                                    != operation_tools(&registry, &policy)
                                        .map_err(|e| (-32603, e.to_string()))?
                                {
                                    writeln!(output,"{}",json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})).map_err(|e| (-32603,e.to_string()))?;
                                }
                                return Ok(structured(
                                    json!({"generation":registry.generation}),
                                    false,
                                ));
                            }
                            if let Some(metadata) = match name {
                                "basecraft.host.operations_list.v1" => {
                                    Some(crate::operations::list(&registry))
                                }
                                "basecraft.host.operations_describe.v1" => Some(
                                    args["operation"]
                                        .as_str()
                                        .ok_or_else(|| "Missing string operation".into())
                                        .and_then(|id| crate::operations::describe(&registry, id)),
                                ),
                                _ => None,
                            } {
                                return Ok(match metadata {
                                    Ok(value) => structured(value, false),
                                    Err(error) => structured(
                                        basecraft_plugin_host::jobs::error_value(
                                            "basecraft.host",
                                            if error.to_string() == "unknown operation" {
                                                OperationErrorCode::PluginUnavailable
                                            } else {
                                                OperationErrorCode::InvalidArguments
                                            },
                                            error.to_string(),
                                        ),
                                        true,
                                    ),
                                });
                            }
                            let job = || {
                                args["job"].as_str().ok_or_else(|| {
                                    basecraft_plugin_host::Error::Protocol(
                                        "invalid_arguments: missing string job".into(),
                                    )
                                })
                            };
                            let job_result = match name {
                                "basecraft.host.jobs_get.v1" => {
                                    Some(job().and_then(|j| jobs.get(j)))
                                }
                                "basecraft.host.jobs_result.v1" => {
                                    Some(job().and_then(|j| jobs.result(j)))
                                }
                                "basecraft.host.jobs_cancel.v1" => {
                                    Some(job().and_then(|j| jobs.cancel(j)))
                                }
                                "basecraft.host.jobs_repair.v1" => {
                                    Some(job().and_then(|j| jobs.repair(j)))
                                }
                                _ => None,
                            };
                            if let Some(result) = job_result {
                                return Ok(match result {
                                    Ok(outcome) => {
                                        let failed = matches!(
                                            outcome.state,
                                            RuntimeState::Failed
                                                | RuntimeState::Partial
                                                | RuntimeState::Cancelled
                                        );
                                        structured(json!(outcome), failed)
                                    }
                                    Err(error) => structured(
                                        basecraft_plugin_host::jobs::error_value(
                                            "basecraft.host",
                                            basecraft_plugin_host::jobs::error_code(&error),
                                            error.to_string(),
                                        ),
                                        true,
                                    ),
                                });
                            }
                            if let Some(operation) = registry.operations.values().find(|op| {
                                basecraft_plugin_host::contract::mcp_name(
                                    &op.package.manifest.id,
                                    &op.descriptor,
                                )
                                .is_ok_and(|tool| tool == name)
                            }) {
                                let mut supplied = args;
                                if supplied
                                    .get("operation")
                                    .is_some_and(|id| id != &json!(operation.id))
                                {
                                    return Ok(structured(
                                        basecraft_plugin_host::jobs::error_value(
                                            &operation.id,
                                            OperationErrorCode::InvalidArguments,
                                            "tool operation identity mismatch",
                                        ),
                                        true,
                                    ));
                                }
                                supplied["operation"] = json!(operation.id);
                                if supplied.get("parameters").is_none() {
                                    supplied["parameters"] = json!({});
                                }
                                let result = serde_json::from_value::<OperationRequest>(supplied)
                                    .map_err(basecraft_plugin_host::Error::from)
                                    .and_then(|request| {
                                        jobs.submit_in_session(
                                            &registry,
                                            request,
                                            policy.clone(),
                                            model,
                                            &mut server.session,
                                        )
                                    });
                                return Ok(match result {
                                    Ok(outcome) => {
                                        if let Some(doc) = jobs.target_document(&outcome.run_id) {
                                            server.documents.insert(doc.0, doc);
                                        }
                                        structured(json!(outcome), false)
                                    }
                                    Err(error) => structured(
                                        basecraft_plugin_host::jobs::error_value(
                                            &operation.id,
                                            basecraft_plugin_host::jobs::error_code(&error),
                                            error.to_string(),
                                        ),
                                        true,
                                    ),
                                });
                            }
                            let known = tools()["tools"]
                                .as_array()
                                .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name));
                            if !known {
                                // An unknown operation-tool spelling carries the same
                                // structured error as CLI/Python unknown operations.
                                unknown = name
                                    .rsplit_once(".v")
                                    .and_then(|(rest, major)| {
                                        let (plugin, local) = rest.rsplit_once('.')?;
                                        major
                                            .parse::<u16>()
                                            .ok()
                                            .map(|major| format!("{plugin}/{local}@{major}"))
                                    })
                                    .and_then(|id| {
                                        registry.resolve(&id, None).err().map(|error| {
                                            basecraft_plugin_host::jobs::error_value(
                                                &id,
                                                basecraft_plugin_host::jobs::error_code(&error),
                                                error.to_string(),
                                            )
                                        })
                                    });
                                return Err((-32602, "Unknown tool".into()));
                            }
                            Ok(match server.call(name, &args) {
                                Ok(value) => structured(value, false),
                                Err(error) => structured(call_error(name, error.as_ref()), true),
                            })
                        }
                        _ => Err((-32601, "Method not found".into())),
                    })(
                    );
                    match result {
                        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                        Err((code, message)) => {
                            let mut error = json!({"code":code,"message":message});
                            if let Some(data) = unknown {
                                error["data"] = data;
                            }
                            json!({"jsonrpc":"2.0","id":id,"error":error})
                        }
                    }
                }
            }
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    drop(jobs);
    Ok(())
}
