//! Compile-only Stage A adapter: no caller constructs a receiver or subscribes.
//! Generated bindings borrow the existing window; they never acquire ownership.

use super::frame::{Observation, Payload, QueueFailure, RootKind};
use cxx_qt::{CxxQtThread, CxxQtType, QMetaObjectConnectionGuard, casting::Downcast};
use std::sync::Arc;

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++Qt" {
        include!(<QtQuick/QQuickWindow>);
        #[qobject]
        type QQuickWindow;

        // Connection-only declarations: do not generate a callable const emitter
        // for Qt's non-const signals. Never use the sender borrow on the render thread.
        #[qsignal]
        #[rust_name = "after_frame_end"]
        pub(self) fn afterFrameEnd(self: &QQuickWindow);
        #[qsignal]
        #[rust_name = "frame_swapped"]
        pub(self) fn frameSwapped(self: &QQuickWindow);
    }

    impl cxx_qt::Threading for FrameReceiver {}
    unsafe extern "RustQt" {
        #[qobject]
        type FrameReceiver = super::Receiver;
    }
}

/// Bounded in-memory endpoint for the typed queue proof, not a logger or protocol.
#[derive(Default)]
pub struct Receiver {
    received: [Option<Payload>; 2],
}

/// Caller supplies only a live borrow; no raw conversion or retained Qt pointer.
fn checked_window(object: Option<&cxx_qt::QObject>) -> (RootKind, Option<&ffi::QQuickWindow>) {
    let Some(object) = object else {
        return (RootKind::Null, None);
    };
    match object.downcast::<ffi::QQuickWindow>() {
        Some(window) => (RootKind::QuickWindow, Some(window)),
        None => (RootKind::OtherObject, None),
    }
}

/// Signatures and generated connectors are linked but never called in Stage A.
/// Future callers must ensure serialized emissions per handler; atomic state
/// does not make simultaneous entry into the generated FnMut closure safe.
fn connect_submission<F: FnMut() + Send + 'static>(
    window: &ffi::QQuickWindow,
    callback: F,
) -> QMetaObjectConnectionGuard {
    let mut callback = callback;
    window.connect_after_frame_end(
        move |_| callback(),
        cxx_qt::ConnectionType::DirectConnection,
    )
}

fn connect_presentation_queue<F: FnMut() + Send + 'static>(
    window: &ffi::QQuickWindow,
    callback: F,
) -> QMetaObjectConnectionGuard {
    let mut callback = callback;
    window.connect_frame_swapped(
        move |_| callback(),
        cxx_qt::ConnectionType::DirectConnection,
    )
}

/// CxxQtThread queues with guarded pointer/mutex protection; acceptance is not
/// delivery. It may allocate/wait. No is_destroyed precheck, lock or retry here.
fn queue_payload(
    thread: &CxxQtThread<ffi::FrameReceiver>,
    observation: Arc<Observation>,
    payload: Payload,
) -> Result<(), QueueFailure> {
    thread
        .queue(move |mut receiver| {
            observation.deliver(payload, |sample| {
                receiver.as_mut().rust_mut().received[sample.event.index()] = Some(sample);
                Ok(())
            });
        })
        .map_err(|error| match error {
            cxx_qt::ThreadingQueueError::ObjectDestroyed => QueueFailure::Destroyed,
            cxx_qt::ThreadingQueueError::InvokeMethodFailed => QueueFailure::Rejected,
            _ => QueueFailure::Unknown,
        })
}

#[test]
fn signatures_link_without_creating_qt_objects() {
    // Keep real generated call paths reachable to the linker, without executing
    // any cast/connection/queue. This proves types/linkage, not Qt runtime safety.
    std::hint::black_box(checked_window as fn(_) -> _);
    std::hint::black_box(connect_submission::<fn()> as fn(_, _) -> _);
    std::hint::black_box(connect_presentation_queue::<fn()> as fn(_, _) -> _);
    std::hint::black_box(queue_payload as fn(_, _, _) -> _);
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<CxxQtThread<ffi::FrameReceiver>>();
    require_send_sync::<Payload>();
}
