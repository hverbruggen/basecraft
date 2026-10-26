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
        OperationsCommand::List { .. } => list(&registry)?,
        OperationsCommand::Describe { operation, .. } => describe(&registry, &operation)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
