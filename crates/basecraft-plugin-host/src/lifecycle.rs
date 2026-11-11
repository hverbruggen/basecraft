//! Execution policy for verified, pinned bundled external side effects.
//! Ordinary analyses retain their existing prompt process cancellation.
use std::{
    fs::OpenOptions,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use basecraft_protocol::plugin::InvokeResult;
use fs4::fs_std::FileExt;
use serde_json::Value;

use crate::{
    Error, Result,
    registry::{Origin, RegisteredOperation},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lifecycle {
    #[default]
    Ordinary,
    ExternalSubmission,
    RetainedReceipt,
    RemoteDraft,
}

impl Lifecycle {
    pub fn for_operation(operation: &RegisteredOperation) -> Result<Self> {
        operation.package.verify()?;
        Ok(if operation.package.origin == Origin::Bundled {
            use basecraft_protocol::operation::LifecyclePolicy;
            match operation
                .descriptor
                .host
                .as_ref()
                .and_then(|host| host.lifecycle)
            {
                Some(LifecyclePolicy::ExternalSubmission) => Self::ExternalSubmission,
                Some(LifecyclePolicy::RetainedReceipt) => Self::RetainedReceipt,
                Some(LifecyclePolicy::RemoteDraft) => Self::RemoteDraft,
                None => Self::Ordinary,
            }
        } else {
            Self::Ordinary
        })
    }

    pub fn external(self) -> bool {
        self == Self::ExternalSubmission
    }
    pub fn remote_draft(self) -> bool {
        self == Self::RemoteDraft
    }

    /// Reuse shared artifact and input-ledger verification for a retained read.
    /// Changed original sources are diagnostics, not a reason to lose a valid RID.
    pub fn verify_retained(
        self,
        inputs: &std::collections::BTreeMap<String, Vec<std::path::PathBuf>>,
        transformation: Option<&str>,
    ) -> Result<Vec<String>> {
        if self != Self::RetainedReceipt {
            return Ok(Vec::new());
        }
        let required = |name| {
            inputs
                .get(name)
                .and_then(|paths| paths.first())
                .ok_or_else(|| Error::Protocol(format!("retained receipt needs {name} input")))
        };
        let receipt = required("receipt")?;
        let query = required("query")?;
        let run = query
            .parent()
            .ok_or_else(|| Error::Protocol("missing query evidence directory".into()))?;
        let input = basecraft_ops::input_evidence::read_input(run)?;
        if input.state != basecraft_protocol::input_records::EvidenceState::Generated
            || Some(input.transformation.as_str()) != transformation
            || input.submitted_hash != basecraft_io::hash_file(query)?
        {
            return Err(Error::Protocol(
                "retained query differs from shared input evidence".into(),
            ));
        }
        let mut diagnostics = basecraft_ops::input_evidence::verify_ancestors(run)?;
        if run.join("manifest.json").is_file() {
            diagnostics.extend(basecraft_ops::provenance::verify(receipt)?);
            let publication = basecraft_ops::provenance::inspect(receipt)?;
            let hash = basecraft_io::hash_file(receipt)?;
            if !publication.outputs.iter().any(|output| output.hash == hash) {
                return Err(Error::Protocol(
                    "retained receipt differs from published artifact identity".into(),
                ));
            }
        } else if receipt != &run.join("pending/receipt") && receipt != &run.join("receipt") {
            return Err(Error::Protocol(
                "unpublished receipt must be in its allocated run".into(),
            ));
        }
        Ok(diagnostics)
    }
    pub fn configure(
        self,
        process: &mut crate::PluginProcess,
        cancelled: std::sync::Arc<AtomicBool>,
    ) {
        if self.external() {
            // Bounds shutdown as well as normal invocations. Cooperative adapter
            // cancellation preserves the cancel/accept race and a returned RID.
            process.runtime_limit(Some(Duration::from_secs(35)));
        } else if self.remote_draft() {
            // Cooperatively cancel between HTTP chunks/requests so a remote
            // draft identity is retained. Socket operations have a 30s timeout.
            process.runtime_limit(Some(Duration::from_secs(3600)));
        } else {
            process.cancellation(cancelled);
        }
    }
    /// At least ten seconds between contacts across GUI/CLI/MCP processes.
    /// Reservation is conservative: a cancelled reservation may send nothing.
    pub fn reserve(self, cancelled: &AtomicBool) -> Result<Option<ContactPermit>> {
        if !self.external() {
            return Ok(None);
        }
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        std::fs::create_dir_all(&paths.state)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(paths.state.join("ncbi-contact.lock"))?;
        let stamp = paths.state.join("ncbi-contact.json");
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(Error::Protocol(
                    "cancelled before upload; nothing sent".into(),
                ));
            }
            if lock.try_lock_exclusive()? {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let previous: f64 = if stamp.exists() {
            serde_json::from_slice(&std::fs::read(&stamp)?)?
        } else {
            0.0
        };
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(Error::Protocol(
                    "cancelled before upload; nothing sent".into(),
                ));
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| Error::Protocol(e.to_string()))?
                .as_secs_f64();
            if !previous.is_finite() || previous > now + 60.0 {
                return Err(Error::Protocol(
                    "NCBI contact clock changed; retry after correcting clock".into(),
                ));
            }
            if now - previous >= 10.0 {
                // If startup/shutdown fails before completion can be recorded,
                // conservatively cover initialization + invocation + teardown.
                basecraft_ops::input_evidence::write_json(&stamp, &(now + 60.0))?;
                return Ok(Some(ContactPermit { _lock: lock, stamp }));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    /// Adapter parsing remains domain-owned. Before preserving acceptance, the
    /// host validates the declared schema, allocated ordinary file and immutable
    /// query identity. A malformed/mismatched receipt never proves acceptance.
    pub fn validate_identity(self, result: &InvokeResult, plan: &Value) -> Result<()> {
        if !self.external() {
            return Ok(());
        }
        let data = result
            .data
            .as_deref()
            .ok_or_else(|| Error::Protocol("external submission missing data".into()))?;
        let mut summary = plan.clone();
        if let Some(object) = summary.as_object_mut() {
            object.remove("fasta");
        }
        if data["plan"] != summary
            || data["query_hash"] != plan["query_hash"]
            || data["approval_hash"] != plan["approval_hash"]
            || data["plan"]["records"] != plan["records"]
            || data["program"] != plan["program"]
            || data["database"] != plan["database"]
            || data["submitted_residues"] != plan["submitted_residues"]
            || !result.ops.is_empty()
            || !result.outputs.is_empty()
            || result.artifacts.len() != 1
            || result.artifacts[0].role != "receipt"
            || result.artifacts[0].path != "."
        {
            return Err(Error::Protocol("external submission identity mismatch; status unknown; do not automatically resubmit".into()));
        }
        Ok(())
    }
    pub fn validate_receipt(
        self,
        result: &InvokeResult,
        receipt: &Path,
        plan: &Value,
    ) -> Result<()> {
        if !self.external() {
            return Ok(());
        }
        let data = result
            .data
            .as_deref()
            .ok_or_else(|| Error::Protocol("external submission missing data".into()))?;
        let stored: Value =
            serde_json::from_slice(&basecraft_ops::provenance::read_record(receipt)?)?;
        if &stored != data
            || data["query_hash"] != plan["query_hash"]
            || data["approval_hash"] != plan["approval_hash"]
            || data["plan"]["records"] != plan["records"]
        {
            return Err(Error::Protocol("external submission receipt/query identity mismatch; submission status unknown; do not resubmit automatically".into()));
        }
        Ok(())
    }
}

/// Keep the interprocess lock through timestamp completion. Spacing is measured
/// from the END of the previous contact, so slow initialization/upload cannot
/// accidentally shorten the actual interval between two POSTs.
pub(crate) struct ContactPermit {
    _lock: std::fs::File,
    stamp: std::path::PathBuf,
}
impl ContactPermit {
    pub fn complete(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Protocol(e.to_string()))?
            .as_secs_f64();
        basecraft_ops::input_evidence::write_json(&self.stamp, &now)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_follows_the_descriptor_and_requires_bundled_origin() {
        use crate::registry::{Registry, RegistryConfig};
        let root = tempfile::tempdir().unwrap();
        let registry = Registry::load(&RegistryConfig {
            bundled: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
            packages: root.path().join("packages"),
            index: root.path().join("plugins.json"),
        })
        .unwrap();
        let mut checked = 0;
        for original in registry.operations.values().filter(|op| {
            op.descriptor
                .host
                .as_ref()
                .is_some_and(|host| host.lifecycle.is_some())
        }) {
            let expected = Lifecycle::for_operation(original).unwrap();
            assert_ne!(expected, Lifecycle::Ordinary);
            let mut renamed = original.clone();
            renamed.id = "example.renamed/operation@1".into();
            assert_eq!(Lifecycle::for_operation(&renamed).unwrap(), expected);
            let mut undeclared = original.clone();
            undeclared.descriptor.host.as_mut().unwrap().lifecycle = None;
            assert_eq!(
                Lifecycle::for_operation(&undeclared).unwrap(),
                Lifecycle::Ordinary
            );
            std::sync::Arc::make_mut(&mut renamed.package).origin = Origin::Editable;
            assert_eq!(
                Lifecycle::for_operation(&renamed).unwrap(),
                Lifecycle::Ordinary
            );
            checked += 1;
        }
        assert_eq!(checked, 3);
    }
}
