//! Validated partition documents. Mutations are transactional and preserve lineage.
use std::collections::BTreeSet;

use basecraft_protocol::delimitation::{
    DelimitationEdit, DelimitationWorkspace, Hypothesis, HypothesisOrigin,
};

use crate::alignment::EditError;

pub fn validate(work: &DelimitationWorkspace) -> Result<(), EditError> {
    if work.version != 1 {
        return fail("unsupported delimitation workspace version");
    }
    let roster: BTreeSet<_> = work.specimens.iter().map(|s| &s.id).collect();
    if roster.len() != work.specimens.len() || roster.iter().any(|id| id.is_empty()) {
        return fail("duplicate or empty specimen IDs");
    }
    let ids: BTreeSet<_> = work.hypotheses.iter().map(|h| &h.id).collect();
    if ids.len() != work.hypotheses.len() || ids.iter().any(|id| id.is_empty()) {
        return fail("duplicate or empty hypothesis IDs");
    }
    for h in &work.hypotheses {
        if let Some(parent) = &h.parent
            && (!ids.contains(parent) || parent == &h.id)
        {
            return fail("unknown or self-referencing parent");
        }
        let mut ancestors = BTreeSet::new();
        let mut current = h;
        while let Some(parent) = &current.parent {
            if !ancestors.insert(parent) {
                return fail("cyclic hypothesis lineage");
            }
            current = work
                .hypotheses
                .iter()
                .find(|x| &x.id == parent)
                .ok_or_else(|| EditError("unknown parent".into()))?;
        }
        let mut members = BTreeSet::new();
        let mut groups = BTreeSet::new();
        for group in &h.groups {
            if group.id.is_empty() || !groups.insert(&group.id) {
                return fail("duplicate or empty group IDs");
            }
            for member in &group.members {
                if !roster.contains(member) || !members.insert(member) {
                    return fail("unknown or overlapping specimen membership");
                }
            }
            for (individual, scores) in &group.individual_scores {
                if !group.members.contains(individual) {
                    return fail("score refers to a specimen outside its group");
                }
                validate_scores(scores)?;
            }
            validate_scores(&group.scores)?;
        }
        for member in &h.unassigned {
            if !roster.contains(member) || !members.insert(member) {
                return fail("unknown or overlapping unassigned specimen");
            }
        }
        if members != roster {
            return fail("every specimen must be assigned or explicitly unassigned");
        }
        validate_scores(&h.scores)?;
        if matches!(
            h.origin,
            HypothesisOrigin::Derived | HypothesisOrigin::Manual | HypothesisOrigin::ManualCut
        ) && (!h.scores.is_empty()
            || h.groups
                .iter()
                .any(|g| !g.scores.is_empty() || !g.individual_scores.is_empty()))
        {
            return fail("manual hypotheses cannot inherit fitted scores");
        }
        let mut mapped = BTreeSet::new();
        for specimens in h.provenance.haplotypes.values() {
            for s in specimens {
                if !roster.contains(s) || !mapped.insert(s) {
                    return fail("unknown or duplicate haplotype mapping");
                }
            }
        }
    }
    if work.view.visible.iter().collect::<BTreeSet<_>>().len() != work.view.visible.len()
        || work.view.visible.iter().any(|id| !ids.contains(id))
        || work.view.selected.iter().any(|id| !roster.contains(id))
    {
        return fail("invalid workspace view references");
    }
    Ok(())
}

fn validate_scores(scores: &[basecraft_protocol::delimitation::Score]) -> Result<(), EditError> {
    if scores
        .iter()
        .any(|s| !s.value.is_finite() || s.scale.is_empty())
    {
        return fail("scores require finite values and a named scale");
    }
    Ok(())
}

fn fail<T>(message: &str) -> Result<T, EditError> {
    Err(EditError(message.into()))
}

/// Apply to a copy, validate, then publish; failure cannot partially mutate state.
pub fn apply(work: &mut DelimitationWorkspace, edit: &DelimitationEdit) -> Result<(), EditError> {
    let mut next = work.clone();
    match edit {
        DelimitationEdit::Combine { hypotheses } => {
            next.hypotheses.extend(hypotheses.clone());
            next.view.visible = next.hypotheses.iter().map(|h| h.id.clone()).collect();
        }
        DelimitationEdit::Add { hypothesis } => next.hypotheses.push(hypothesis.clone()),
        DelimitationEdit::Remove { hypotheses } => {
            if hypotheses
                .iter()
                .any(|id| !next.hypotheses.iter().any(|h| &h.id == id))
            {
                return fail("unknown hypothesis");
            }
            next.hypotheses.retain(|h| !hypotheses.contains(&h.id));
            if next
                .hypotheses
                .iter()
                .any(|h| h.parent.as_ref().is_some_and(|p| hypotheses.contains(p)))
            {
                return fail("keep the hypotheses that edited copies were made from");
            }
            next.view.visible.retain(|id| !hypotheses.contains(id));
            next.view.columns.retain(|id, _| !hypotheses.contains(id));
        }
        DelimitationEdit::SaveCut {
            hypothesis,
            threshold,
            boundary,
        } => {
            if !threshold.is_finite()
                || *threshold < 0.0
                || boundary != "node_age_le_threshold"
                || hypothesis.origin != HypothesisOrigin::ManualCut
            {
                return fail("invalid manual cut convention or threshold");
            }
            let mut h = hypothesis.clone();
            h.provenance
                .parameters
                .insert("threshold".into(), (*threshold).into());
            h.provenance
                .parameters
                .insert("boundary".into(), boundary.clone().into());
            next.hypotheses.push(h);
        }
        DelimitationEdit::Derive { source, id, name } => {
            let mut h = find(&mut next, source)?.clone();
            h.parent = Some(source.clone());
            h.id.clone_from(id);
            h.name.clone_from(name);
            h.origin = HypothesisOrigin::Derived;
            h.scores.clear();
            for g in &mut h.groups {
                g.scores.clear();
                g.individual_scores.clear();
            }
            h.provenance.method = "manual".into();
            h.provenance.status = None;
            next.hypotheses.push(h);
        }
        // A name is a label, not part of an inferred or imported result.
        DelimitationEdit::Rename { hypothesis, name } => {
            find(&mut next, hypothesis)?.name.clone_from(name);
        }
        DelimitationEdit::RenameGroup {
            hypothesis,
            group,
            label,
        } => {
            editable(find(&mut next, hypothesis)?)?
                .groups
                .iter_mut()
                .find(|g| &g.id == group)
                .ok_or_else(|| EditError("unknown group".into()))?
                .label
                .clone_from(label);
        }
        DelimitationEdit::Split { hypothesis, group } => {
            let h = editable(find(&mut next, hypothesis)?)?;
            if h.groups.iter().any(|g| g.id == group.id) || group.members.is_empty() {
                return fail("split requires a new group ID and nonempty selection");
            }
            remove_members(h, &group.members);
            h.groups.retain(|g| !g.members.is_empty());
            h.groups.push(group.clone());
        }
        DelimitationEdit::Merge {
            hypothesis,
            groups,
            id,
            label,
        } => {
            let h = editable(find(&mut next, hypothesis)?)?;
            if groups.len() < 2
                || groups
                    .iter()
                    .any(|id| !h.groups.iter().any(|g| &g.id == id))
            {
                return fail("merge requires at least two known groups");
            }
            let mut merged = h
                .groups
                .iter()
                .find(|g| groups.contains(&g.id))
                .unwrap()
                .clone();
            merged.id.clone_from(id);
            merged.label.clone_from(label);
            merged.members.clear();
            for g in &h.groups {
                if groups.contains(&g.id) {
                    merged.members.extend(g.members.clone());
                }
            }
            h.groups.retain(|g| !groups.contains(&g.id));
            h.groups.push(merged);
        }
        DelimitationEdit::Reassign {
            hypothesis,
            specimens,
            group,
        } => {
            let h = editable(find(&mut next, hypothesis)?)?;
            if group
                .as_ref()
                .is_some_and(|id| !h.groups.iter().any(|g| &g.id == id))
            {
                return fail("unknown destination group");
            }
            remove_members(h, specimens);
            if let Some(id) = group {
                h.groups
                    .iter_mut()
                    .find(|g| &g.id == id)
                    .unwrap()
                    .members
                    .extend(specimens.clone());
            } else {
                h.unassigned.extend(specimens.clone());
            }
            h.groups.retain(|g| !g.members.is_empty());
        }
        DelimitationEdit::SetView { view } => next.view.clone_from(view),
        DelimitationEdit::SetDisplayTree {
            path,
            hash,
            evidence_run,
        } => {
            if next.view.display_tree.as_ref() != Some(path) {
                next.view.tree_edits.clear();
            }
            next.view.display_tree = Some(path.clone());
            next.exchange_fields
                .insert("basecraft_display_hash".into(), hash.clone());
            next.exchange_fields.insert(
                "basecraft_display_evidence_run".into(),
                evidence_run.clone(),
            );
        }
    }
    validate(&next)?;
    *work = next;
    Ok(())
}
fn remove_members(h: &mut Hypothesis, members: &BTreeSet<String>) {
    for g in &mut h.groups {
        g.members.retain(|s| !members.contains(s));
    }
    h.unassigned.retain(|s| !members.contains(s));
}
fn editable(h: &mut Hypothesis) -> Result<&mut Hypothesis, EditError> {
    if matches!(
        h.origin,
        HypothesisOrigin::Imported | HypothesisOrigin::Inferred
    ) {
        return fail("derive the immutable result before editing");
    }
    Ok(h)
}
fn find<'a>(
    work: &'a mut DelimitationWorkspace,
    id: &str,
) -> Result<&'a mut Hypothesis, EditError> {
    work.hypotheses
        .iter_mut()
        .find(|h| h.id == id)
        .ok_or_else(|| EditError("unknown hypothesis".into()))
}
