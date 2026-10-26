//! `plugin.toml`.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub contract: Option<String>,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kinds: Vec<String>,
    pub entry: Entry,
    /// Declarative dependencies, provisioned only by explicit installation.
    #[serde(default)]
    pub runtime: Option<crate::runtime::RuntimeSpec>,
    /// Frozen by registry discovery, never supplied by package metadata.
    #[serde(skip)]
    pub managed_runtime: Option<crate::runtime::Runtime>,
    #[serde(default)]
    pub operations: Vec<OperationDecl>,
    /// Settings forms the app shows before running an operation.
    #[serde(default)]
    pub forms: Vec<Form>,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// A settings form: the app draws it from `fields` and sends their values,
/// with `fixed`, as the operation's parameters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Form {
    pub id: String,
    pub title: String,
    pub operation: String,
    /// The run button's text.
    pub button: String,
    /// The new file is `NAME.<suffix>.<extension>`.
    pub output: (String, String),
    #[serde(default)]
    pub min_rows: usize,
    /// The status message while it runs.
    pub working: String,
    /// Send the allocated CPU threads as `threads`.
    #[serde(default)]
    pub threads: bool,
    #[serde(default)]
    pub note: String,
    /// Parameters sent with every run.
    #[serde(default)]
    pub fixed: Map<String, Value>,
    #[serde(default)]
    pub fields: Vec<Field>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Choice,
    Integer,
    Boolean,
}

/// One setting. Conditions read `name == value` or `name != value`, against
/// the other fields' values.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Field {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub default: Value,
    /// `[value, label]`, or `[value, label, condition]` for a choice that
    /// applies only in some contexts, such as `sequence == dna`. An empty
    /// value sends nothing, for the tool's default.
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    /// The field applies only while this holds; otherwise it is greyed and
    /// not sent.
    #[serde(default)]
    pub when: Option<String>,
    /// A higher minimum while a condition holds.
    #[serde(default)]
    pub min_when: Vec<(String, i64)>,
    /// A fixed value while a condition holds.
    #[serde(default)]
    pub value_when: Vec<(String, Value)>,
    /// Shapes the form only; never sent.
    #[serde(default)]
    pub local: bool,
}

/// One choice of a `choice` field.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub when: Option<String>,
}

impl TryFrom<Vec<String>> for Choice {
    type Error = String;
    fn try_from(parts: Vec<String>) -> std::result::Result<Self, String> {
        let mut parts = parts.into_iter();
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(value), Some(label), when, None) => Ok(Self { value, label, when }),
            _ => Err("a choice is [value, label] or [value, label, condition]".into()),
        }
    }
}

impl From<Choice> for Vec<String> {
    fn from(choice: Choice) -> Self {
        [choice.value, choice.label]
            .into_iter()
            .chain(choice.when)
            .collect()
    }
}

/// Whether `condition` holds for `values`. Values compare as text, so
/// `support == true` matches the boolean.
#[must_use]
pub fn holds(condition: &str, values: &Map<String, Value>) -> bool {
    let (name, expected, equal) = if let Some((name, value)) = condition.split_once("!=") {
        (name, value, false)
    } else if let Some((name, value)) = condition.split_once("==") {
        (name, value, true)
    } else {
        return false;
    };
    let actual = match values.get(name.trim()) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    (actual == expected.trim()) == equal
}

impl Field {
    fn applies(&self, values: &Map<String, Value>) -> bool {
        self.when.as_deref().is_none_or(|c| holds(c, values))
    }

    fn minimum(&self, values: &Map<String, Value>) -> Option<i64> {
        self.min_when
            .iter()
            .filter(|(c, _)| holds(c, values))
            .map(|&(_, min)| min)
            .chain(self.min)
            .max()
    }
}

impl Form {
    /// The form as it applies in `context`, such as `{"sequence": "dna"}`:
    /// choices for other contexts are left out, and a default that was left
    /// out becomes the first remaining choice.
    #[must_use]
    pub fn for_context(&self, context: &Map<String, Value>) -> Self {
        let mut form = self.clone();
        for field in &mut form.fields {
            field
                .choices
                .retain(|c| c.when.as_deref().is_none_or(|w| holds(w, context)));
            let kept = field
                .choices
                .iter()
                .any(|c| Some(c.value.as_str()) == field.default.as_str());
            if field.kind == FieldKind::Choice && !kept {
                if let Some(first) = field.choices.first() {
                    field.default = Value::String(first.value.clone());
                }
            }
        }
        form
    }

    /// The parameters for a run: `given` values for the fields that apply,
    /// defaults for those not given, and the fixed ones. Refuses values the
    /// form does not allow.
    pub fn parameters(
        &self,
        given: &Map<String, Value>,
    ) -> std::result::Result<Map<String, Value>, String> {
        let values: Map<String, Value> = self
            .fields
            .iter()
            .map(|f| {
                (
                    f.name.clone(),
                    given
                        .get(&f.name)
                        .cloned()
                        .unwrap_or_else(|| f.default.clone()),
                )
            })
            .collect();
        let mut out = Map::new();
        for field in &self.fields {
            if field.local || !field.applies(&values) {
                continue;
            }
            let forced = field.value_when.iter().find(|(c, _)| holds(c, &values));
            let value = forced.map_or_else(|| values[&field.name].clone(), |(_, v)| v.clone());
            match field.kind {
                FieldKind::Boolean if !value.is_boolean() => {
                    return Err(format!("{} must be on or off", field.label));
                }
                FieldKind::Integer => {
                    let n = value
                        .as_i64()
                        .ok_or_else(|| format!("{} must be a whole number", field.label))?;
                    let min = field.minimum(&values).unwrap_or(i64::MIN);
                    let max = field.max.unwrap_or(i64::MAX);
                    if !(min..=max).contains(&n) {
                        return Err(format!("{} must be from {min} to {max}", field.label));
                    }
                }
                FieldKind::Choice => {
                    let text = value.as_str().unwrap_or_default();
                    if !field.choices.iter().any(|c| c.value == text) {
                        return Err(format!("{text:?} is not a choice for {}", field.label));
                    }
                    if text.is_empty() {
                        continue;
                    }
                }
                FieldKind::Boolean => {}
            }
            out.insert(field.name.clone(), value);
        }
        out.extend(self.fixed.clone());
        Ok(out)
    }
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
        let file = std::fs::File::open(&path).map_err(|e| err(e.to_string()))?;
        let mut text = String::new();
        file.take((basecraft_protocol::operation::MAX_DESCRIPTOR_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|e| err(e.to_string()))?;
        if text.len() > basecraft_protocol::operation::MAX_DESCRIPTOR_BYTES {
            return Err(err("manifest exceeds byte limit".into()));
        }
        let mut m: Manifest = toml::from_str(&text).map_err(|e| err(e.to_string()))?;
        if m.schema != 1 && m.schema != 2 {
            return Err(err(format!("unsupported schema {}", m.schema)));
        }
        m.dir = dir.to_path_buf();
        if let Some(runtime) = &m.runtime {
            runtime.validate()?;
        }
        if m.schema == 2 {
            if !m.operations.is_empty() {
                return Err(err("schema 2 operations belong only in the contract".into()));
            }
            m.operation_contract()?;
            m.python_entry()?;
        } else if m.contract.is_some() {
            return Err(err("schema 1 cannot declare an operation contract".into()));
        }
        Ok(m)
    }

    /// Resolve a regular file contained within this package. Metadata loading never executes it.
    pub fn package_file(&self, relative: &str) -> Result<PathBuf> {
        let path = Path::new(relative);
        if relative.is_empty()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::Protocol(
                "package file must be a relative path without traversal".into(),
            ));
        }
        let root = self.dir.canonicalize()?;
        let mut current = root.clone();
        for part in path.components() {
            current.push(part);
            if std::fs::symlink_metadata(&current)?
                .file_type()
                .is_symlink()
            {
                return Err(Error::Protocol(
                    "package files cannot use symbolic links".into(),
                ));
            }
        }
        if !current.is_file() {
            return Err(Error::Protocol(
                "package entry is not a regular file".into(),
            ));
        }
        Ok(current)
    }

    pub fn operation_contract(
        &self,
    ) -> Result<Option<basecraft_protocol::operation::OperationContract>> {
        if self.schema == 1 {
            return Ok(None);
        }
        let relative = self
            .contract
            .as_deref()
            .ok_or_else(|| Error::Protocol("schema 2 requires contract".into()))?;
        let path = self.package_file(relative)?;
        let file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take((basecraft_protocol::operation::MAX_DESCRIPTOR_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > basecraft_protocol::operation::MAX_DESCRIPTOR_BYTES {
            return Err(Error::Protocol(
                "operation contract exceeds byte limit".into(),
            ));
        }
        let contract = serde_json::from_slice(&bytes)?;
        crate::contract::validate_contract(&self.id, &contract)?;
        Ok(Some(contract))
    }

    pub fn form(&self, id: &str) -> Option<&Form> {
        self.forms.iter().find(|f| f.id == id)
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
        if self.schema == 2 {
            self.package_file(script)
        } else {
            Ok(self.dir.join(script))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shipped(plugin: &str) -> Manifest {
        Manifest::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugins")
                .join(plugin),
        )
        .unwrap()
    }

    fn given(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn forms_send_what_applies_and_refuse_what_they_do_not_allow() {
        let iqtree = shipped("iqtree");
        let form = iqtree.form("tree").unwrap();
        // ModelFinder: rate options do not apply; no support, no replicates.
        assert_eq!(
            form.parameters(&Map::new()).unwrap(),
            given(json!({"model": "MFP", "support": "none", "seed": 1}))
        );
        let chosen = form
            .parameters(&given(
                json!({"model": "GTR", "invariant": true, "support": "ufboot"}),
            ))
            .unwrap();
        assert_eq!(chosen["gamma"], json!(true));
        assert_eq!(chosen["replicates"], json!(1000));
        // UFBoot needs 1000 replicates; a standard bootstrap may use fewer.
        let few = json!({"support": "ufboot", "replicates": 100});
        assert!(form.parameters(&given(few)).unwrap_err().contains("1000"));
        let standard = json!({"support": "bootstrap", "replicates": 100});
        assert_eq!(
            form.parameters(&given(standard)).unwrap()["replicates"],
            json!(100)
        );
        assert!(form.parameters(&given(json!({"model": "XYZ"}))).is_err());

        // Only the models for the alignment's sequence type are offered.
        let fasttree = shipped("fasttree");
        let form = fasttree.form("tree").unwrap();
        let dna = form.for_context(&given(json!({"sequence": "dna"})));
        let p = dna.parameters(&Map::new()).unwrap();
        assert_eq!(
            p,
            given(json!({"model": "GTR", "gamma": false, "support": true, "resamples": 1000}))
        );
        assert!(dna.parameters(&given(json!({"model": "LG"}))).is_err());
        let protein = form.for_context(&given(json!({"sequence": "protein"})));
        assert_eq!(
            protein.parameters(&Map::new()).unwrap()["model"],
            json!("LG")
        );

        // Bootstrap trees use the sorted method; the switch itself is not sent.
        let distance = shipped("distance");
        let nj = distance.form("nj").unwrap();
        let p = nj
            .parameters(&given(json!({"bootstrap_on": true, "method": "njstar"})))
            .unwrap();
        assert_eq!(p["method"], json!("sorted"));
        assert_eq!(p["bootstrap"], json!(1000));
        assert!(!p.contains_key("bootstrap_on"));
        let upgma = distance
            .form("upgma")
            .unwrap()
            .parameters(&Map::new())
            .unwrap();
        assert_eq!(upgma["method"], json!("upgma"));
        assert!(!upgma.contains_key("bootstrap"));
        assert_eq!(
            distance
                .form("matrix")
                .unwrap()
                .parameters(&Map::new())
                .unwrap(),
            given(json!({"model": "p", "min_overlap": 1}))
        );
    }
}
