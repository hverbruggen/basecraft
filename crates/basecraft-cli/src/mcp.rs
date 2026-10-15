//! Initial local MCP stdio server, implementing the 2025-11-25 protocol.
//! Documents remain in one headless session. This does not attach to a GUI.
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Actor, Op};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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
    fn path(&self, path: &str) -> Result<PathBuf> {
        let path = self.session.root().join(path);
        let resolved = if path.exists() {
            path.canonicalize()?
        } else {
            path.parent()
                .ok_or("Missing parent")?
                .canonicalize()?
                .join(path.file_name().ok_or("Missing filename")?)
        };
        if !resolved.starts_with(self.session.root()) {
            return Err("Path lies outside the configured project".into());
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
            .ok_or_else(|| "Unknown document ID".into())
    }

    fn state(&self, doc: DocId) -> Value {
        let (kind, version) = if self.session.is_alignment(doc) {
            ("alignment", self.session.alignment(doc).version())
        } else if self.session.is_tree(doc) {
            ("tree", self.session.tree(doc).version())
        } else if self.session.is_delimitation(doc) {
            ("delimitation", self.session.generation(doc))
        } else {
            ("sequence", self.session.doc(doc).version())
        };
        json!({"document":doc.0,"path":self.session.doc_key(doc),"kind":kind,"version":version,
            "state":self.session.state(doc),"can_undo":self.session.can_undo(doc),"can_redo":self.session.can_redo(doc)})
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
        if matches!(name, "apply" | "save" | "run_tool" | "undo" | "redo") && !self.writable {
            return Err(
                "This server is read-only; start it with --allow-writes to enable edits".into(),
            );
        }
        match name {
            "open" => {
                let path = self.path(text(args, "path")?)?;
                let doc = self.session.open(&path)?;
                self.documents.insert(doc.0, doc);
                Ok(self.state(doc))
            }
            "documents" => {
                let mut docs: Vec<_> = self.documents.values().copied().collect();
                docs.sort_by_key(|d| d.0);
                Ok(json!(
                    docs.into_iter().map(|d| self.state(d)).collect::<Vec<_>>()
                ))
            }
            "state" => Ok(self.state(self.doc(args)?)),
            "document_record" => Ok(serde_json::to_value(
                self.session.document_record(self.doc(args)?)?.report()?,
            )?),
            "read" => {
                let doc = self.doc(args)?;
                if self.session.is_tree(doc) {
                    return Ok(
                        json!({"tips":self.session.tree(doc).nodes().iter().filter(|n|n.is_tip()).map(|n|&n.name).collect::<Vec<_>>()}),
                    );
                }
                if self.session.is_delimitation(doc) {
                    return Ok(serde_json::to_value(self.session.delimitation(doc))?);
                }
                let start = args["start"].as_u64().unwrap_or(0);
                let end = args["end"]
                    .as_u64()
                    .ok_or("Provide an exclusive end coordinate")?;
                if end < start || end - start > 1_000_000 || end > self.session.state(doc).len {
                    return Err("Invalid range or more than one million residues requested".into());
                }
                let residues = if self.session.is_alignment(doc) {
                    let row = args["row"]
                        .as_u64()
                        .and_then(|r| usize::try_from(r).ok())
                        .ok_or("Provide an alignment row")?;
                    if row >= self.session.alignment(doc).row_count() {
                        return Err("Row outside alignment".into());
                    }
                    self.session.alignment(doc).read(row, start, end)
                } else {
                    self.session.doc(doc).read(start, end)
                };
                Ok(json!({"residues":String::from_utf8_lossy(&residues)}))
            }
            "apply" => {
                let doc = self.doc(args)?;
                let version = args["version"]
                    .as_u64()
                    .ok_or("Provide the document version you inspected")?;
                if self.state(doc)["version"] != version {
                    return Err("Document version changed; inspect it again before editing".into());
                }
                let op: Op = serde_json::from_value(args["operation"].clone())?;
                self.session.apply(doc, op)?;
                self.session.flush()?;
                Ok(self.state(doc))
            }
            "undo" | "redo" => {
                let doc = self.doc(args)?;
                if name == "undo" {
                    self.session.undo(doc)?;
                } else {
                    self.session.redo(doc)?;
                }
                Ok(self.state(doc))
            }
            "save" => {
                let doc = self.doc(args)?;
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|p| self.path(p))
                    .transpose()?;
                self.session.save(doc, path.as_deref())?;
                Ok(self.state(doc))
            }
            "run_tool" => {
                let doc = self.doc(args)?;
                let output = self.path(text(args, "output")?)?;
                let mut params = args
                    .get("params")
                    .map(|v| v.as_object().cloned().ok_or("params must be an object"))
                    .transpose()?
                    .unwrap_or_default();
                // Executable/cache overrides are not exposed by this project-scoped tool.
                if params.keys().any(|k| {
                    ![
                        "model",
                        "method",
                        "threads",
                        "tree_threads",
                        "seed",
                        "min_overlap",
                        "rooted",
                        "workers",
                        "tree_index",
                        "candidate_budget",
                        "reference_version",
                        "reference_age_clamp",
                        "unique_haplotypes",
                        "analysis_input",
                        "mapping",
                        "groups_data",
                        "replicates",
                        "progress",
                        "cancel_file",
                        "threshold",
                    ]
                    .contains(&k.as_str())
                }) {
                    return Err("Unsupported tool parameter".into());
                }
                for name in ["analysis_input", "mapping", "progress", "cancel_file"] {
                    if let Some(path) = params.get(name).and_then(Value::as_str) {
                        params.insert(name.into(), json!(self.path(path)?));
                    }
                }
                crate::tools::run(&mut self.session, doc, text(args, "tool")?, output, params)
            }
            _ => Err("Unknown tool".into()),
        }
    }
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
        tool("save","Save a document, optionally to a new project path.",json!({"document":doc,"path":string}), &["document"]),
        tool("run_tool","Run a bundled alignment tool, retaining output provenance.",json!({"document":doc,"tool":{"enum":["mafft","trimal","iqtree","fasttree","distance","delimitation"]},"output":string,"params":{"type":"object"}}), &["document","tool","output"])
    ]})
}

pub fn serve(root: &Path, writable: bool, model: &str) -> Result<()> {
    let mut session = Session::recoverable(root.canonicalize()?)?;
    session.set_actor(Actor::Agent {
        model: model.into(),
    });
    let mut server = Server {
        session,
        documents: HashMap::new(),
        writable,
        initialized: false,
    };
    let input = std::io::stdin();
    let mut output = std::io::stdout().lock();
    for line in input.lock().lines() {
        let line = line?;
        let parsed: std::result::Result<Value, _> = serde_json::from_str(&line);
        let response = match parsed {
            Err(e) => {
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}})
            }
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                if request["jsonrpc"] != "2.0" || !request["method"].is_string() {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid JSON-RPC request"}})
                } else if request.get("id").is_none() {
                    continue;
                } else {
                    let method = request["method"].as_str().unwrap_or("");
                    let result = match method {
                        "initialize" => {
                            server.initialized = true;
                            Ok(
                                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"basecraft","version":env!("CARGO_PKG_VERSION")},"instructions":"Headless project session; does not attach to the running GUI. Edits require --allow-writes."}),
                            )
                        }
                        "ping" => Ok(json!({})),
                        _ if !server.initialized => Err((-32002, "Initialize first")),
                        "tools/list" => Ok(tools()),
                        "tools/call" => {
                            let params = &request["params"];
                            let call = server
                                .call(params["name"].as_str().unwrap_or(""), &params["arguments"]);
                            let (value, error) = match call {
                                Ok(v) => (v, false),
                                Err(e) => (json!({"error":e.to_string()}), true),
                            };
                            Ok(
                                json!({"content":[{"type":"text","text":value.to_string()}],"isError":error}),
                            )
                        }
                        _ => Err((-32601, "Method not found")),
                    };
                    match result {
                        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                        Err((code, message)) => {
                            json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
                        }
                    }
                }
            }
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    server.session.flush()?;
    Ok(())
}
