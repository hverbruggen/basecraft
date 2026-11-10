//! Stage A observation state. No Qt objects, clocks, subscriptions or output.
//! A sample means first observed after attachment, never first presentation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameEvent {
    Submitted,
    QueuedForPresentation,
}

impl FrameEvent {
    pub fn index(self) -> usize {
        match self {
            Self::Submitted => 0,
            Self::QueuedForPresentation => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Payload {
    pub generation: u64,
    pub event: FrameEvent,
    pub captured_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    Null,
    OtherObject,
    QuickWindow,
}

/// Selection is GUI-owned. Type results are supplied by the checked Qt adapter.
pub struct Selection {
    enabled: bool,
    selected: Option<Arc<Observation>>,
}

impl Selection {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            selected: None,
        }
    }

    pub fn select(
        &mut self,
        url: &str,
        root: RootKind,
        generation: u64,
    ) -> Option<Arc<Observation>> {
        if !self.enabled
            || self.selected.is_some()
            || url != super::MAIN_QML_URL
            || root != RootKind::QuickWindow
        {
            return None;
        }
        let observation = Arc::new(Observation::new(generation));
        self.selected = Some(Arc::clone(&observation));
        Some(observation)
    }
}

impl Drop for Selection {
    fn drop(&mut self) {
        if let Some(selected) = &self.selected {
            selected.cancel();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueFailure {
    Destroyed,
    Rejected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QueueOutcome {
    NotAttempted,
    Accepted,
    Destroyed,
    Rejected,
    Unknown,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryOutcome {
    Pending,
    Delivered,
    SinkFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureResult {
    Duplicate,
    Cancelled,
    Queued,
    Failed(QueueFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryResult {
    Cancelled,
    StaleGeneration,
    Duplicate,
    Delivered,
    SinkFailed,
}

#[derive(Default)]
struct Slot {
    claimed: AtomicBool,
    captured_ns: AtomicU64,
    queue: AtomicU8,
    delivery_claimed: AtomicBool,
    delivery: AtomicU8,
}

pub struct Observation {
    generation: u64,
    cancelled: AtomicBool,
    slots: [Slot; 2],
}

impl Observation {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            cancelled: AtomicBool::new(false),
            slots: [Slot::default(), Slot::default()],
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// At most one timestamp capture and queue attempt per event. Queue acceptance
    /// is not delivery. A concurrent cancel can race with an already claimed
    /// attempt; the receiver must check cancellation again before consuming it.
    /// Contention tests exercise this helper, never concurrent Qt `FnMut` handlers.
    pub fn capture(
        &self,
        event: FrameEvent,
        timestamp: impl FnOnce() -> u64,
        queue: impl FnOnce(Payload) -> Result<(), QueueFailure>,
    ) -> CaptureResult {
        if self.cancelled.load(Ordering::Acquire) {
            return CaptureResult::Cancelled;
        }
        let slot = &self.slots[event.index()];
        if slot.claimed.swap(true, Ordering::AcqRel) {
            return CaptureResult::Duplicate;
        }
        let captured_ns = timestamp();
        slot.captured_ns.store(captured_ns, Ordering::Release);
        if self.cancelled.load(Ordering::Acquire) {
            slot.queue
                .store(QueueOutcome::Cancelled as u8, Ordering::Release);
            return CaptureResult::Cancelled;
        }
        let result = queue(Payload {
            generation: self.generation,
            event,
            captured_ns,
        });
        let (outcome, result) = match result {
            Ok(()) => (QueueOutcome::Accepted, CaptureResult::Queued),
            Err(QueueFailure::Destroyed) => (
                QueueOutcome::Destroyed,
                CaptureResult::Failed(QueueFailure::Destroyed),
            ),
            Err(QueueFailure::Rejected) => (
                QueueOutcome::Rejected,
                CaptureResult::Failed(QueueFailure::Rejected),
            ),
            Err(QueueFailure::Unknown) => (
                QueueOutcome::Unknown,
                CaptureResult::Failed(QueueFailure::Unknown),
            ),
        };
        slot.queue.store(outcome as u8, Ordering::Release);
        result
    }

    /// The future GUI receiver serializes delivery with its own teardown. This
    /// check cannot retract a sink call already in progress when cancel races.
    /// No recorder lock is held, and capture time is never replaced by delivery time.
    pub fn deliver(
        &self,
        payload: Payload,
        sink: impl FnOnce(Payload) -> Result<(), ()>,
    ) -> DeliveryResult {
        if payload.generation != self.generation {
            return DeliveryResult::StaleGeneration;
        }
        if self.cancelled.load(Ordering::Acquire) {
            return DeliveryResult::Cancelled;
        }
        let slot = &self.slots[payload.event.index()];
        if slot.delivery_claimed.swap(true, Ordering::AcqRel) {
            return DeliveryResult::Duplicate;
        }
        let (outcome, result) = if sink(payload).is_ok() {
            (DeliveryOutcome::Delivered, DeliveryResult::Delivered)
        } else {
            (DeliveryOutcome::SinkFailed, DeliveryResult::SinkFailed)
        };
        slot.delivery.store(outcome as u8, Ordering::Release);
        result
    }
}

#[cfg(test)]
mod tests;
