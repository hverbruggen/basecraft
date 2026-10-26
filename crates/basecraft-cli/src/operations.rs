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
    /// Process-owned JSON job session used by the public Python client.
    Serve {
        #[arg(long)]
        project: PathBuf,
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
    if let OperationsCommand::Serve { project } = &command {
        return serve_jobs(project);
    }
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
            let mut jobs = basecraft_plugin_host::jobs::JobManager::default();
            let mut session = basecraft_ops::Session::recoverable(project.canonicalize()?)?;
            basecraft_platform::interrupt::install()?;
            let accepted = match jobs.submit_in_session(
                &registry,
                request.clone(),
                policy,
                "CLI",
                &mut session,
            ) {
                Ok(accepted) => accepted,
                Err(error) => {
                    println!(
                        "{}",
                        basecraft_plugin_host::jobs::error_value(
                            &request.operation,
                            basecraft_plugin_host::jobs::error_code(&error),
                            error.to_string()
                        )
                    );
                    return Ok(());
                }
            };
            loop {
                jobs.poll_proposals(&mut session);
                if basecraft_platform::interrupt::interrupted() {
                    jobs.cancel(&accepted.run_id)?;
                }
                let outcome = jobs.get(&accepted.run_id)?;
                if outcome.state.terminal() {
                    break serde_json::to_value(outcome)?;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        OperationsCommand::Serve { .. } => return Err("invalid session dispatch".into()),
        OperationsCommand::List { .. } => list(&registry)?,
        OperationsCommand::Describe { operation, .. } => describe(&registry, &operation)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

/// Bounded stdio framing shared by the MCP owner and Python job session.
pub fn read_frame(input: &mut impl std::io::BufRead) -> std::io::Result<Option<Vec<u8>>> {
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

fn serve_jobs(project: &Path) -> Result<()> {
    use std::io::Write;
    let config = RegistryConfig::discover()?;
    let mut registry = Registry::load(&config)?;
    let policy = basecraft_plugin_host::policy::SessionPolicy::new(project, true)?;
    let mut jobs = basecraft_plugin_host::jobs::JobManager::default();
    let mut session = basecraft_ops::Session::recoverable(project.canonicalize()?)?;
    basecraft_platform::interrupt::install()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        while let Ok(Some(line)) = read_frame(&mut input) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut output = std::io::stdout().lock();
    loop {
        jobs.poll_proposals(&mut session);
        if basecraft_platform::interrupt::interrupted() {
            break;
        }
        let line = match receiver.recv_timeout(std::time::Duration::from_millis(25)) {
            Ok(line) => line,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        let request: Value = serde_json::from_slice(&line)?;
        let operation = request["request"]["operation"].as_str().unwrap_or("");
        let response = (|| -> basecraft_plugin_host::Result<Value> {
            match request["method"].as_str().unwrap_or("") {
                "submit" => Ok(serde_json::to_value(jobs.submit_in_session(
                    &registry,
                    serde_json::from_value(request["request"].clone())?,
                    policy.clone(),
                    "Python",
                    &mut session,
                )?)?),
                "status" => Ok(serde_json::to_value(jobs.get(
                    request["job"].as_str().ok_or_else(|| {
                        basecraft_plugin_host::Error::Protocol("missing job".into())
                    })?,
                )?)?),
                "result" => Ok(serde_json::to_value(jobs.result(
                    request["job"].as_str().ok_or_else(|| {
                        basecraft_plugin_host::Error::Protocol("missing job".into())
                    })?,
                )?)?),
                "cancel" => Ok(serde_json::to_value(jobs.cancel(
                    request["job"].as_str().ok_or_else(|| {
                        basecraft_plugin_host::Error::Protocol("missing job".into())
                    })?,
                )?)?),
                "repair" => Ok(serde_json::to_value(jobs.repair(
                    request["job"].as_str().ok_or_else(|| {
                        basecraft_plugin_host::Error::Protocol("missing job".into())
                    })?,
                )?)?),
                "refresh" => {
                    registry = Registry::load(&config)?;
                    list(&registry)
                        .map_err(|error| basecraft_plugin_host::Error::Protocol(error.to_string()))
                }
                _ => Err(basecraft_plugin_host::Error::Protocol(
                    "unknown job-session method".into(),
                )),
            }
        })()
        .unwrap_or_else(|error| {
            basecraft_plugin_host::jobs::error_value(
                operation,
                basecraft_plugin_host::jobs::error_code(&error),
                error.to_string(),
            )
        });
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    drop(jobs);
    Ok(())
}
