//! Existing plugin installation index and managed-runtime receipt data.
//!
//! These native-path records describe installed state, not portable environments.
//! Filesystem verification, provisioning, selection and size limits belong to the
//! host. Runtime unknown fields remain rejected; index extras remain semantic JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Retained managed-runtime receipt. The host verifies its paths and digests.
/// The schema describes objects; the existing serde reader also accepts positional
/// sequences of seven through nine fields, with trailing maps defaulted.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub schema: u32,
    pub executables: Vec<String>,
    pub root: PathBuf,
    pub interpreter: PathBuf,
    pub lock_digest: String,
    pub manifest_digest: String,
    pub package_digest: String,
    #[serde(default)]
    pub reported_versions: BTreeMap<String, String>,
    #[serde(default)]
    pub executable_digests: BTreeMap<String, String>,
}

/// Plugin selections and retained runtimes. Missing schema decodes as one;
/// Rust Default intentionally emits zero, matching the original host declaration.
/// The payload parameter preserves the host's behavior-bearing runtime wrapper.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(bound(serialize = "R: Serialize", deserialize = "R: Deserialize<'de>"))]
#[schemars(bound = "R: JsonSchema")]
pub struct InstallationIndex<R = Runtime> {
    #[serde(default = "index_version")]
    pub schema: u32,
    #[serde(default)]
    pub active: BTreeMap<String, String>,
    #[serde(default)]
    pub runtimes: BTreeMap<String, R>,
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub editable: BTreeSet<PathBuf>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

fn index_version() -> u32 {
    1
}

impl<R> Default for InstallationIndex<R> {
    fn default() -> Self {
        Self {
            schema: 0,
            active: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            disabled: BTreeSet::new(),
            editable: BTreeSet::new(),
            unknown: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_index_and_runtime_bytes_round_trip() {
        for bytes in [
            include_bytes!("../../../testdata/fixtures/records-v1/installation-index-empty.json")
                .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-populated.json"
            )
            .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-defaulted.json"
            )
            .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-mutations-enable.json"
            )
            .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-mutations-remove.json"
            )
            .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-mutations-reselect.json"
            )
            .as_slice(),
            include_bytes!(
                "../../../testdata/fixtures/records-v1/installation-index-rust-default.json"
            )
            .as_slice(),
        ] {
            let record: InstallationIndex = serde_json::from_slice(bytes).unwrap();
            assert_eq!(serde_json::to_vec_pretty(&record).unwrap(), bytes);
        }
        for bytes in [
            include_bytes!("../../../testdata/fixtures/records-v1/managed-plugin-runtime-serializer-empty-maps.json").as_slice(),
            include_bytes!("../../../testdata/fixtures/records-v1/managed-plugin-runtime-serializer-populated.json").as_slice(),
        ] {
            let record: Runtime = serde_json::from_slice(bytes).unwrap();
            assert_eq!(serde_json::to_vec_pretty(&record).unwrap(), bytes);
        }
    }

    // Deliberately neither Default nor JsonSchema. Deriving these on the host
    // runtime must never become necessary merely to decode or default the index.
    #[derive(Deserialize, Serialize)]
    struct Payload(String);

    #[test]
    fn payload_bounds_are_independent_of_default_and_schema() {
        struct NoTraits;
        assert_eq!(InstallationIndex::<NoTraits>::default().schema, 0);
        let index: InstallationIndex<Payload> =
            serde_json::from_str(r#"{"runtimes":{"entry":"receipt"},"extension":[1,true]}"#)
                .unwrap();
        assert_eq!(index.schema, 1);
        assert_eq!(index.runtimes["entry"].0, "receipt");
        assert_eq!(
            serde_json::to_string(&index).unwrap(),
            r#"{"schema":1,"active":{},"runtimes":{"entry":"receipt"},"disabled":[],"editable":[],"extension":[1,true]}"#
        );
        let empty: InstallationIndex<Payload> = serde_json::from_str("{}").unwrap();
        assert!(empty.runtimes.is_empty());
        assert_eq!(empty.schema, 1);
    }

    #[test]
    fn schemas_describe_the_same_nested_runtime() {
        let runtime = serde_json::to_value(schemars::schema_for!(Runtime)).unwrap();
        let index = serde_json::to_value(schemars::schema_for!(InstallationIndex)).unwrap();
        assert_eq!(runtime["title"], "Runtime");
        assert_eq!(runtime["additionalProperties"], false);
        assert_eq!(runtime["properties"]["schema"]["type"], "integer");
        assert_eq!(index["properties"]["schema"]["default"], 1);
        assert_eq!(
            index["properties"]["runtimes"]["additionalProperties"]["$ref"],
            "#/$defs/Runtime"
        );
        let mut nested = index["$defs"]["Runtime"].clone();
        let mut standalone = runtime;
        nested.as_object_mut().unwrap().remove("title");
        standalone.as_object_mut().unwrap().remove("title");
        standalone.as_object_mut().unwrap().remove("$schema");
        assert_eq!(nested, standalone);
    }
}
