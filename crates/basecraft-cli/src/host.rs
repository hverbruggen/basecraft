//! Commands that need what `basecraft-ops` can't reach: the plugin host,
//! which runs installed operations, and the figure renderer. They are entries
//! of the same table type as the commands in `basecraft_ops::commands`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_ops::commands::Kind::{Document, File, Files, NewFile, Text, Texts};
use basecraft_ops::commands::{Arg, Call, Command, Routes};
use basecraft_ops::{Error, Session};
use basecraft_plugin_host::registry::{self, Registry, RegistryConfig};
use basecraft_plugin_host::{Manifest, policy::SessionPolicy};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const fn arg(name: &'static str, kind: basecraft_ops::commands::Kind, help: &'static str) -> Arg {
    Arg::new(name, kind, help)
}

/// The words that group commands on the command line, with their help.
pub const GROUPS: &[(&str, &str)] = &[
    ("plugins", "Manage trusted local plugin packages."),
    (
        "operations",
        "Inspect and run installed operations without executing plugins to list them.",
    ),
];

const ID: Arg = Arg::new("id", Text, "The package ID.").positional();
/// Scripts pass `--json`; these commands print only JSON anyway.
const JSON: Arg = Arg::new(
    "json",
    basecraft_ops::commands::Kind::Flag,
    "Print JSON (the only output).",
);
const DIRECTORY: Arg = Arg::new("directory", File, "The package folder.").positional();

/// One table entry whose function returns this module's error type.
macro_rules! command {
    ($name:literal, $help:literal, [$($arg:expr),*], $writes:expr, $routes:expr, $run:path) => {
        Command {
            name: $name,
            help: $help,
            args: &[$($arg),*],
            writes: $writes,
            routes: $routes,
            run: |call| $run(call).map_err(boxed),
        }
    };
}

pub const COMMANDS: &[Command] = &[
    command!("delimitation figures", "Write a species workspace's comparison figure on its display tree as SVG files in a folder, in publication style: one with every hypothesis (NAME.svg) and one per hypothesis (NAME.ID.svg). Each is recorded as an export.",
        [Arg::new("document", Document, "The species workspace.").positional(), arg("output", NewFile, "The folder.").short('o')],
        true, Routes::All, figures_command),
    command!("plugins list", "Discover packages without executing their code.", [JSON], false, Routes::CommandLine, plugins_list),
    command!("plugins validate", "Validate a package's static metadata only; never imports the entry script.", [DIRECTORY], false, Routes::CommandLine, plugins_validate),
    command!("plugins install", "Install local trusted code. Successful installation enables it immediately.", [DIRECTORY], true, Routes::CommandLine, plugins_install),
    command!("plugins provision-bundled", "Provision frozen dependencies for the exact installed bundled plugin. Deployment runs this before launch; discovery never installs anything.", [ID], true, Routes::CommandLine, plugins_provision),
    command!("plugins disable", "Disable an installed package.", [ID], true, Routes::CommandLine, plugins_disable),
    command!("plugins enable", "Enable an installed package.", [ID], true, Routes::CommandLine, plugins_enable),
    command!("plugins remove", "Remove a package from the active selection; retained package versions and run evidence remain.", [ID], true, Routes::CommandLine, plugins_remove),
    command!("plugins register-editable", "Register a trusted development folder without copying it.", [DIRECTORY], true, Routes::CommandLine, plugins_register),
    command!("operations list", "Every installed operation with its descriptor, pins, enabled state and origin, and registry diagnostics.", [JSON], false, Routes::CommandLine, operations_list),
    command!("operations describe", "Describe one installed operation by ID (plugin/name@major).", [Arg::new("operation", Text, "The operation ID.").positional(), JSON], false, Routes::CommandLine, operations_describe),
    command!("operations run", "Run an OperationRequest in the foreground in the project; prints one terminal JSON outcome.",
        [Arg::new("operation", Text, "The operation ID.").positional(), arg("request", File, "The request file."), JSON],
        true, Routes::CommandLine, operations_run),
    command!("run", "Run one tool on files: input files in, new result files out, no open or save step. `basecraft run mafft/align rbcL.fasta -o rbcL.aln.fasta`. With the project the run belongs to it; without it the first input is a loose file and the run's records go beside it.",
        [Arg::new("operation", Text, "The operation: its full ID, or a part that names one, such as `mafft/align`.").positional(),
         Arg::new("files", Files, "Input files, for an operation that takes one input role.").positional().optional(),
         arg("input", Texts, "An input file for a named role: ROLE=FILE; repeat for several.").optional(),
         arg("output", Texts, "Where a result goes: FILE for the operation's one result, or ROLE=FILE (ROLE/MEMBER=FILE inside a collection).").short('o').optional(),
         arg("param", Texts, "A setting: NAME=VALUE. VALUE is read as JSON when it parses, else as text.").optional()],
        true, Routes::CommandLine, run_files),
    command!("plugin", "Run an installed or registered plugin's sequence operation on a file, apply what it proposes and save the file.",
        [Arg::new("plugin", File, "The plugin folder, with a plugin.toml.").positional(),
         Arg::new("file", File, "The sequence file.").positional(),
         Arg::new("operation", Text, "The operation's name.").positional(),
         Arg::new("params", Texts, "Parameters as key=value; values parse as JSON if they can.").positional().optional()],
        true, Routes::CommandLine, run_plugin),
    command!("submission validate", "Validate an exported package with table2asn (the registered operation basecraft.table2asn/validate@1) and record its reports.",
        [Arg::new("package", File, "The package folder.").positional()],
        true, Routes::CommandLine, validate_submission),
];

/// An ops error stays itself; any other passes through as a host error.
fn boxed(error: Box<dyn std::error::Error + Send + Sync>) -> Error {
    match error.downcast::<Error>() {
        Ok(error) => *error,
        Err(error) => Error::Host(error),
    }
}

fn figures_command(call: &mut Call) -> Result<Value> {
    let doc = call.doc("document")?;
    let folder = call.path("output")?;
    Ok(json!({"figures": figures(call.session, doc, &folder)?}))
}

/// The batch figures of species workspace `doc` in `folder`
/// (`basecraft_viewmodel::delimitation::batch_figures`), each recorded as an
/// export.
fn figures(
    session: &mut Session,
    doc: basecraft_ops::DocId,
    folder: &Path,
) -> Result<Vec<PathBuf>> {
    let mut tree = basecraft_ops::delimitation::attached_display_tree(session, doc)?.tree;
    let work = session.delimitation(doc)?.clone();
    basecraft_ops::session::apply_drawing_edits(&mut tree, &work.view.tree_edits);
    let stem = Path::new(session.doc_key(doc)?)
        .file_stem()
        .map_or_else(|| "species".into(), |s| s.to_string_lossy().into_owned());
    let theme = basecraft_viewmodel::Theme::publication();
    std::fs::create_dir_all(folder)?;
    let mut written = Vec::new();
    // The figures' font: a generic family every SVG viewer has.
    for (name, svg) in
        basecraft_viewmodel::delimitation::batch_figures(&work, &tree, &theme, "sans-serif", &stem)
    {
        let path = folder.join(name);
        session.export(doc, &path, "svg", |w| w.write_all(svg.as_bytes()))?;
        written.push(path);
    }
    Ok(written)
}

fn config() -> Result<RegistryConfig> {
    Ok(RegistryConfig::discover()?)
}

fn plugins_list(_: &mut Call) -> Result<Value> {
    let registry = Registry::load(&config()?)?;
    let packages: Vec<_> = registry.packages.values().map(|p| json!({"id":p.manifest.id,"version":p.manifest.version,
        "schema":p.manifest.schema,"enabled":p.enabled,"origin":p.origin,"digest":p.digest,"runtime":"unverified"})).collect();
    Ok(
        json!({"generation":registry.generation,"packages":packages,"diagnostics":registry.diagnostics}),
    )
}

fn plugins_validate(call: &mut Call) -> Result<Value> {
    let directory = call.path("directory")?;
    let manifest = Manifest::load(&directory)?;
    let digest = registry::package_digest(&directory)?;
    Ok(
        json!({"id":manifest.id,"version":manifest.version,"schema":manifest.schema,
        "contract":manifest.operation_contract()?,"digest":digest,"runtime":"unverified"}),
    )
}

fn plugins_install(call: &mut Call) -> Result<Value> {
    eprintln!("{}", registry::INSTALL_WARNING);
    Ok(json!({"installed": registry::install(&config()?, &call.path("directory")?)?}))
}

fn plugins_register(call: &mut Call) -> Result<Value> {
    eprintln!("{}", registry::INSTALL_WARNING);
    let directory = call.path("directory")?;
    registry::register_editable(&config()?, &directory)?;
    Ok(json!({"registered": directory}))
}

fn plugins_provision(call: &mut Call) -> Result<Value> {
    let id = call.text("id")?;
    registry::provision_bundled(&config()?, id)?;
    Ok(json!({"provisioned": id}))
}

fn plugins_disable(call: &mut Call) -> Result<Value> {
    let id = call.text("id")?;
    registry::set_enabled(&config()?, id, false)?;
    Ok(json!({"disabled": id}))
}

fn plugins_enable(call: &mut Call) -> Result<Value> {
    let id = call.text("id")?;
    registry::set_enabled(&config()?, id, true)?;
    Ok(json!({"enabled": id}))
}

fn plugins_remove(call: &mut Call) -> Result<Value> {
    let id = call.text("id")?;
    registry::remove(&config()?, id)?;
    Ok(json!({"removed": id}))
}

pub fn describe(registry: &Registry, operation: &str) -> Result<Value> {
    let op = registry.operations.get(operation).ok_or_else(|| {
        basecraft_plugin_host::Error::refused(
            basecraft_protocol::operation::OperationErrorCode::PluginUnavailable,
            "unknown operation",
        )
    })?;
    Ok(
        json!({"operation":op.id,"descriptor":op.descriptor,"pins":op.pins(&registry.generation),
        "enabled":op.package.enabled,"runtime":"unverified","origin":op.package.origin}),
    )
}

pub fn list(registry: &Registry) -> Result<Value> {
    let operations = registry
        .operations
        .keys()
        .map(|id| describe(registry, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"generation":registry.generation,"operations":operations,"diagnostics":registry.diagnostics}),
    )
}

fn operations_list(_: &mut Call) -> Result<Value> {
    list(&Registry::load(&config()?)?)
}

fn operations_describe(call: &mut Call) -> Result<Value> {
    describe(&Registry::load(&config()?)?, call.text("operation")?)
}

fn operations_run(call: &mut Call) -> Result<Value> {
    use std::io::Read;
    let project = call.project()?;
    let operation = call.text("operation")?.to_owned();
    let mut bytes = Vec::new();
    std::fs::File::open(call.path("request")?)?
        .take((basecraft_protocol::operation::MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > basecraft_protocol::operation::MAX_FRAME_BYTES {
        return Err("request exceeds byte limit".into());
    }
    let request: basecraft_protocol::operation::OperationRequest = serde_json::from_slice(&bytes)?;
    if request.operation != operation {
        return Err("request operation differs from command".into());
    }
    let registry = Registry::load(&config()?)?;
    let policy = SessionPolicy::new(project.root(), true)?;
    Ok(run_foreground(&registry, &request, policy, call.session)?.0)
}

/// Submit one request in this process and wait for its terminal outcome.
/// A refused submission returns the shared structured error value. Also returns
/// the session document that received sequence proposals, if any.
pub fn run_foreground(
    registry: &Registry,
    request: &basecraft_protocol::operation::OperationRequest,
    policy: SessionPolicy,
    session: &mut Session,
) -> Result<(Value, Option<basecraft_ops::DocId>)> {
    use basecraft_plugin_host::jobs;
    basecraft_platform::interrupt::install()?;
    let mut manager = jobs::JobManager::default();
    let accepted =
        match manager.submit_in_session(registry, request.clone(), policy, "CLI", session) {
            Ok(accepted) => accepted,
            Err(error) => {
                let code = jobs::error_code(&error);
                return Ok((
                    jobs::error_value(&request.operation, code, error.to_string()),
                    None,
                ));
            }
        };
    let outcome = manager.wait_in_session(
        &accepted.run_id,
        session,
        &basecraft_platform::interrupt::interrupted,
    )?;
    let target = manager.target_document(&accepted.run_id);
    Ok((serde_json::to_value(outcome)?, target))
}

/// The one ID among the registered operations that `name` names.
fn find_operation(registry: &Registry, name: &str) -> Result<String> {
    if registry.operations.contains_key(name) {
        return Ok(name.into());
    }
    let matches: Vec<&String> = registry
        .operations
        .keys()
        .filter(|id| id.contains(&format!("{name}@")) || id.contains(&format!(".{name}/")))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "no installed operation is called {name}; see `basecraft operations list`"
        )
        .into()),
        several => Err(format!(
            "{name} could be {}; give the full ID",
            several
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into()),
    }
}

/// The only name in `names`, or a refusal that lists them and says how to choose.
fn only<'a>(names: impl Iterator<Item = &'a String>, what: &str, how: &str) -> Result<String> {
    let names: Vec<&String> = names.collect();
    match names.as_slice() {
        [one] => Ok((*one).clone()),
        _ => Err(format!(
            "this operation has {} {what} roles ({}); name one with {how}",
            names.len(),
            names
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into()),
    }
}

fn pair(text: &str, how: &str) -> Result<(String, String)> {
    text.split_once('=')
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .ok_or_else(|| format!("{text}: expected {how}").into())
}

/// A `NAME=VALUE` setting; VALUE is JSON when it parses, else text.
fn settings(items: &[String], how: &str) -> Result<serde_json::Map<String, Value>> {
    let mut map = serde_json::Map::new();
    for item in items {
        let (name, text) = pair(item, how)?;
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        map.insert(name, value);
    }
    Ok(map)
}

/// A recoverable session for the CLI at `root`, in the chosen project.
fn script_session(root: &Path, call: &Call) -> Result<Session> {
    let mut session = Session::recoverable(root)?.in_project(call.session.project().cloned());
    session.set_actor(basecraft_protocol::Actor::Script {
        client: "CLI".into(),
    });
    Ok(session)
}

fn run_files(call: &mut Call) -> Result<Value> {
    let registry = Registry::load(&config()?)?;
    let operation = find_operation(&registry, call.text("operation")?)?;
    let descriptor = &registry.operations[&operation].descriptor;
    let mut inputs: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let files = call.paths("files")?;
    if !files.is_empty() {
        let required = descriptor
            .inputs
            .iter()
            .filter(|(_, binding)| binding.min > 0);
        let role = only(required.map(|(role, _)| role), "input", "--input ROLE=FILE")?;
        inputs.entry(role).or_default().extend(files);
    }
    for item in call.texts("input")? {
        let (role, file) = pair(&item, "--input ROLE=FILE")?;
        inputs.entry(role).or_default().push(call.resolve(&file)?);
    }
    let mut outputs = BTreeMap::new();
    for item in call.texts("output")? {
        let (member, file) = if let Some((member, file)) = item.split_once('=') {
            (member.to_owned(), file.to_owned())
        } else {
            let role = only(descriptor.outputs.keys(), "result", "-o ROLE=FILE")?;
            let binding = &descriptor.outputs[&role];
            let member = match (&binding.primary, binding.collection) {
                (Some(primary), true) => format!("{role}/{primary}"),
                _ => role,
            };
            (member, item.clone())
        };
        outputs.insert(member, PathBuf::from(file));
    }
    let parameters = settings(&call.texts("param")?, "--param NAME=VALUE")?;
    let first = inputs
        .values()
        .flatten()
        .next()
        .ok_or("give at least one input file")?;
    let root = match call.session.project() {
        Some(project) => project.root().to_path_buf(),
        None => first
            .parent()
            .ok_or("the input file has no folder")?
            .to_path_buf(),
    };
    let request = basecraft_ops::requests::file_request(
        &root,
        &operation,
        &inputs,
        Value::Object(parameters),
        &outputs,
    )?;
    let policy = SessionPolicy::new(&root, true)?;
    let mut session = script_session(&root, call)?;
    let (value, _) = run_foreground(&registry, &request, policy, &mut session)?;
    if value["state"] != "completed" {
        let reason = value["errors"][0]["message"]
            .as_str()
            .unwrap_or("the run did not complete");
        eprintln!("basecraft: {reason}");
        call.fail();
    }
    Ok(value)
}

/// `plugin`: a schema-2 package's registered sequence operation through the
/// same job runner as `operations run`, against the file opened in this run.
fn run_plugin(call: &mut Call) -> Result<Value> {
    use basecraft_protocol::operation::{ExecutionFamily, InputReference, OperationRequest};
    let manifest = Manifest::load(&call.path("plugin")?)?;
    let operation = call.text("operation")?;
    let registry = Registry::load(&config()?)?;
    let directory = manifest.dir.canonicalize()?;
    let registered = registry
        .operations
        .values()
        .find(|op| {
            op.package.manifest.id == manifest.id
                && op.descriptor.name == operation
                && op.package.manifest.dir.canonicalize().ok().as_ref() == Some(&directory)
        })
        .ok_or("This plugin directory is not an installed or registered package. Register it with `basecraft plugins register-editable DIR`, then run it again or use `basecraft operations run`")?;
    if registered.descriptor.family != ExecutionFamily::SequenceProposal {
        return Err("`plugin` applies sequence proposals only; run file analyses with `basecraft operations run`".into());
    }
    let binding = registered
        .descriptor
        .inputs
        .keys()
        .next()
        .ok_or("operation declares no snapshot input")?;
    let file = call.path("file")?;
    let request = OperationRequest {
        operation: registered.id.clone(),
        inputs: [(
            binding.clone(),
            vec![InputReference::File {
                path: file.display().to_string(),
            }],
        )]
        .into(),
        parameters: Value::Object(settings(&call.texts("params")?, "key=value")?),
        destinations: BTreeMap::default(),
        pins: None,
        request_key: None,
        target: None,
    };
    let root = file.parent().ok_or("File has no parent")?;
    let policy = SessionPolicy::new(root, true)?;
    let (outcome, target) = run_foreground(&registry, &request, policy, call.session)?;
    if outcome["state"] != "completed" {
        // Nothing is saved after a failed run.
        println!("{outcome}");
        return Err(format!("{} did not complete", registered.id).into());
    }
    call.edits(target.ok_or("operation returned no target document")?);
    Ok(outcome)
}

fn validate_submission(call: &mut Call) -> Result<Value> {
    let project = call.project()?;
    let package = call.path("package")?;
    let request = basecraft_ops::submission::validation_request(project.root(), &package)?;
    let request: basecraft_protocol::operation::OperationRequest = serde_json::from_value(request)?;
    let registry = Registry::load(&config()?)?;
    let policy = SessionPolicy::new(project.root(), true)?;
    let (outcome, _) = run_foreground(&registry, &request, policy, call.session)?;
    if outcome.get("state").is_none() {
        // A refused submission: the shared structured error.
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Err("table2asn validation was refused".into());
    }
    let summary = basecraft_ops::submission::record_validation(project.root(), &package, &outcome)?;
    Ok(serde_json::to_value(summary)?)
}
