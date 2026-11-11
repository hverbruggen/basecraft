//! Import support values from another tree file into an open tree.
//!
//! The values themselves replay from the typed `TreeAddSupport` edits. The
//! source file is registered as a referenced input (never copied) and named by
//! a `ToolRun` activity, so a later save links it as upstream evidence and
//! verification reports a moved or changed source.
use std::path::Path;

use basecraft_protocol::{Op, SupportKind};

use crate::{DocId, Result, Session, error::invalid};

/// Plugin and operation names of the recorded import activity.
pub const ACTIVITY: (&str, &str) = ("basecraft.tree", "import_support");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SupportImport {
    /// Names of the support sets added, in order.
    pub added: Vec<String>,
    /// Most branches matched by any one imported set.
    pub matched: usize,
}

impl Session {
    /// Map every support set of the tree at `source` onto the open tree `id` and
    /// add them as one undo step. `name` renames the sets (suffixing the set
    /// name when there are several); empty keeps the source's set names.
    /// A source without support values changes nothing and records nothing.
    pub fn import_support(
        &mut self,
        id: DocId,
        source: &Path,
        name: &str,
    ) -> Result<SupportImport> {
        self.tree(id)?;
        let source = source.canonicalize()?;
        let hash = basecraft_io::hash_file(&source)?;
        let other = crate::files::load_tree(&source)?;
        if basecraft_io::hash_file(&source)? != hash {
            return Err(invalid("support source changed while it was read"));
        }
        let sets = self
            .tree(id)?
            .support_from(&other.doc)
            .map_err(|e| invalid(e.0))?;
        if sets.is_empty() {
            return Ok(SupportImport::default());
        }
        let label = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let base = name.trim();
        let many = sets.len() > 1;
        let matched = sets
            .iter()
            .map(|(_, values)| values.iter().filter(|v| v.is_some()).count())
            .max()
            .unwrap_or(0);
        let mut ops = Vec::new();
        for (set, values) in sets {
            let original = other.doc.support(&set);
            let kind = original.map_or(SupportKind::Unknown, |s| s.kind);
            let described = original
                .filter(|s| !s.source.is_empty())
                .map_or_else(|| label.clone(), |s| s.source.clone());
            let name = match (base.is_empty(), many) {
                (true, _) => set,
                (false, false) => base.to_owned(),
                (false, true) => format!("{base} {set}"),
            };
            if self.tree(id)?.support(&name).is_some() {
                return Err(invalid(format!(
                    "there's already a support set named {name:?}"
                )));
            }
            ops.push(Op::TreeAddSupport {
                name,
                source: described,
                values,
                kind,
            });
        }
        // Register the reference before editing: a refused registration
        // leaves the tree unchanged.
        let mut params = serde_json::json!({"source": label, "matched": matched});
        if self.tracked
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
        {
            let anchor = self.resolve(self.doc_key(id)?);
            let ledger = crate::records::input_evidence::reference_input_with_dependencies(
                &anchor,
                &source,
                &hash,
                &[],
                &format!("Support values imported into {}", self.doc_key(id)?),
            )?;
            let record = ledger
                .directory()
                .join("records")
                .join(format!("{}.json", ledger.id()));
            params["input_run"] =
                serde_json::to_value(basecraft_protocol::records::ActivityInputRun {
                    schema_version: 1,
                    id: ledger.id().into(),
                    hash: basecraft_io::hash_file(&record)?,
                    extra: std::collections::BTreeMap::new(),
                })?;
        }
        let added: Vec<String> = ops
            .iter()
            .filter_map(|op| match op {
                Op::TreeAddSupport { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        params["sets"] = serde_json::json!(added);
        let key = self.relative(&source);
        self.begin_group();
        let mut edit_ids = Vec::new();
        let applied = ops.into_iter().try_for_each(|op| -> Result<()> {
            edit_ids.push(self.apply(id, op)?.entry_id);
            Ok(())
        });
        self.end_group();
        if let Err(error) = applied {
            // One undo removes the partial group; nothing else is touched.
            if !edit_ids.is_empty() {
                self.undo(id)?;
            }
            return Err(error);
        }
        self.log_contributing_tool_run(
            id,
            self.actor.clone(),
            ACTIVITY,
            params,
            &[(key, hash)],
            &edit_ids,
        )?;
        Ok(SupportImport { added, matched })
    }
}
