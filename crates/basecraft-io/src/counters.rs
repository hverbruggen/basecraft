//! Opt-in test instrumentation for synchronous I/O on the current thread.
//!
//! Reads count successful complete reads, not failed/partial reads or physical
//! disk accesses. Sync counts are invocations (including failed attempts).
//! Spawned threads have independent counters; reset and inspect on the thread
//! doing the I/O. No filesystem writes or process-global environment changes.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub full_reads: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub reads: BTreeMap<PathBuf, Reads>,
    pub sync_data: u64,
    pub sync_all: u64,
    /// A read whose path disappeared before canonicalization is not silently
    /// attributed to a different path. Tests should assert this remains zero.
    pub unresolved_reads: u64,
}

thread_local! {
    static COUNTERS: RefCell<Counters> = RefCell::new(Counters::default());
}

pub fn reset() {
    COUNTERS.with(|c| *c.borrow_mut() = Counters::default());
}

pub fn snapshot() -> Counters {
    COUNTERS.with(|c| c.borrow().clone())
}

/// Record only at the completed read, never again in a delegating hash helper.
pub fn record_read(path: &Path, bytes: u64) {
    COUNTERS.with(|c| {
        let mut counters = c.borrow_mut();
        if let Ok(path) = path.canonicalize() {
            let reads = counters.reads.entry(path).or_default();
            reads.full_reads += 1;
            reads.bytes += bytes;
        } else {
            counters.unresolved_reads += 1;
        }
    });
}

pub fn record_sync_data() {
    COUNTERS.with(|c| c.borrow_mut().sync_data += 1);
}

pub fn record_sync_all() {
    COUNTERS.with(|c| c.borrow_mut().sync_all += 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hashes_resets_and_thread_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, b"ACGT").unwrap();
        reset();
        crate::Source::open(&path).unwrap();
        crate::hash_file(&dir.path().join(".").join("source")).unwrap();
        crate::sha256_file(&path).unwrap();
        assert!(crate::Source::open(&dir.path().join("missing")).is_err());
        assert_eq!(
            snapshot().reads[&path.canonicalize().unwrap()],
            Reads {
                full_reads: 3,
                bytes: 12
            }
        );
        std::thread::spawn(move || {
            assert_eq!(snapshot(), Counters::default());
            crate::hash_file(&path).unwrap();
            assert_eq!(snapshot().reads.len(), 1);
            reset();
        })
        .join()
        .unwrap();
        assert_eq!(snapshot().reads.values().next().unwrap().full_reads, 3);
        reset();
        crate::write_atomic(&dir.path().join("output"), |w| w.write_all(b"A")).unwrap();
        assert_eq!(snapshot().sync_all, 1);
        reset();
        assert_eq!(snapshot(), Counters::default());
    }
}
