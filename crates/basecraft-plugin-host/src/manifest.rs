//! `plugin.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Error, Result};

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kinds: Vec<String>,
    pub entry: Entry,
    #[serde(default)]
    pub operations: Vec<OperationDecl>,
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    /// `python:<script relative to the plugin directory>`.
    pub operation: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OperationDecl {
    pub name: String,
    #[serde(default)]
    pub label: String,
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("plugin.toml");
        let err = |message: String| Error::Manifest {
            path: path.display().to_string(),
            message,
        };
        let text = std::fs::read_to_string(&path).map_err(|e| err(e.to_string()))?;
        let mut m: Manifest = toml::from_str(&text).map_err(|e| err(e.to_string()))?;
        if m.schema != 1 {
            return Err(err(format!("unsupported schema {}", m.schema)));
        }
        m.dir = dir.to_path_buf();
        Ok(m)
    }

    /// The Python script that implements the plugin's operations.
    pub fn python_entry(&self) -> Result<PathBuf> {
        let entry = self.entry.operation.as_deref().unwrap_or("");
        let script = entry
            .strip_prefix("python:")
            .ok_or_else(|| Error::Manifest {
                path: self.dir.join("plugin.toml").display().to_string(),
                message: format!("entry.operation {entry:?} is not python:<script>"),
            })?;
        Ok(self.dir.join(script))
    }
}
