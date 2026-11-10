//! Provenance export identity mappings. These data models describe the existing
//! artifact writer; they introduce no production reader or version validation.
//! Writers serialize through `serde_json::Value` for recursive key sorting.
use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Ordered relocations of publications whose original bytes are also retained.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceExportMap {
    pub schema_version: u32,
    pub records: Vec<ProvenanceExportMapping>,
    /// Additive data retained by this model, not a package rewrite policy.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One original publication identity and byte hash, with its relocated identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceExportMapping {
    pub original_id: String,
    pub original_hash: String,
    pub export_id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn model_preserves_extensions_and_arbitrary_identity_strings() {
        let value = json!({
            "schema_version": 7,
            "records": [{
                "original_id": "café/\"source\"",
                "original_hash": "opaque hash",
                "export_id": "relocated\nidentity",
                "row_extension": {"nested":[null,true,{"text":"é"}]}
            }],
            "map_extension": {"ordered":[3,1,3],"unknown":null}
        });
        let model: ProvenanceExportMap = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(model.schema_version, 7); // Data integer, not a const-1 validator.
        assert_eq!(model.extra["map_extension"], value["map_extension"]);
        assert_eq!(
            model.records[0].extra["row_extension"],
            value["records"][0]["row_extension"]
        );
        assert_eq!(serde_json::to_value(&model).unwrap(), value);
        let empty: ProvenanceExportMap =
            serde_json::from_value(json!({"schema_version":0,"records":[]})).unwrap();
        assert_eq!(
            serde_json::to_value(&empty).unwrap(),
            json!({"schema_version":0,"records":[]})
        );
        assert!(empty.extra.is_empty());
    }

    #[test]
    fn model_requires_existing_fields_without_defaults() {
        let baseline = json!({"schema_version":1,"records":[{"original_id":"old","original_hash":"hash","export_id":"new"}]});
        for field in ["schema_version", "records"] {
            let mut missing = baseline.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<ProvenanceExportMap>(missing).is_err(),
                "{field}"
            );
            for wrong in [Value::Null, json!("wrong"), json!({}), json!(true)] {
                let mut value = baseline.clone();
                value[field] = wrong;
                assert!(
                    serde_json::from_value::<ProvenanceExportMap>(value).is_err(),
                    "{field}"
                );
            }
        }
        for wrong in [json!(-1), json!(1.5), json!(4_294_967_296_u64), json!([])] {
            let mut value = baseline.clone();
            value["schema_version"] = wrong;
            assert!(serde_json::from_value::<ProvenanceExportMap>(value).is_err());
        }
        for wrong in [json!(1), json!([null]), json!(["wrong"])] {
            let mut value = baseline.clone();
            value["records"] = wrong;
            assert!(serde_json::from_value::<ProvenanceExportMap>(value).is_err());
        }
        for field in ["original_id", "original_hash", "export_id"] {
            let mut missing = baseline.clone();
            missing["records"][0].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<ProvenanceExportMap>(missing).is_err(),
                "{field}"
            );
            for wrong in [Value::Null, json!(1), json!({}), json!([]), json!(true)] {
                let mut value = baseline.clone();
                value["records"][0][field] = wrong;
                assert!(
                    serde_json::from_value::<ProvenanceExportMap>(value).is_err(),
                    "{field}"
                );
            }
        }
    }

    #[test]
    fn schema_describes_required_fields_and_nested_row_once() {
        let schema = serde_json::to_value(schemars::schema_for!(ProvenanceExportMap)).unwrap();
        assert_eq!(schema["required"], json!(["schema_version", "records"]));
        assert_eq!(schema["properties"]["schema_version"]["type"], "integer");
        assert!(
            schema["properties"]["schema_version"]
                .get("const")
                .is_none()
        );
        assert!(
            schema["properties"]["schema_version"]
                .get("default")
                .is_none()
        );
        assert_eq!(
            schema["properties"]["records"]["items"]["$ref"],
            "#/$defs/ProvenanceExportMapping"
        );
        assert_eq!(schema["$defs"].as_object().unwrap().len(), 1);
        let row = &schema["$defs"]["ProvenanceExportMapping"];
        assert_eq!(
            row["required"],
            json!(["original_id", "original_hash", "export_id"])
        );
        for field in ["original_id", "original_hash", "export_id"] {
            assert_eq!(row["properties"][field]["type"], "string");
            assert!(row["properties"][field].get("default").is_none());
        }
        assert_ne!(schema.get("additionalProperties"), Some(&json!(false)));
        assert_ne!(row.get("additionalProperties"), Some(&json!(false)));
    }
}
