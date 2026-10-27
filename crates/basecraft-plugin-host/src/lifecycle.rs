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
}

impl Lifecycle {
    pub fn for_operation(operation: &RegisteredOperation) -> Result<Self> {
        operation.package.verify()?;
        Ok(
            if operation.package.origin == Origin::Bundled
                && operation.id == "basecraft.blast/submit@1"
            {
                Self::ExternalSubmission
            } else {
                Self::Ordinary
            },
        )
    }
    pub fn external(self) -> bool {
        self == Self::ExternalSubmission
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
        } else {
            process.cancellation(cancelled);
        }
    }
    /// At least ten seconds between contacts across GUI/CLI/MCP processes.
    /// Reservation is conservative: a cancelled reservation may send nothing.
    pub fn reserve(self, cancelled: &AtomicBool) -> Result<Option<std::fs::File>> {
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
                basecraft_ops::input_evidence::write_json(&stamp, &now)?;
                return Ok(Some(lock));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    /// Adapter parsing remains domain-owned. Before preserving acceptance, the
    /// host validates the declared schema, allocated ordinary file and immutable
    /// query identity. A malformed/mismatched receipt never proves acceptance.
    pub fn validate_identity(self, result: &InvokeResult, plan: &Value) -> Result<()> {
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
