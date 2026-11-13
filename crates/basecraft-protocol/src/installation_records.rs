//! Existing plugin installation index and managed-runtime receipt data.
//!
//! These native-path records describe installed state, not portable environments.
//! Filesystem verification, provisioning, selection and size limits belong to the
//! host. Only the current complete record layout is accepted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Retained managed-runtime receipt. The host verifies its paths and digests.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Runtime {
    pub schema: u32,
    pub executables: Vec<String>,
    pub root: PathBuf,
    pub interpreter: PathBuf,
    pub lock_digest: String,
    pub manifest_digest: String,
    pub package_digest: String,
    pub reported_versions: BTreeMap<String, String>,
    pub executable_digests: BTreeMap<String, String>,
}

/// Plugin selections and retained runtimes.
/// The payload parameter preserves the host's behavior-bearing runtime wrapper.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(bound(serialize = "R: Serialize", deserialize = "R: Deserialize<'de>"))]
#[schemars(bound = "R: JsonSchema")]
pub struct InstallationIndex<R = Runtime> {
    pub schema: u32,
    pub active: BTreeMap<String, String>,
    pub runtimes: BTreeMap<String, R>,
    pub disabled: BTreeSet<String>,
    pub editable: BTreeSet<PathBuf>,
}

impl<R> Default for InstallationIndex<R> {
    fn default() -> Self {
        Self {
            schema: 1,
            active: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            disabled: BTreeSet::new(),
            editable: BTreeSet::new(),
        }
    }
}
