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

/// Known producer support semantics, which must win over numeric-range guesses.
/// `package` and `implementation` name the plugin and its operation.
pub(crate) fn producer_support(
    package: &str,
    implementation: &str,
    params: &serde_json::Value,
) -> Option<(&'static str, basecraft_protocol::SupportKind)> {
    if implementation != "tree" {
        return None;
    }
    match package {
        "basecraft.distance"
            if params
                .get("bootstrap")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0 =>
        {
            Some(("Bootstrap", basecraft_protocol::SupportKind::Bootstrap))
        }
        "basecraft.fasttree" => Some((
            "SH-like local support",
            basecraft_protocol::SupportKind::Probability,
        )),
        "basecraft.iqtree" => iqtree_support(params.get("support")),
        _ => None,
    }
}

/// The built-in IQ-TREE form permits one support method per run. Both IQ-TREE
/// bootstrap variants write percentages; their names preserve the distinction.
fn iqtree_support(
    selected: Option<&serde_json::Value>,
) -> Option<(&'static str, basecraft_protocol::SupportKind)> {
    match selected.and_then(serde_json::Value::as_str) {
        Some("ufboot") => Some((
            "Ultrafast bootstrap (UFBoot)",
            basecraft_protocol::SupportKind::Bootstrap,
        )),
        Some("bootstrap") => Some((
            "Standard bootstrap",
            basecraft_protocol::SupportKind::Bootstrap,
        )),
        _ => None,
    }
}

/// Persist producer semantics only when the parsed tree has exactly one support
/// field. Slash-separated external/combined values need their own verified order.
pub(crate) fn write_support_sidecar(
    tree: &Path,
    name: &str,
    kind: basecraft_protocol::SupportKind,
) -> Result<Option<PathBuf>> {
    let mut doc = basecraft_ops::files::load_tree(tree)?.doc;
    if doc.supports().len() != 1 {
        return Ok(None);
    }
    let old = doc.supports()[0].name.clone();
    doc.rename_support(&old, name);
    doc.set_support_kind(name, kind);
    let metadata = basecraft_ops::files::tree_sidecar(tree);
    basecraft_io::write_atomic(&metadata, |w| {
        basecraft_ops::files::write_tree_sidecar_at(
            &doc,
            &basecraft_io::hash_file(tree).map_err(std::io::Error::other)?,
            w,
        )
    })
    .map_err(basecraft_ops::Error::from)?;
    Ok(Some(metadata))
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
    let before = basecraft_ops::input_evidence::stamp(source)?;
    std::fs::copy(source, destination)?;
    let hash = basecraft_io::hash_file(destination).map_err(basecraft_ops::Error::from)?;
    if before != basecraft_ops::input_evidence::stamp(source)?
        || basecraft_io::hash_file(source).map_err(basecraft_ops::Error::from)? != hash
    {
        return Err(Error::Protocol("input changed during submission".into()));
    }
    Ok(hash)
}

/// Both execution families promote only validated staging files through the
/// common store's no-clobber primitive.
pub(crate) fn publish_artifact(
    run: &basecraft_ops::analysis_store::ArtifactRun,
    name: &str,
    location: basecraft_ops::analysis_store::ArtifactDestination,
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
