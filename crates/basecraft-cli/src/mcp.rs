//! Initial local MCP stdio server, implementing the 2025-11-25 protocol.
//! Documents remain in one headless session. This does not attach to a GUI.
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Actor, Op};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::Write,
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
            "description":operation.descriptor.description,"inputSchema":schema,"outputSchema":outcome_schema()?,
            "annotations":{"readOnlyHint":!operation.descriptor.effects.document_write && !operation.descriptor.effects.artifact_write,
                "destructiveHint":operation.descriptor.effects.document_write,"openWorldHint":true}});
        metadata["_meta"] = json!({"basecraft":{"pins":operation.pins(&registry.generation),"runtime":"unverified"}});
        definitions.push(metadata);
    }
    Ok(definitions)
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

fn structured(value: Value, failed: bool) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":failed})
}

pub fn serve(root: &Path, writable: bool, model: &str) -> Result<()> {
    use basecraft_plugin_host::{
        jobs::JobManager,
        policy::SessionPolicy,
        registry::{Registry, RegistryConfig},
    };
    use basecraft_protocol::operation::{OperationErrorCode, OperationRequest, RuntimeState};
    let mut session = Session::recoverable(root.canonicalize()?)?;
    session.set_actor(Actor::Agent {
        model: model.into(),
    });
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
            match crate::operations::read_frame(&mut input) {
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
                            let job_result = match name {
                                "basecraft.host.jobs_get.v1" => {
                                    Some(jobs.get(args["job"].as_str().unwrap_or("")))
                                }
                                "basecraft.host.jobs_result.v1" => {
                                    Some(jobs.result(args["job"].as_str().unwrap_or("")))
                                }
                                "basecraft.host.jobs_cancel.v1" => {
                                    Some(jobs.cancel(args["job"].as_str().unwrap_or("")))
                                }
                                "basecraft.host.jobs_repair.v1" => {
                                    Some(jobs.repair(args["job"].as_str().unwrap_or("")))
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
                                supplied["operation"] = json!(operation.id);
                                if supplied.get("parameters").is_none() {
                                    supplied["parameters"] = json!({});
                                }
                                let result = serde_json::from_value::<OperationRequest>(supplied)
                                    .map_err(basecraft_plugin_host::Error::from)
                                    .and_then(|request| {
                                        jobs.submit(&registry, request, policy.clone(), model)
                                    });
                                return Ok(match result {
                                    Ok(outcome) => structured(json!(outcome), false),
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
                                return Err((-32602, "Unknown tool".into()));
                            }
                            Ok(match server.call(name, &args) {
                                Ok(value) => structured(value, false),
                                Err(error) => structured(
                                    basecraft_plugin_host::jobs::error_value(
                                        name,
                                        OperationErrorCode::InvalidArguments,
                                        error.to_string(),
                                    ),
                                    true,
                                ),
                            })
                        }
                        _ => Err((-32601, "Method not found".into())),
                    })(
                    );
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
    drop(jobs);
    server.session.flush()?;
    Ok(())
}
