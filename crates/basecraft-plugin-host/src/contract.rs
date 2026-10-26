//! Static contract validation and the bounded, offline JSON Schema profile.
//!
//! The validator implements JSON Schema. This module limits the accepted profile,
//! reference expansion and work before handing a schema/instance to it.

use std::collections::BTreeSet;

use basecraft_protocol::operation::{
    CONTRACT_VERSION, ExecutionFamily, InputView, MAX_ARTIFACTS, MAX_BINDINGS,
    MAX_DESCRIPTOR_BYTES, MAX_FRAME_BYTES, MAX_INPUT_FILES, MAX_NESTING, MAX_OPERATIONS,
    MAX_VALIDATION_WORK, OperationContract, OperationDescriptor,
};
use serde_json::{Map, Value, json};

use crate::{Error, Result};

fn invalid(message: impl Into<String>) -> Error {
    Error::Protocol(format!("operation contract: {}", message.into()))
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub fn validate_plugin_id(id: &str) -> Result<()> {
    if id.len() > 80
        || !id.contains('.')
        || !id.split('.').all(|s| {
            !s.is_empty()
                && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(invalid(
            "plugin ID must be lower-case reverse-domain segments (at most 80 bytes)",
        ));
    }
    Ok(())
}

pub fn major(version: &str) -> Result<u16> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|s| {
            s.is_empty()
                || (s.len() > 1 && s.starts_with('0'))
                || !s.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(invalid("operation version must be major.minor.patch"));
    }
    let n: u16 = parts[0]
        .parse()
        .map_err(|_| invalid("invalid operation major"))?;
    if !(1..=9999).contains(&n) {
        return Err(invalid("operation major must be 1..9999"));
    }
    Ok(n)
}

pub fn operation_id(plugin: &str, descriptor: &OperationDescriptor) -> Result<String> {
    validate_plugin_id(plugin)?;
    if !valid_name(&descriptor.name) {
        return Err(invalid("invalid local operation name"));
    }
    Ok(format!(
        "{plugin}/{}@{}",
        descriptor.name,
        major(&descriptor.version)?
    ))
}

pub fn mcp_name(plugin: &str, descriptor: &OperationDescriptor) -> Result<String> {
    operation_id(plugin, descriptor)?;
    Ok(format!(
        "{plugin}.{}.v{}",
        descriptor.name,
        major(&descriptor.version)?
    ))
}

/// Compiled schema plus its conservatively expanded work estimate.
pub struct BoundedSchema {
    validator: jsonschema::Validator,
    work: usize,
    unique: bool,
}

impl BoundedSchema {
    pub fn compile(schema: &Value) -> Result<Self> {
        if serde_json::to_vec(schema)?.len() > MAX_DESCRIPTOR_BYTES {
            return Err(invalid("schema exceeds descriptor byte limit"));
        }
        let mut work = 0;
        let mut unique = false;
        inspect_schema(schema, schema, 0, &mut Vec::new(), &mut work, &mut unique)?;
        let validator = jsonschema::draft202012::options()
            .build(schema)
            .map_err(|e| invalid(format!("invalid JSON Schema: {e}")))?;
        Ok(Self {
            validator,
            work,
            unique,
        })
    }

    pub fn validate(&self, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(value)?.len();
        if bytes > MAX_FRAME_BYTES {
            return Err(invalid("value exceeds frame byte limit"));
        }
        let nodes = value_nodes(value, 0)?.saturating_add(bytes);
        let work = self.work.saturating_mul(nodes);
        let work = if self.unique {
            work.saturating_mul(nodes)
        } else {
            work
        };
        if work > MAX_VALIDATION_WORK {
            return Err(invalid("validation work budget exceeded"));
        }
        self.validator.validate(value).map_err(|e| {
            // Do not copy arbitrary instance values into errors.
            invalid(format!(
                "value does not satisfy schema at {}",
                e.instance_path
            ))
        })
    }
}

fn value_nodes(value: &Value, depth: usize) -> Result<usize> {
    if depth > MAX_NESTING {
        return Err(invalid("value nesting limit exceeded"));
    }
    let children: Box<dyn Iterator<Item = &Value> + '_> = match value {
        Value::Object(m) => Box::new(m.values()),
        Value::Array(a) => Box::new(a.iter()),
        _ => return Ok(1),
    };
    let mut nodes: usize = 1;
    for child in children {
        nodes = nodes.saturating_add(value_nodes(child, depth + 1)?);
        if nodes > MAX_VALIDATION_WORK {
            return Err(invalid("value work budget exceeded"));
        }
    }
    Ok(nodes)
}

fn inspect_schema<'a>(
    root: &'a Value,
    schema: &'a Value,
    depth: usize,
    refs: &mut Vec<&'a str>,
    work: &mut usize,
    unique: &mut bool,
) -> Result<()> {
    *work += 1;
    if depth > MAX_NESTING || *work > 4096 {
        return Err(invalid("schema nesting/reference expansion limit exceeded"));
    }
    if schema.is_boolean() {
        return Ok(());
    }
    let map = schema
        .as_object()
        .ok_or_else(|| invalid("schema must be an object or boolean"))?;
    for (key, value) in map {
        match key.as_str() {
            "$schema" => {
                if value != "https://json-schema.org/draft/2020-12/schema" {
                    return Err(invalid("only JSON Schema draft 2020-12 is supported"));
                }
            }
            "$ref" => {
                let reference = value.as_str().ok_or_else(|| invalid("$ref must be text"))?;
                if !reference.starts_with("#/$defs/") || refs.contains(&reference) {
                    return Err(invalid("only acyclic local $defs references are supported"));
                }
                let target = root
                    .pointer(&reference[1..])
                    .ok_or_else(|| invalid("unresolved local reference"))?;
                refs.push(reference);
                inspect_schema(root, target, depth + 1, refs, work, unique)?;
                refs.pop();
            }
            "$defs" | "properties" => {
                for child in value
                    .as_object()
                    .ok_or_else(|| invalid(format!("{key} must be an object")))?
                    .values()
                {
                    inspect_schema(root, child, depth + 1, refs, work, unique)?;
                }
            }
            "items" | "additionalProperties" | "not" | "if" | "then" | "else" | "contains" => {
                inspect_schema(root, value, depth + 1, refs, work, unique)?;
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                for child in value
                    .as_array()
                    .ok_or_else(|| invalid(format!("{key} must be an array")))?
                {
                    inspect_schema(root, child, depth + 1, refs, work, unique)?;
                }
            }
            "uniqueItems" => {
                *unique |= value == true;
            }
            "type" | "required" | "enum" | "const" | "minimum" | "maximum" | "exclusiveMinimum"
            | "exclusiveMaximum" | "multipleOf" | "minLength" | "maxLength" | "minItems"
            | "maxItems" | "minProperties" | "maxProperties" | "minContains" | "maxContains"
            | "title" | "description" | "default" | "examples" | "$comment" => {
                *work = work.saturating_add(value_nodes(value, depth + 1)?);
            }
            _ => return Err(invalid(format!("unsupported schema keyword {key}"))),
        }
    }
    Ok(())
}

const RESERVED: &[&str] = &[
    "input",
    "output",
    "inputs",
    "outputs",
    "destinations",
    "context",
    "run_id",
    "job_id",
    "cancel_file",
    "progress_file",
    "executable",
    "interpreter",
    "python",
    "environment",
    "env",
    "cwd",
    "cache",
    "cache_dir",
    "output_dir",
    "cpu_threads",
];

pub fn validate_contract(plugin: &str, contract: &OperationContract) -> Result<()> {
    validate_plugin_id(plugin)?;
    if contract.schema != CONTRACT_VERSION
        || contract.operations.is_empty()
        || contract.operations.len() > MAX_OPERATIONS
    {
        return Err(invalid("unsupported contract version or operation count"));
    }
    let mut ids = BTreeSet::new();
    for descriptor in &contract.operations {
        if !ids.insert(operation_id(plugin, descriptor)?) {
            return Err(invalid("duplicate operation identity"));
        }
        if !valid_name(&descriptor.implementation) {
            return Err(invalid("invalid implementation name"));
        }
        if descriptor.required_features.iter().any(|f| {
            f != basecraft_protocol::operation::CONTRACT_CAPABILITY && f != "host-workflow-v1"
        }) {
            return Err(invalid("unsupported required host feature"));
        }
        if descriptor.inputs.len() > MAX_BINDINGS || descriptor.outputs.len() > MAX_BINDINGS {
            return Err(invalid("binding count limit exceeded"));
        }
        for (name, binding) in &descriptor.inputs {
            if !valid_name(name)
                || binding.min > binding.max
                || binding.max == 0
                || binding.max > MAX_INPUT_FILES
            {
                return Err(invalid("invalid input binding/cardinality"));
            }
            if (binding.view == InputView::SequenceSnapshot)
                != (descriptor.family == ExecutionFamily::SequenceProposal)
            {
                return Err(invalid("input view is incompatible with execution family"));
            }
        }
        for (name, binding) in &descriptor.outputs {
            if !valid_name(name)
                || binding.min > binding.max
                || binding.max == 0
                || binding.max > MAX_ARTIFACTS
                || (!binding.collection && binding.max != 1)
            {
                return Err(invalid("invalid output binding/cardinality"));
            }
        }
        if descriptor.family == ExecutionFamily::SequenceProposal
            && (descriptor.inputs.len() != 1
                || descriptor.inputs.values().any(|b| b.min != 1 || b.max != 1))
        {
            return Err(invalid(
                "sequence proposals require exactly one snapshot binding",
            ));
        }
        if (!descriptor.outputs.is_empty() && !descriptor.effects.artifact_write)
            || (descriptor.family == ExecutionFamily::FileAnalysis
                && descriptor.effects.document_write)
        {
            return Err(invalid("effects do not match execution family/bindings"));
        }
        let schema = descriptor
            .parameters
            .as_object()
            .ok_or_else(|| invalid("parameters must be an object schema"))?;
        if schema.get("type") != Some(&json!("object"))
            || schema.get("additionalProperties") != Some(&Value::Bool(false))
        {
            return Err(invalid(
                "parameters require type object and additionalProperties false",
            ));
        }
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("parameters require properties"))?;
        if properties
            .keys()
            .any(|key| RESERVED.contains(&key.as_str()))
        {
            return Err(invalid("reserved control parameter"));
        }
        for cpu in descriptor
            .cpu_parameter
            .iter()
            .chain(&descriptor.cpu_parameters)
        {
            let field = properties
                .get(cpu)
                .ok_or_else(|| invalid("CPU binding is not a parameter"))?;
            if field.get("type") != Some(&json!("integer")) {
                return Err(invalid("CPU parameter must be integer"));
            }
        }
        if let Some(workflow) = &descriptor.host_workflow {
            if plugin != "basecraft.host"
                || ![
                    "gmyc_alignment_single",
                    "gmyc_alignment_multiple",
                    "asap_alignment",
                ]
                .contains(&workflow.as_str())
            {
                return Err(invalid("unsupported or unreserved host workflow"));
            }
        }
        if let Some(adapter) = &descriptor.legacy_adapter {
            if adapter
                .inputs
                .iter()
                .any(|(role, param)| !descriptor.inputs.contains_key(role) || !valid_name(param))
                || adapter
                    .input_arrays
                    .iter()
                    .any(|role| !adapter.inputs.contains_key(role))
            {
                return Err(invalid("invalid legacy input binding"));
            }
            let mut bound = BTreeSet::new();
            for (param, output) in &adapter.outputs {
                let binding = descriptor
                    .outputs
                    .get(&output.role)
                    .ok_or_else(|| invalid("undeclared legacy output role"))?;
                if !valid_name(param)
                    || !bound.insert(param)
                    || (output.path != "."
                        && (!binding.collection
                            || output.path.is_empty()
                            || std::path::Path::new(&output.path)
                                .components()
                                .any(|p| !matches!(p, std::path::Component::Normal(_)))))
                {
                    return Err(invalid("invalid legacy output allocation"));
                }
            }
            if adapter
                .allocation
                .as_ref()
                .is_some_and(|role| !descriptor.outputs.get(role).is_some_and(|b| b.collection))
            {
                return Err(invalid("legacy allocation requires a collection output"));
            }
        }
        BoundedSchema::compile(&descriptor.parameters)?;
        BoundedSchema::compile(&descriptor.result)?;
        for definition in properties.values() {
            if let Some(default) = definition.get("default") {
                // Keep local definitions available while validating each default.
                let mut check = definition.clone();
                if let Some(defs) = schema.get("$defs") {
                    check
                        .as_object_mut()
                        .ok_or_else(|| invalid("default requires object schema"))?
                        .insert("$defs".into(), defs.clone());
                }
                BoundedSchema::compile(&check)?.validate(default)?;
            }
        }
    }
    Ok(())
}

/// Fill only explicitly declared top-level defaults. No coercion or branch guessing.
pub fn normalize_parameters(descriptor: &OperationDescriptor, requested: &Value) -> Result<Value> {
    let mut parameters = requested
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("parameters must be an object"))?;
    let properties = descriptor
        .parameters
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("missing parameter properties"))?;
    if parameters.keys().any(|name| !properties.contains_key(name)) {
        return Err(invalid("unknown parameter"));
    }
    for (name, definition) in properties {
        if !parameters.contains_key(name)
            && let Some(default) = definition.get("default")
        {
            parameters.insert(name.clone(), default.clone());
        }
    }
    let effective = Value::Object(parameters);
    BoundedSchema::compile(&descriptor.parameters)?.validate(&effective)?;
    Ok(effective)
}

/// Preserve local references when placing the parameter schema in an MCP request.
pub fn embedded_parameters(schema: &Value) -> Result<Value> {
    embedded_schema(schema, "#/properties/parameters")
}

/// Embed an already bounded author schema at an absolute local JSON pointer.
pub fn embedded_schema(schema: &Value, location: &str) -> Result<Value> {
    fn rebase(schema: &Value, location: &str) -> Value {
        match schema {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let value = match key.as_str() {
                            "$ref" => value.as_str().map_or_else(
                                || value.clone(),
                                |r| json!(format!("{location}{}", &r[1..])),
                            ),
                            "$defs" | "properties" => value.as_object().map_or_else(
                                || value.clone(),
                                |definitions| {
                                    Value::Object(
                                        definitions
                                            .iter()
                                            .map(|(name, definition)| {
                                                (name.clone(), rebase(definition, location))
                                            })
                                            .collect(),
                                    )
                                },
                            ),
                            "default" | "const" | "enum" | "examples" => value.clone(),
                            _ => rebase(value, location),
                        };
                        (key.clone(), value)
                    })
                    .collect::<Map<_, _>>(),
            ),
            Value::Array(array) => {
                Value::Array(array.iter().map(|value| rebase(value, location)).collect())
            }
            _ => schema.clone(),
        }
    }
    BoundedSchema::compile(schema)?;
    Ok(rebase(schema, location))
}
