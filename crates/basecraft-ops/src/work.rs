//! Cancellation and progress for slow work. The desktop app, the CLI and the
//! MCP server hand the same type to the code that does the work.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// One piece of slow work: a cancel flag and a place to report progress.
/// Clones share both.
#[derive(Clone)]
pub struct Work {
    cancelled: Arc<AtomicBool>,
    report: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Default for Work {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

impl Work {
    /// `report` receives each progress text, on the thread doing the work.
    pub fn new(report: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            report: Arc::new(report),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// The flag itself, for functions that take `&AtomicBool`.
    pub fn flag(&self) -> &AtomicBool {
        &self.cancelled
    }

    pub fn progress(&self, text: &str) {
        (self.report)(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn clones_share_cancellation_and_progress() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let work = Work::new(move |text| sink.lock().unwrap().push(text.to_owned()));
        let worker = work.clone();
        let thread = std::thread::spawn(move || {
            worker.progress("half");
            while !worker.cancelled() {
                std::thread::yield_now();
            }
            worker.flag().load(Ordering::Acquire)
        });
        work.cancel();
        assert!(thread.join().unwrap());
        assert_eq!(*seen.lock().unwrap(), ["half"]);
    }
}
