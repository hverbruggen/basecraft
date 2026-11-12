//! Plugin operation parameters as the window's forms show them.
use serde_json::Value;

/// Whether the parameter form can show `schema` faithfully. Recursion is
/// bounded.
// Keep recursion bounded and reject forms we cannot represent faithfully.
pub fn supported(schema: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    if schema["enum"].is_array() {
        return true;
    }
    match schema["type"].as_str() {
        Some("boolean" | "integer" | "number" | "string") => true,
        Some("array") => supported(&schema["items"], depth + 1),
        Some("object") => schema["properties"].as_object().is_some_and(|properties| {
            properties
                .values()
                .all(|field| supported(field, depth + 1))
                && schema.get("oneOf").is_none_or(|choices| {
                    choices.as_array().is_some_and(|choices| {
                        let mut names = std::collections::BTreeSet::new();
                        choices.iter().all(|choice| {
                            choice.as_object().is_some_and(|fields| {
                                fields
                                    .keys()
                                    .all(|name| matches!(name.as_str(), "required" | "title"))
                                    && choice["required"].as_array().is_some_and(|required| {
                                        !required.is_empty()
                                            && required.iter().all(|name| {
                                                name.as_str().is_some_and(|name| {
                                                    properties.contains_key(name)
                                                        && names.insert(name.to_owned())
                                                })
                                            })
                                    })
                            })
                        })
                    })
                })
        }),
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supports_nested_calibrations_and_exclusive_smoothing() {
        let contract: Value =
            serde_json::from_str(include_str!("../../../plugins/sanderson/operations.json"))
                .unwrap();
        assert!(supported(
            &contract["operations"][0]["parameters"],
            0
        ));
        assert!(!supported(&json!({"type":"null"}), 0));
        assert!(!supported(
            &json!({"type":"object","properties":{"x":{"type":"number"}},"oneOf":[{"required":["x"]},{"required":["x"]}]}),
            0
        ));
    }
}
