//! Local per-task CPU budget. Jobs freeze the allocation when they are created.
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOCATION: AtomicUsize = AtomicUsize::new(0);
pub fn available() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
pub fn allocation() -> usize {
    match ALLOCATION.load(Ordering::Relaxed) {
        0 => available().min(4),
        n => n.min(available()).max(1),
    }
}
/// Resolve the shared profile for headless consumers, unless the application
/// supplied an explicit session allocation. Loading preferences creates no files.
pub fn effective_allocation() -> crate::Result<usize> {
    let configured = ALLOCATION.load(Ordering::Relaxed);
    if configured != 0 {
        return Ok(configured.clamp(1, available()));
    }
    let paths = basecraft_platform::paths::AppPaths::discover()
        .map_err(|e| crate::Error::Protocol(format!("application paths: {e}")))?;
    let prefs = basecraft_platform::settings::SettingsStore::new(paths.preferences())
        .load()
        .map_err(|e| crate::Error::Protocol(format!("preferences: {e}")))?;
    Ok((prefs.compute.local_cpu_threads as usize).clamp(1, available()))
}
pub fn configure(threads: usize) {
    ALLOCATION.store(threads.clamp(1, available()), Ordering::Relaxed);
}

/// Resolve built-in worker parameters explicitly, so the effective values are
/// both sent through the public API and retained in tool-run provenance.
pub fn parameters(
    plugin: &str,
    mut params: Map<String, Value>,
    budget: usize,
) -> Map<String, Value> {
    let descriptor = crate::registry::RegistryConfig::discover()
        .ok()
        .and_then(|config| crate::registry::Registry::load(&config).ok())
        .and_then(|registry| {
            registry
                .packages
                .get(plugin)
                .and_then(|package| package.descriptors.first())
                .cloned()
        });
    if let Some(descriptor) = descriptor {
        params = parameters_for(&descriptor, params, budget);
    }
    params
}

/// Resolve resource parameters from the public contract, including old facade
/// calls. Zero is automatic only where the declared schema permits it.
pub fn parameters_for(
    descriptor: &basecraft_protocol::operation::OperationDescriptor,
    mut params: Map<String, Value>,
    budget: usize,
) -> Map<String, Value> {
    for key in descriptor
        .cpu_parameter
        .iter()
        .chain(&descriptor.cpu_parameters)
    {
        let field = &descriptor.parameters["properties"][key];
        let cap = field["maximum"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(budget)
            .min(budget)
            .max(1);
        let effective = match params.get(key) {
            None => Some(cap),
            Some(value) => value.as_u64().and_then(|n| {
                if n == 0 && field["minimum"].as_u64().is_some_and(|min| min > 0) {
                    None
                } else {
                    Some(if n == 0 {
                        cap
                    } else {
                        usize::try_from(n).unwrap_or(usize::MAX).min(cap)
                    })
                }
            }),
        };
        if let Some(effective) = effective {
            params.insert(key.clone(), json!(effective));
        }
    }
    params
}

/// One application process shares its configured CPU budget across concurrent
/// file jobs. Independent GUI/CLI processes retain independent budgets.
static JOB_BUDGET: (std::sync::Mutex<usize>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());

pub(crate) struct CpuPermit(usize);
impl CpuPermit {
    pub(crate) fn threads(&self) -> usize {
        self.0
    }
}
impl Drop for CpuPermit {
    fn drop(&mut self) {
        let mut used = JOB_BUDGET
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *used = used.saturating_sub(self.0);
        JOB_BUDGET.1.notify_all();
    }
}

pub(crate) fn reserve_cpu(
    requested: usize,
    cancel: Option<&std::path::Path>,
) -> crate::Result<CpuPermit> {
    reserve_cpu_with_capacity(requested, effective_allocation()?, cancel)
}

fn reserve_cpu_with_capacity(
    requested: usize,
    capacity: usize,
    cancel: Option<&std::path::Path>,
) -> crate::Result<CpuPermit> {
    let requested = requested.clamp(1, capacity.max(1));
    let mut used = JOB_BUDGET
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        if cancel.is_some_and(std::path::Path::exists) {
            return Err(crate::Error::Protocol(
                "job cancelled while waiting for CPU allocation".into(),
            ));
        }
        if used.saturating_add(requested) <= capacity {
            *used += requested;
            return Ok(CpuPermit(requested));
        }
        let (next, _) = JOB_BUDGET
            .1
            .wait_timeout(used, std::time::Duration::from_millis(100))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        used = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budgets_cap_workers_preserve_smaller_choices_and_resolve_auto() {
        let input = json!({"threads":64,"tree_threads":2})
            .as_object()
            .unwrap()
            .clone();
        let p = parameters("basecraft.distance", input, 4);
        assert_eq!(p["threads"], 4);
        assert_eq!(p["tree_threads"], 2);
        let p = parameters(
            "basecraft.distance",
            json!({"threads":0}).as_object().unwrap().clone(),
            3,
        );
        assert_eq!(p["threads"], 3);
        assert_eq!(p["tree_threads"], 3);
        assert_eq!(parameters("basecraft.mafft", Map::new(), 1)["threads"], 1);
        assert_eq!(
            parameters("basecraft.delimitation", Map::new(), 4)["workers"],
            4
        );
        assert!(parameters("other.plugin", Map::new(), 4).is_empty());
        assert_eq!(
            parameters(
                "basecraft.iqtree",
                json!({"threads":-1}).as_object().unwrap().clone(),
                4
            )["threads"],
            -1
        );
    }
}

#[cfg(test)]
mod scheduler_tests {
    use super::*;
    #[test]
    fn simultaneous_jobs_wait_and_cancel_without_consuming_another_budget() {
        let capacity = 2;
        let first = reserve_cpu_with_capacity(capacity, capacity, None).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let cancel = directory.path().join("cancel");
        let other_cancel = cancel.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            reserve_cpu_with_capacity(capacity, capacity, Some(&other_cancel))
                .err()
                .map(|e| e.to_string())
        });
        ready.recv().unwrap();
        std::fs::write(cancel, "cancel").unwrap();
        assert!(worker.join().unwrap().unwrap().contains("cancelled"));
        assert_eq!(*JOB_BUDGET.0.lock().unwrap(), capacity);
        drop(first);
        assert_eq!(*JOB_BUDGET.0.lock().unwrap(), 0);
        drop(reserve_cpu_with_capacity(capacity, capacity, None).unwrap());
    }
}
