//! Test-only, process-wide requested-byte accounting. Opt in with a binary-local
//! `#[global_allocator] static ALLOCATOR: heap::Allocator = heap::Allocator;`.
//! This is not RSS: mappings, allocator metadata/slack and internal realloc copies
//! are invisible. All threads count; use isolated processes for scoped workloads.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Identity of the accounting semantics, independent of timing instrumentation.
pub const INSTRUMENTATION: &str = "system-requested-live-v1";

#[derive(Clone, Copy, Debug, Default)]
struct Ledger {
    live: usize,
    peak: usize,
    invalid: bool,
    owner: bool,
    installed: bool,
    failed_allocations: usize,
    reallocations: usize,
    failed_reallocations: usize,
}

impl Ledger {
    fn replace(&mut self, old: usize, new: usize) {
        match self.live.checked_sub(old).and_then(|n| n.checked_add(new)) {
            Some(n) => {
                self.live = n;
                self.peak = self.peak.max(n);
            }
            None => self.invalid = true, // Sticky; never report wrapped bytes as valid.
        }
    }
    fn increment(&mut self, realloc: bool, failed: bool) {
        let counter = if realloc {
            if failed {
                &mut self.failed_reallocations
            } else {
                &mut self.reallocations
            }
        } else {
            &mut self.failed_allocations
        };
        if let Some(n) = counter.checked_add(1) {
            *counter = n;
        } else {
            self.invalid = true;
        }
    }
}

struct State(UnsafeCell<Ledger>);
// SAFETY: every access to the cell, including allocator hooks, holds LOCK.
unsafe impl Sync for State {}
static STATE: State = State(UnsafeCell::new(Ledger {
    live: 0,
    peak: 0,
    invalid: false,
    owner: false,
    installed: false,
    failed_allocations: 0,
    reallocations: 0,
    failed_reallocations: 0,
}));
static LOCK: AtomicBool = AtomicBool::new(false);

fn locked<T>(f: impl FnOnce(&mut Ledger) -> T) -> T {
    while LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    struct Unlock;
    impl Drop for Unlock {
        fn drop(&mut self) {
            LOCK.store(false, Ordering::Release);
        }
    }
    let _unlock = Unlock;
    // SAFETY: exclusive lock held, reference cannot escape this private closure.
    f(unsafe { &mut *STATE.0.get() })
}

/// System allocator with serialized, allocation-free hooks. No logging, TLS,
/// formatting or potentially allocating framework code is called under the lock.
/// System calls are inside the lock so concurrent snapshots cannot observe half
/// of a realloc/deallocation. This perturbs timings; never compare to plain runs.
pub struct Allocator;
// SAFETY: all pointer/layout obligations are forwarded unchanged to System.
// Failed realloc leaves the old allocation live. Successful realloc replaces its
// requested size atomically, rather than pretending both blocks remain live.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        locked(|s| {
            s.installed = true;
            let ptr = unsafe { System.alloc(layout) };
            if ptr.is_null() {
                s.increment(false, true);
            } else {
                s.replace(0, layout.size());
            }
            ptr
        })
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        locked(|s| {
            s.installed = true;
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if ptr.is_null() {
                s.increment(false, true);
            } else {
                s.replace(0, layout.size());
            }
            ptr
        })
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        locked(|s| {
            unsafe {
                System.dealloc(ptr, layout);
            }
            s.replace(layout.size(), 0);
        });
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        locked(|s| {
            let next = unsafe { System.realloc(ptr, layout, new_size) };
            s.increment(true, next.is_null());
            if !next.is_null() {
                s.replace(layout.size(), new_size);
            }
            next
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub baseline_bytes: usize,
    pub peak_bytes: usize,
    pub live_bytes: usize,
    pub incremental_peak_bytes: usize,
    pub successful_reallocations: usize,
    pub failed_reallocations: usize,
    pub failed_allocations: usize,
}

/// Single shared measurement owner. Dropping it also releases ownership on error.
pub struct Measurement {
    baseline: Ledger,
}

impl Measurement {
    pub fn start() -> Result<Self, &'static str> {
        locked(|s| {
            if !s.installed {
                return Err("counting allocator is not installed");
            }
            if s.invalid {
                return Err("heap accounting overflow or underflow");
            }
            if s.owner {
                return Err("heap measurement already active");
            }
            s.owner = true;
            s.peak = s.live;
            Ok(Self { baseline: *s })
        })
    }
    pub fn finish(self) -> Result<Stats, &'static str> {
        locked(|s| {
            if s.invalid {
                return Err("heap accounting overflow or underflow");
            }
            Ok(Stats {
                baseline_bytes: self.baseline.live,
                peak_bytes: s.peak,
                live_bytes: s.live,
                incremental_peak_bytes: s.peak - self.baseline.live,
                successful_reallocations: s.reallocations - self.baseline.reallocations,
                failed_reallocations: s.failed_reallocations - self.baseline.failed_reallocations,
                failed_allocations: s.failed_allocations - self.baseline.failed_allocations,
            })
        })
    }
}
impl Drop for Measurement {
    fn drop(&mut self) {
        locked(|s| s.owner = false);
    }
}

pub fn live_bytes() -> Result<usize, &'static str> {
    locked(|s| {
        if s.invalid {
            Err("heap accounting overflow or underflow")
        } else {
            Ok(s.live)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_sizes_growth_shrink_free_and_failed_realloc() {
        let mut s = Ledger::default();
        s.replace(0, 100);
        s.replace(100, 400);
        s.increment(true, false);
        s.increment(true, true); // Failed realloc must leave the 400 bytes live.
        assert_eq!((s.live, s.peak), (400, 400));
        s.replace(400, 40);
        s.increment(true, false);
        s.replace(40, 0);
        s.increment(false, true);
        assert_eq!((s.live, s.peak), (0, 400));
        assert_eq!(
            (
                s.reallocations,
                s.failed_reallocations,
                s.failed_allocations
            ),
            (2, 1, 1)
        );
        assert!(!s.invalid);
    }
    #[test]
    fn real_system_hooks_and_exclusive_owner() {
        // This unit-test binary uses its ordinary allocator; only these explicit
        // calls count, so parallel harness allocations cannot perturb assertions.
        unsafe {
            let a = Allocator;
            let layout = Layout::from_size_align(64, 8).unwrap();
            let p = a.alloc_zeroed(layout);
            assert!(!p.is_null());
            assert!((0..64).all(|i| *p.add(i) == 0));
            let m = Measurement::start().unwrap();
            assert!(Measurement::start().is_err());
            std::thread::spawn(|| assert!(Measurement::start().is_err()))
                .join()
                .unwrap();
            let p = a.realloc(p, layout, 256);
            assert!(!p.is_null());
            let large = Layout::from_size_align(256, 8).unwrap();
            // System cannot fulfill an isize::MAX request on supported hosts.
            assert!(a.realloc(p, large, (usize::MAX / 2) & !7).is_null());
            let p = a.realloc(p, large, 16);
            assert!(!p.is_null());
            a.dealloc(p, Layout::from_size_align(16, 8).unwrap());
            assert!(
                a.alloc(Layout::from_size_align((usize::MAX / 2) & !7, 1).unwrap())
                    .is_null()
            );
            let s = m.finish().unwrap();
            assert_eq!(
                (
                    s.baseline_bytes,
                    s.peak_bytes,
                    s.live_bytes,
                    s.incremental_peak_bytes
                ),
                (64, 256, 0, 192)
            );
            assert_eq!(
                (
                    s.successful_reallocations,
                    s.failed_reallocations,
                    s.failed_allocations
                ),
                (2, 1, 1)
            );
            let m = Measurement::start().unwrap();
            drop(m); // Error/unwind cleanup releases the sole owner.
            assert!(Measurement::start().is_ok());
        }
    }
    #[test]
    fn arithmetic_faults_are_sticky() {
        let mut s = Ledger::default();
        s.replace(1, 0);
        assert!(s.invalid);
        let mut s = Ledger::default();
        s.replace(0, usize::MAX);
        s.replace(0, 1);
        assert!(s.invalid);
        let mut s = Ledger {
            reallocations: usize::MAX,
            ..Ledger::default()
        };
        s.increment(true, false);
        assert!(s.invalid);
    }
}
