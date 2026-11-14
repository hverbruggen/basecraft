//! The MCP stdio server, implementing the 2025-11-25 protocol. Its tools are
//! the commands of the shared table (`basecraft_ops::commands`) offered in a
//! live session, plus the installed operations and their jobs. Documents stay
//! open in one headless session; it does not attach to a GUI.
use basecraft_ops::Session;
use basecraft_ops::commands::{self, Call, Command, Kind, Routes};
use basecraft_protocol::operation::OperationErrorCode as Code;
use basecraft_protocol::{Actor, Op};
use serde_json::{Map, Value, json};
use std::{io::Write, path::Path, path::PathBuf};

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
    commands: Vec<&'static Command>,
    writable: bool,
    initialized: bool,
    recent: Vec<PathBuf>,
}

impl Server {
    /// Run the table command `command` with the client's arguments.
    fn call(&mut self, command: &Command, args: Value) -> basecraft_ops::Result<Value> {
        if command.writes && !self.writable {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "This server is read-only; start it with --allow-writes to enable edits",
            )
            .into());
        }
        let Value::Object(args) = args else {
            return Err(basecraft_ops::Error::MissingArgument("arguments"));
        };
        if let Some(unknown) = args
            .keys()
            .find(|key| !command.args.iter().any(|arg| arg.name == key.as_str()))
        {
            return Err(basecraft_ops::Error::WrongArgument {
                name: unknown.clone(),
                expected: format!(
                    "one of the arguments of {}",
                    commands::tool_name(command.name)
                ),
            });
        }
        let mut call = Call::live(
            &mut self.session,
            command,
            args,
            self.writable,
            &self.recent,
        );
        Ok(match (command.run)(&mut call)? {
            Value::String(text) => json!({ "text": text }),
            Value::Null => json!({}),
            value => value,
        })
    }
}

/// Keep save refusals typed while retaining the existing MCP error envelope.
fn call_error(name: &str, error: &basecraft_ops::Error) -> Value {
    if matches!(name, "save" | "save_check") {
        let mut value = crate::save::error_value(error);
        value["schema"] = json!(1);
        value["operation"] = json!(name);
        value["errors"][0]["operation"] = json!(name);
        return value;
    }
    basecraft_plugin_host::jobs::error_value(name, core_error_code(error), error.to_string())
}

/// The MCP tool of a table command, with a JSON Schema of its arguments.
/// `_meta` lists the arguments in the table's order, for positional calls.
fn tool(command: &Command) -> Value {
    let mut properties = Map::new();
    let mut definitions = None;
    for arg in command.args {
        let mut schema = match arg.kind {
            Kind::Document | Kind::Number => json!({"type": "integer", "minimum": 0}),
            Kind::File | Kind::NewFile | Kind::Text => json!({"type": "string"}),
            Kind::Files | Kind::Texts | Kind::List => {
                json!({"type": "array", "items": {"type": "string"}})
            }
            Kind::Flag => json!({"type": "boolean"}),
            Kind::Choice(words) => json!({"type": "string", "enum": words}),
            Kind::Json(schema) => serde_json::from_str(schema).expect("table schemas are JSON"),
            Kind::Operations => {
                let mut op = serde_json::to_value(schemars::schema_for!(Op)).expect("schema");
                let mut defs = op.as_object_mut().and_then(|o| o.remove("$defs"));
                if let Some(defs) = defs.as_mut() {
                    defs["Op"] = op;
                }
                definitions = defs;
                let one = json!({"$ref": "#/$defs/Op"});
                json!({"anyOf": [one, {"type": "array", "items": one}]})
            }
        };
        schema["description"] = json!(arg.help);
        if let Some(default) = arg.default {
            schema["default"] = serde_json::from_str(default).unwrap_or_else(|_| json!(default));
        }
        properties.insert(arg.name.to_owned(), schema);
    }
    let required: Vec<_> = command
        .args
        .iter()
        .filter(|a| a.required)
        .map(|a| a.name)
        .collect();
    let mut input = json!({"type": "object", "properties": properties, "required": required,
        "additionalProperties": false});
    if let Some(definitions) = definitions {
        input["$defs"] = definitions;
    }
    let names: Vec<_> = command.args.iter().map(|a| a.name).collect();
    json!({"name": commands::tool_name(command.name), "description": command.help,
        "inputSchema": input, "annotations": {"readOnlyHint": !command.writes},
        "_meta": {"basecraft/arguments": names}})
}

/// A host tool that takes no table arguments.
fn host_tool(name: &str, description: &str, properties: &Value, required: &[&str]) -> Value {
    json!({"name": name, "description": description, "inputSchema": {"type": "object",
        "properties": properties, "required": required, "additionalProperties": false}})
}

/// Every tool: the table's commands, the host's job and registry tools, and
/// one tool per installed operation this session may run.
fn operation_tools(
    commands: &[&Command],
    registry: &basecraft_plugin_host::registry::Registry,
    policy: &basecraft_plugin_host::policy::SessionPolicy,
) -> Result<Vec<Value>> {
    let mut definitions: Vec<Value> = commands.iter().map(|command| tool(command)).collect();
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
        let mut metadata = host_tool(
            name,
            description,
            &json!({"job":{"type":"string"}}),
            &["job"],
        );
        metadata["outputSchema"] = outcome_schema()?;
        definitions.push(metadata);
    }
    definitions.push(host_tool(
        "basecraft.host.registry_refresh.v1",
        "Refresh installed metadata without running plugins.",
        &json!({}),
        &[],
    ));
    definitions.push(host_tool(
        "basecraft.host.operations_list.v1",
        "List every installed operation with descriptor, pins, enabled state, origin and registry diagnostics, including operations this session cannot run. Same output as `basecraft operations list`.",
        &json!({}),
        &[],
    ));
    definitions.push(host_tool(
        "basecraft.host.operations_describe.v1",
        "Describe one installed operation by ID (plugin/name@major). Same output as `basecraft operations describe`.",
        &json!({"operation":{"type":"string"}}),
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
    match error.downcast_ref::<basecraft_ops::Error>() {
        Some(basecraft_ops::Error::Host(inner)) => core_error_code(inner.as_ref()),
        Some(error) => error.operation_code(),
        None => error.downcast_ref::<basecraft_plugin_host::Error>().map_or(
            Code::InvalidArguments,
            basecraft_plugin_host::jobs::error_code,
        ),
    }
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
        commands: crate::all_commands()
            .into_iter()
            .filter(|command| command.routes != Routes::CommandLine)
            .collect(),
        writable,
        initialized: false,
        recent: basecraft_viewmodel::projects::recent(),
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
                                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"basecraft","version":basecraft_protocol::VERSION},"instructions":"Independent headless project session. Installed operation calls return owned jobs. Use jobs_get/result/cancel; request cancellation does not cancel accepted jobs. Writes require --allow-writes."}),
                            )
                        }
                        "ping" => Ok(json!({})),
                        _ if !server.initialized => Err((-32002, "Initialize first".into())),
                        "tools/list" => {
                            let definitions = operation_tools(&server.commands, &registry, &policy)
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
                                let previous =
                                    operation_tools(&server.commands, &registry, &policy)
                                        .map_err(|e| (-32603, e.to_string()))?;
                                registry = Registry::reload(&config)
                                    .map_err(|e| (-32603, e.to_string()))?;
                                if previous
                                    != operation_tools(&server.commands, &registry, &policy)
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
                                    Some(crate::host::list(&registry))
                                }
                                "basecraft.host.operations_describe.v1" => Some(
                                    args["operation"]
                                        .as_str()
                                        .ok_or_else(|| "Missing string operation".into())
                                        .and_then(|id| crate::host::describe(&registry, id)),
                                ),
                                _ => None,
                            } {
                                return Ok(match metadata {
                                    Ok(value) => structured(value, false),
                                    Err(error) => structured(
                                        basecraft_plugin_host::jobs::error_value(
                                            "basecraft.host",
                                            core_error_code(error.as_ref()),
                                            error.to_string(),
                                        ),
                                        true,
                                    ),
                                });
                            }
                            let job = || {
                                args["job"].as_str().ok_or_else(|| {
                                    basecraft_plugin_host::Error::refused(
                                        Code::InvalidArguments,
                                        "missing string job",
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
                            let Some(command) = commands::find(&server.commands, name) else {
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
                            };
                            Ok(match server.call(command, args) {
                                Ok(value) => structured(value, false),
                                Err(error) => structured(call_error(name, &error), true),
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
