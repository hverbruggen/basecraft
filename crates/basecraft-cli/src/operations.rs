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
            let mut session = crate::script_session(project.canonicalize()?)?;
            basecraft_platform::interrupt::install()?;
            let (outcome, target) =
                run_foreground(&registry, &request, policy, "CLI", &mut session)?;
            // A named target, such as a species workspace, keeps what the run
            // added to it; a one-shot command has no later save.
            if request.target.is_some() && outcome["state"] == "completed" {
                if let Some(target) = target {
                    session.save(target, None)?;
                }
            }
            outcome
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
    loop {
        manager.poll_session(session);
        if basecraft_platform::interrupt::interrupted() {
            manager.cancel(&accepted.run_id)?;
        }
        let outcome = manager.get(&accepted.run_id)?;
        if outcome.state.terminal() {
            let target = manager.target_document(&accepted.run_id);
            return Ok((serde_json::to_value(outcome)?, target));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
