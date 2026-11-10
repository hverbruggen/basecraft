//! Submitted-input evidence data. Validation and filesystem effects belong to the host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::InputReference;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SubmittedInput {
    pub schema_version: u32,
    pub source: Option<InputReference>,
    pub submitted_hash: String,
    pub submitted_size: u64,
    /// canonical-fasta-v1 or ungapped-fasta-v1; generated inputs retain their bytes.
    pub transformation: String,
    pub state: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::SubmittedInput;
    use serde_json::{Value, json};

    #[test]
    fn input_records_schema_describes_data_without_host_policy() {
        let schema = serde_json::to_value(schemars::schema_for!(SubmittedInput)).unwrap();
        let properties = &schema["properties"];
        for name in ["state", "transformation"] {
            assert_eq!(properties[name]["type"], "string");
            assert!(properties[name].get("enum").is_none());
        }
        assert_eq!(properties["schema_version"]["type"], "integer");
        assert!(properties["schema_version"].get("const").is_none());
        assert!(properties["schema_version"].get("default").is_none());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("schema_version")));
        assert!(!required.contains(&json!("source")));
        assert!(
            properties["source"]["anyOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["type"] == "null")
        );
        assert_ne!(schema["additionalProperties"], false);
        // Schemars describes an object here. Reused FileStamp serde also accepts
        // positional sequences; schema generation does not change that decoder.
        assert_eq!(schema["$defs"]["FileStamp"]["type"], "object");
        for stamp in [json!([7, 0, 0]), json!([7, 0, 0, null, null])] {
            let input = json!({
                "schema_version": 42, "state": "future", "transformation": "future",
                "submitted_hash": "", "submitted_size": 9,
                "source": {"path": "x", "hash": "h", "stamp": stamp, "future": null},
                "future": {"nested": [null, "雪"]}
            });
            let decoded: SubmittedInput = serde_json::from_value(input).unwrap();
            assert_eq!(decoded.schema_version, 42);
            assert_eq!(decoded.state, "future");
            assert_eq!(decoded.transformation, "future");
            assert_eq!(decoded.source.unwrap().extra["future"], Value::Null);
            assert_eq!(decoded.extra["future"], json!({"nested": [null, "雪"]}));
        }
    }
}
