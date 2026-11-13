//! Shared input freezing, support semantics, publication and Tasks cancellation.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Clone)]
enum CancellationTarget {
    Registered(crate::jobs::TaskCancellation),
}

static CANCELLATIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, CancellationTarget>>,
> = std::sync::OnceLock::new();

fn cancellations()
-> &'static std::sync::Mutex<std::collections::HashMap<String, CancellationTarget>> {
    CANCELLATIONS.get_or_init(std::sync::Mutex::default)
}

fn cancellation(id: &str) -> Option<CancellationTarget> {
    cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .cloned()
}

pub fn can_cancel_run(id: &str) -> bool {
    match cancellation(id) {
        Some(CancellationTarget::Registered(handle)) => handle.can_cancel(),
        None => false,
    }
}

/// Only an active in-process owner can authorize cancellation. Registered runs
/// use the manager's state lock and cancellation flag, never a retained path.
pub fn cancel_run(id: &str) -> Result<()> {
    match cancellation(id) {
        Some(CancellationTarget::Registered(handle)) => handle.cancel(),
        None => Err(Error::Protocol(
            "Cancellation is unavailable for this run".into(),
        )),
    }
}

pub(crate) fn register_task_cancellation(
    id: String,
    handle: crate::jobs::TaskCancellation,
) -> CancellationRegistration {
    cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.clone(), CancellationTarget::Registered(handle));
    CancellationRegistration(id)
}

/// Producer-declared support semantics win over numeric-range guesses.
pub(crate) fn producer_support<'a>(
    descriptor: &'a basecraft_protocol::operation::OperationDescriptor,
    params: &serde_json::Value,
) -> Option<(&'a str, basecraft_protocol::SupportKind)> {
    descriptor
        .outputs
        .values()
        .flat_map(|output| &output.tree_support)
        .find(|rule| rule.when.as_ref().is_none_or(|when| when.holds(params)))
        .map(|rule| (rule.name.as_str(), rule.kind))
}

/// What an output's trees' branch lengths measure, when the producer states
/// it for runs with these parameters.
pub(crate) fn stated_branch_lengths<'a>(
    output: &'a basecraft_protocol::operation::OutputBinding,
    params: &serde_json::Value,
) -> Option<&'a basecraft_protocol::facts::BranchLengths> {
    let facts = &output.facts;
    facts
        .branch_lengths
        .as_ref()
        .filter(|_| facts.when.as_ref().is_none_or(|when| when.holds(params)))
}

/// Write what the producer states about an output tree into its companion:
/// the support semantics (only when it has exactly one support field;
/// slash-separated values need their own verified order) and the
/// branch-length meaning. None when nothing is stated.
pub(crate) fn write_tree_companion(
    path: &Path,
    support: Option<(&str, basecraft_protocol::SupportKind)>,
    branch_lengths: Option<&basecraft_protocol::facts::BranchLengths>,
) -> Result<Option<PathBuf>> {
    use basecraft_protocol::facts::{Fact, FactSource};
    let mut doc = basecraft_ops::files::load_tree(path)?.doc;
    let support = support.filter(|_| doc.supports().len() == 1);
    if support.is_none() && branch_lengths.is_none() {
        return Ok(None);
    }
    if let Some((name, kind)) = support {
        let old = doc.supports()[0].name.clone();
        doc.rename_support(&old, name);
        doc.set_support_kind(name, kind);
    }
    if let Some(meaning) = branch_lengths {
        doc.restore_branch_lengths(Some(Fact {
            value: meaning.clone(),
            source: FactSource::Stated,
        }));
    }
    Ok(Some(basecraft_ops::files::save_tree_companion(path, &doc)?))
}

pub(crate) fn is_tree(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
}

pub(crate) struct CancellationRegistration(String);
impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        cancellations()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

pub fn output_beside(file: &Path, suffix: &str, ext: &str) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // Drop the extension, and an `.aln` before it.
    let mut stem = name.split('.').next().unwrap_or("alignment").to_string();
    if stem.is_empty() {
        stem = "alignment".into();
    }
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(format!("{stem}.{suffix}.{ext}"))
        } else {
            dir.join(format!("{stem}.{suffix}-{n}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Freeze and verify the accepted bytes for a registered operation.
pub(crate) fn freeze_file(source: &Path, destination: &Path) -> Result<String> {
    let before = basecraft_ops::records::stamp(source)?;
    std::fs::copy(source, destination)?;
    let hash = basecraft_io::hash_file(destination)?;
    if before != basecraft_ops::records::stamp(source)? || basecraft_io::hash_file(source)? != hash
    {
        return Err(Error::Protocol("input changed during submission".into()));
    }
    Ok(hash)
}

/// Both execution families promote only validated staging files through the
/// common store's no-clobber primitive.
pub(crate) fn publish_artifact(
    run: &basecraft_ops::records::ArtifactRun,
    name: &str,
    location: basecraft_ops::records::ArtifactDestination,
) -> Result<PathBuf> {
    Ok(run.publish(name, location)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_number_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rbcL.aln.fasta");
        let first = output_beside(&file, "mafft", "fasta");
        assert_eq!(first, dir.path().join("rbcL.mafft.fasta"));
        std::fs::write(&first, "").unwrap();
        assert_eq!(
            output_beside(&file, "mafft", "fasta"),
            dir.path().join("rbcL.mafft-2.fasta")
        );
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use basecraft_protocol::{SupportKind, operation::OperationContract};
    use serde_json::json;

    #[test]
    fn support_semantics_follow_contract_for_any_package() {
        let contract: OperationContract =
            serde_json::from_str(include_str!("../../../plugins/distance/operations.json"))
                .unwrap();
        crate::contract::validate_contract("example.scientific-tool", &contract).unwrap();
        let descriptor = contract
            .operations
            .iter()
            .find(|op| op.name == "tree")
            .unwrap();
        assert_eq!(
            producer_support(descriptor, &json!({"bootstrap": 10})),
            Some(("Bootstrap", SupportKind::Bootstrap))
        );
        assert_eq!(producer_support(descriptor, &json!({"bootstrap": 0})), None);
        let contract: OperationContract =
            serde_json::from_str(include_str!("../../../plugins/iqtree/operations.json")).unwrap();
        let descriptor = &contract.operations[0];
        assert_eq!(
            producer_support(descriptor, &json!({"support":"ufboot"})),
            Some(("Ultrafast bootstrap (UFBoot)", SupportKind::Bootstrap))
        );
        assert_eq!(
            producer_support(descriptor, &json!({"support":"bootstrap"})),
            Some(("Standard bootstrap", SupportKind::Bootstrap))
        );
        assert_eq!(
            producer_support(descriptor, &json!({"support":"none"})),
            None
        );
    }
}
