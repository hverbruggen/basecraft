//! Thin installed-plugin management and static metadata commands.
use std::path::{Path, PathBuf};

use basecraft_plugin_host::{
    Manifest,
    registry::{self, Registry, RegistryConfig},
};
use clap::Subcommand;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum PluginsCommand {
    /// Discover packages without executing their code.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Validate static metadata only; never imports the entry script.
    Validate {
        directory: PathBuf,
    },
    /// Install local trusted code. Successful installation enables it immediately.
    Install {
        directory: PathBuf,
    },
    /// Provision frozen dependencies for the exact installed bundled plugin.
    /// Deployment runs this before launch; discovery never installs anything.
    ProvisionBundled {
        id: String,
    },
    Disable {
        id: String,
    },
    Enable {
        id: String,
    },
    /// Remove active selection; retained package versions and run evidence remain.
    Remove {
        id: String,
    },
    /// Explicitly register a trusted development directory without copying it.
    RegisterEditable {
        directory: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum OperationsCommand {
    /// Execute a shared request in the foreground; outputs one terminal JSON outcome.
    Run {
        operation: String,
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    Describe {
        operation: String,
        #[arg(long)]
        json: bool,
    },
}

pub fn describe(registry: &Registry, operation: &str) -> Result<Value> {
    let op = registry
        .operations
        .get(operation)
        .ok_or("unknown operation")?;
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

fn validate(directory: &Path) -> Result<Value> {
    let manifest = Manifest::load(directory)?;
    let digest = registry::package_digest(directory)?;
    Ok(
        json!({"id":manifest.id,"version":manifest.version,"schema":manifest.schema,
        "contract":manifest.operation_contract()?,"digest":digest,"runtime":"unverified"}),
    )
}

pub fn plugins(command: PluginsCommand) -> Result<()> {
    let config = RegistryConfig::discover()?;
    let value = match command {
        PluginsCommand::List { .. } => {
            let registry = Registry::load(&config)?;
            let packages: Vec<_> = registry.packages.values().map(|p| json!({"id":p.manifest.id,"version":p.manifest.version,
                "schema":p.manifest.schema,"enabled":p.enabled,"origin":p.origin,"digest":p.digest,"runtime":"unverified"})).collect();
            json!({"generation":registry.generation,"packages":packages,"diagnostics":registry.diagnostics})
        }
        PluginsCommand::Validate { directory } => validate(&directory)?,
        PluginsCommand::Install { directory } => {
            eprintln!("{}", registry::INSTALL_WARNING);
            let id = registry::install(&config, &directory)?;
            json!({"installed":id})
        }
        PluginsCommand::ProvisionBundled { id } => {
            registry::provision_bundled(&config, &id)?;
            json!({"provisioned":id})
        }
        PluginsCommand::RegisterEditable { directory } => {
            eprintln!("{}", registry::INSTALL_WARNING);
            registry::register_editable(&config, &directory)?;
            json!({"registered":directory})
        }
        PluginsCommand::Disable { id } => {
            registry::set_enabled(&config, &id, false)?;
            json!({"disabled":id})
        }
        PluginsCommand::Enable { id } => {
            registry::set_enabled(&config, &id, true)?;
            json!({"enabled":id})
        }
        PluginsCommand::Remove { id } => {
            registry::remove(&config, &id)?;
            json!({"removed":id})
        }
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

pub fn operations(command: OperationsCommand) -> Result<()> {
    let registry = Registry::load(&RegistryConfig::discover()?)?;
    let value = match command {
        OperationsCommand::Run {
            operation,
            project,
            request,
            ..
        } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(request)?
                .take((basecraft_protocol::operation::MAX_FRAME_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > basecraft_protocol::operation::MAX_FRAME_BYTES {
                return Err("request exceeds byte limit".into());
            }
            let mut request: basecraft_protocol::operation::OperationRequest =
                serde_json::from_slice(&bytes)?;
            if request.operation != operation {
                return Err("request operation differs from command".into());
            }
            request.operation = operation;
            let policy = basecraft_plugin_host::policy::SessionPolicy::new(&project, true)?;
            let mut session = crate::project_session(&project)?;
            basecraft_platform::interrupt::install()?;
            run_foreground(&registry, &request, policy, "CLI", &mut session)?.0
        }
        OperationsCommand::List { .. } => list(&registry)?,
        OperationsCommand::Describe { operation, .. } => describe(&registry, &operation)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

/// Submit one request in this process and wait for its terminal outcome.
/// A refused submission returns the shared structured error value. Also returns
/// the session document that received sequence proposals, if any.
pub fn run_foreground(
    registry: &Registry,
    request: &basecraft_protocol::operation::OperationRequest,
    policy: basecraft_plugin_host::policy::SessionPolicy,
    initiator: &str,
    session: &mut basecraft_ops::Session,
) -> Result<(Value, Option<basecraft_ops::DocId>)> {
    use basecraft_plugin_host::jobs;
    let mut manager = jobs::JobManager::default();
    let accepted =
        match manager.submit_in_session(registry, request.clone(), policy, initiator, session) {
            Ok(accepted) => accepted,
            Err(error) => {
                return Ok((
                    jobs::error_value(
                        &request.operation,
                        jobs::error_code(&error),
                        error.to_string(),
                    ),
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

/// One tool on files (D1): input files in, new result files out, no open or
/// save step. `basecraft run mafft/align rbcL.fasta -o rbcL.aln.fasta`
#[derive(clap::Args)]
pub struct RunArgs {
    /// The operation: its full ID, or a part that names one, such as `mafft/align`.
    operation: String,
    /// Input files, for an operation that takes one input role.
    files: Vec<PathBuf>,
    /// An input file for a named role: `ROLE=FILE`; repeat for several.
    #[arg(long = "input", value_name = "ROLE=FILE")]
    inputs: Vec<String>,
    /// Where a result goes: `FILE` for the operation's one result, or
    /// `ROLE=FILE` (`ROLE/MEMBER=FILE` inside a collection).
    #[arg(short, long = "output", value_name = "[ROLE=]FILE")]
    outputs: Vec<String>,
    /// A setting: `NAME=VALUE`. VALUE is read as JSON when it parses, else as text.
    #[arg(long = "param", value_name = "NAME=VALUE")]
    params: Vec<String>,
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

/// Run one tool on files. With the global `--project` the run belongs to that
/// project; without it the first input file is a loose file, and the run's
/// records go beside it.
pub fn run_files(args: RunArgs, project: Option<&basecraft_ops::project::Project>) -> Result<()> {
    use std::collections::BTreeMap;
    let registry = Registry::load(&RegistryConfig::discover()?)?;
    let operation = find_operation(&registry, &args.operation)?;
    let descriptor = &registry.operations[&operation].descriptor;
    let mut inputs: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    if !args.files.is_empty() {
        let required = descriptor
            .inputs
            .iter()
            .filter(|(_, binding)| binding.min > 0);
        let role = only(required.map(|(role, _)| role), "input", "--input ROLE=FILE")?;
        inputs.entry(role).or_default().extend(args.files);
    }
    for item in &args.inputs {
        let (role, file) = pair(item, "--input ROLE=FILE")?;
        inputs.entry(role).or_default().push(file.into());
    }
    let mut outputs = BTreeMap::new();
    for item in &args.outputs {
        let (member, file) = if let Some((member, file)) = item.split_once('=') {
            (member.to_owned(), PathBuf::from(file))
        } else {
            let role = only(descriptor.outputs.keys(), "result", "-o ROLE=FILE")?;
            let binding = &descriptor.outputs[&role];
            let member = match (&binding.primary, binding.collection) {
                (Some(primary), true) => format!("{role}/{primary}"),
                _ => role,
            };
            (member, PathBuf::from(item))
        };
        outputs.insert(member, file);
    }
    let mut parameters = serde_json::Map::new();
    for item in &args.params {
        let (name, text) = pair(item, "--param NAME=VALUE")?;
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        parameters.insert(name, value);
    }
    let first = inputs
        .values()
        .flatten()
        .next()
        .ok_or("give at least one input file")?;
    let root = match project {
        Some(project) => project.root().to_path_buf(),
        None => first
            .canonicalize()?
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
    let policy = basecraft_plugin_host::policy::SessionPolicy::new(&root, true)?;
    let mut session = crate::script_session(&root, project)?;
    basecraft_platform::interrupt::install()?;
    let (value, _) = run_foreground(&registry, &request, policy, "CLI", &mut session)?;
    println!("{}", serde_json::to_string(&value)?);
    if value["state"] != "completed" {
        let reason = value["errors"][0]["message"]
            .as_str()
            .unwrap_or("the run did not complete");
        return Err(reason.to_owned().into());
    }
    Ok(())
}
