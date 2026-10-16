//! Folder-local analysis artifacts. Opening a document never creates this store.
//!
//! An unmarked `.basecraft` may contain unrelated application data. We claim only
//! `analyses-v1`, and refuse an existing namespace without our exact marker.
use std::path::{Component, Path, PathBuf};

use crate::{Result, error::invalid};

const MARKER: &str = "{\"format\":\"basecraft.analysis-store\",\"schema_version\":1}\n";

#[derive(Clone, Debug)]
pub struct ArtifactRun {
    directory: PathBuf,
    working_directory: PathBuf,
    id: String,
}

/// The default is hidden permanent metadata; working-directory output is opt-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArtifactDestination {
    #[default]
    Metadata,
    WorkingDirectory,
}

fn component(name: &str) -> Result<()> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(invalid("artifact name must be one ordinary path component"));
    }
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(invalid(format!(
            "unsafe artifact directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

impl ArtifactRun {
    /// Reserve a run before execution. Existing IDs and foreign namespaces fail.
    /// No project marker is created. Failed/partial runs remain available here.
    pub fn create(output: &Path, id: &str) -> Result<Self> {
        component(id)?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("invalid analysis run identity"));
        }
        let working_directory = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let hidden = working_directory.join(".basecraft");
        directory(&hidden)?;
        let store = hidden.join("analyses-v1");
        match std::fs::create_dir(&store) {
            Ok(()) => {
                basecraft_io::write_atomic(&store.join("store.json"), |w| {
                    w.write_all(MARKER.as_bytes())
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::validate_store(&store)?;
            }
            Err(error) => return Err(error.into()),
        }
        directory(&store.join("runs"))?;
        let path = store.join("runs").join(id);
        std::fs::create_dir(&path)?;
        std::fs::create_dir(path.join("pending"))?;
        Ok(Self {
            directory: path,
            working_directory,
            id: id.into(),
        })
    }

    pub fn validate_store(store: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(store)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("analysis namespace is not an ordinary directory"));
        }
        let marker = store.join("store.json");
        let metadata = std::fs::symlink_metadata(&marker)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
            return Err(invalid("analysis namespace has an unsafe marker"));
        }
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker)?)?;
        if value["format"] != "basecraft.analysis-store" || value["schema_version"] != 1 {
            return Err(invalid(
                "foreign or unsupported analysis namespace; nothing was migrated",
            ));
        }
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn artifact(&self, name: &str) -> Result<PathBuf> {
        self.destination(ArtifactDestination::Metadata, name)
    }

    pub fn destination(&self, location: ArtifactDestination, name: &str) -> Result<PathBuf> {
        component(name)?;
        Ok(match location {
            ArtifactDestination::Metadata => self.directory.join(name),
            ArtifactDestination::WorkingDirectory => self.working_directory.join(name),
        })
    }

    /// Stream here, close/sync successfully, then publish. Pending data is never
    /// advertised as a completed artifact and is retained on failure/cancel.
    pub fn staging(&self, name: &str) -> Result<PathBuf> {
        component(name)?;
        Ok(self.directory.join("pending").join(name))
    }

    /// Same-filesystem no-clobber publication. Unlike rename/write_atomic this
    /// cannot replace another run or user file. Cross-filesystem export fails.
    pub fn publish(&self, name: &str, location: ArtifactDestination) -> Result<PathBuf> {
        let source = self.staging(name)?;
        let metadata = std::fs::symlink_metadata(&source)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("staged artifact must be an ordinary file"));
        }
        std::fs::File::open(&source)?.sync_all()?;
        let destination = self.destination(location, name)?;
        std::fs::hard_link(&source, &destination)?;
        std::fs::File::open(
            destination
                .parent()
                .ok_or_else(|| invalid("artifact has no parent"))?,
        )?
        .sync_all()?;
        // Keep the pending hardlink if cleanup fails; publication already succeeded.
        let _ = std::fs::remove_file(source);
        Ok(destination)
    }
}
