//! Combining saved hypotheses preserves identities, lineage and exact source evidence.
use crate::{DocId, Result, Session, error::invalid, files};
use basecraft_protocol::{Op, delimitation::DelimitationEdit};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub fn combine(session: &mut Session, doc: DocId, paths: &[PathBuf]) -> Result<()> {
    if !session.is_delimitation(doc) {
        return Err(invalid("Choose a species workspace"));
    }
    let roster: BTreeSet<_> = session
        .delimitation(doc)
        .specimens
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let mut hypotheses = vec![];
    for path in paths {
        let hash = session.snapshot_file(path)?;
        let sidecar = files::tree_sidecar(path);
        let side_hash = if sidecar.exists() {
            Some(session.snapshot_file(&sidecar)?)
        } else {
            None
        };
        let work = files::load_delimitation(path)?;
        if basecraft_io::hash_file(path)? != hash
            || (if sidecar.exists() {
                Some(basecraft_io::hash_file(&sidecar)?)
            } else {
                None
            }) != side_hash
        {
            return Err(invalid(
                "A source changed while combining; retry with unchanged files",
            ));
        }
        if work
            .specimens
            .iter()
            .map(|s| s.id.clone())
            .collect::<BTreeSet<_>>()
            != roster
        {
            return Err(invalid(format!(
                "{} has a different specimen roster; combine requires matching sequence IDs",
                path.display()
            )));
        }
        let ids: BTreeMap<_, _> = work
            .hypotheses
            .iter()
            .map(|h| (h.id.clone(), ulid::Ulid::generate().to_string()))
            .collect();
        for mut hypothesis in work.hypotheses {
            let original = hypothesis.id.clone();
            hypothesis.id.clone_from(
                ids.get(&original)
                    .ok_or_else(|| invalid("Missing hypothesis identity"))?,
            );
            hypothesis.parent = hypothesis
                .parent
                .map(|p| {
                    ids.get(&p)
                        .cloned()
                        .ok_or_else(|| invalid("Missing hypothesis parent"))
                })
                .transpose()?;
            hypothesis.provenance.parameters.insert("combined_source".into(), serde_json::json!({"path":path,"hash":hash,"sidecar_hash":side_hash,"hypothesis":original}));
            hypotheses.push(hypothesis);
        }
    }
    if hypotheses.is_empty() {
        return Err(invalid("The selected files contain no hypotheses"));
    }
    session.apply(
        doc,
        Op::DelimitationEdit {
            edit: Box::new(DelimitationEdit::Combine { hypotheses }),
        },
    )?;
    Ok(())
}
