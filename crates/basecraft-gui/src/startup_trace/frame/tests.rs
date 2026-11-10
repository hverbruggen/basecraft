use super::*;
use std::sync::atomic::AtomicUsize;

fn queued_sample(observation: &Observation, event: FrameEvent, ns: u64) -> Payload {
    let mut sample = None;
    assert_eq!(
        observation.capture(
            event,
            || ns,
            |payload| {
                sample = Some(payload);
                Ok(())
            }
        ),
        CaptureResult::Queued
    );
    sample.unwrap()
}

#[test]
fn disabled_and_rejected_selection_allocate_no_shared_observer() {
    let mut disabled = Selection::new(false);
    assert!(
        disabled
            .select(super::super::MAIN_QML_URL, RootKind::QuickWindow, 1)
            .is_none()
    );
    assert!(disabled.selected.is_none());
    let mut selection = Selection::new(true);
    for (url, kind) in [
        ("other.qml", RootKind::QuickWindow),
        (super::super::MAIN_QML_URL, RootKind::Null),
        (super::super::MAIN_QML_URL, RootKind::OtherObject),
    ] {
        assert!(selection.select(url, kind, 1).is_none());
        assert!(selection.selected.is_none());
    }
    // These are injected type results, not an exercised Qt cast.
    let observation = selection
        .select(super::super::MAIN_QML_URL, RootKind::QuickWindow, 7)
        .unwrap();
    assert_eq!(observation.generation, 7);
    for generation in [7, 8] {
        assert!(
            selection
                .select(
                    super::super::MAIN_QML_URL,
                    RootKind::QuickWindow,
                    generation
                )
                .is_none()
        );
    }
    drop(selection);
    assert!(observation.cancelled.load(Ordering::Acquire));
}

#[test]
fn each_event_is_independent_once_only_in_either_order() {
    for events in [
        [FrameEvent::Submitted, FrameEvent::QueuedForPresentation],
        [FrameEvent::QueuedForPresentation, FrameEvent::Submitted],
    ] {
        let observation = Observation::new(0);
        for (event, time) in [(events[0], 100), (events[1], 0)] {
            let sample = queued_sample(&observation, event, time);
            let slot = &observation.slots[event.index()];
            assert_eq!(slot.captured_ns.load(Ordering::Acquire), time);
            assert_eq!(
                slot.queue.load(Ordering::Acquire),
                QueueOutcome::Accepted as u8
            );
            assert_eq!(
                slot.delivery.load(Ordering::Acquire),
                DeliveryOutcome::Pending as u8
            );
            assert_eq!(
                observation.capture(
                    event,
                    || panic!("duplicate clock"),
                    |_| panic!("duplicate queue")
                ),
                CaptureResult::Duplicate
            );
            assert_eq!(
                observation.deliver(sample, |actual| {
                    assert_eq!(actual, sample);
                    Ok(())
                }),
                DeliveryResult::Delivered
            );
            assert_eq!(
                slot.delivery.load(Ordering::Acquire),
                DeliveryOutcome::Delivered as u8
            );
            assert_eq!(
                observation.deliver(sample, |_| panic!("duplicate sink")),
                DeliveryResult::Duplicate
            );
        }
    }
}

#[test]
fn concurrent_helper_capture_has_one_clock_and_one_queue_winner() {
    let observation = Observation::new(3);
    let clocks = AtomicUsize::new(0);
    let queues = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                observation.capture(
                    FrameEvent::Submitted,
                    || {
                        clocks.fetch_add(1, Ordering::Relaxed);
                        8
                    },
                    |_| {
                        queues.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    },
                );
            });
        }
    });
    assert_eq!(clocks.load(Ordering::Relaxed), 1);
    assert_eq!(queues.load(Ordering::Relaxed), 1);
    // No Qt signal or generated FnMut closure was invoked concurrently.
}

#[test]
fn cancellation_before_capture_and_between_capture_and_queue() {
    let observation = Observation::new(1);
    observation.cancel();
    assert_eq!(
        observation.capture(
            FrameEvent::Submitted,
            || panic!("cancelled clock"),
            |_| panic!("cancelled queue")
        ),
        CaptureResult::Cancelled
    );
    assert_eq!(
        observation.slots[0].queue.load(Ordering::Acquire),
        QueueOutcome::NotAttempted as u8
    );
    let observation = Observation::new(2);
    assert_eq!(
        observation.capture(
            FrameEvent::Submitted,
            || {
                observation.cancel();
                25
            },
            |_| panic!("cancelled queue")
        ),
        CaptureResult::Cancelled
    );
    assert_eq!(observation.slots[0].captured_ns.load(Ordering::Acquire), 25);
    assert_eq!(
        observation.slots[0].queue.load(Ordering::Acquire),
        QueueOutcome::Cancelled as u8
    );
}

#[test]
fn cancellation_can_race_with_queue_but_prevents_later_consumption() {
    let observation = Observation::new(1);
    let mut pending = None;
    assert_eq!(
        observation.capture(
            FrameEvent::Submitted,
            || 9,
            |payload| {
                observation.cancel();
                pending = Some(payload);
                Ok(())
            }
        ),
        CaptureResult::Queued
    );
    assert_eq!(
        observation.deliver(pending.unwrap(), |_| panic!("cancelled sink")),
        DeliveryResult::Cancelled
    );
    assert_eq!(
        observation.slots[0].delivery.load(Ordering::Acquire),
        DeliveryOutcome::Pending as u8
    );
}

#[test]
fn stale_generation_does_not_consume_the_valid_delivery() {
    let observation = Observation::new(8);
    let payload = queued_sample(&observation, FrameEvent::Submitted, 1);
    assert_eq!(
        observation.deliver(
            Payload {
                generation: 7,
                ..payload
            },
            |_| panic!("stale sink")
        ),
        DeliveryResult::StaleGeneration
    );
    assert_eq!(
        observation.deliver(payload, |_| Ok(())),
        DeliveryResult::Delivered
    );
}

#[test]
fn queue_failures_are_terminal_and_are_not_delivery() {
    for (error, outcome) in [
        (QueueFailure::Destroyed, QueueOutcome::Destroyed),
        (QueueFailure::Rejected, QueueOutcome::Rejected),
        (QueueFailure::Unknown, QueueOutcome::Unknown),
    ] {
        let observation = Observation::new(1);
        assert_eq!(
            observation.capture(FrameEvent::Submitted, || u64::MAX, |_| Err(error)),
            CaptureResult::Failed(error)
        );
        assert_eq!(
            observation.slots[0].queue.load(Ordering::Acquire),
            outcome as u8
        );
        assert_eq!(
            observation.slots[0].delivery.load(Ordering::Acquire),
            DeliveryOutcome::Pending as u8
        );
        assert_eq!(
            observation.capture(
                FrameEvent::Submitted,
                || panic!("retry clock"),
                |_| panic!("retry queue")
            ),
            CaptureResult::Duplicate
        );
    }
    // Injected destroyed result, not actual QObject destruction evidence.
}

#[test]
fn accepted_queue_need_not_deliver_and_sink_failure_is_not_retried() {
    let observation = Observation::new(1);
    let sample = queued_sample(&observation, FrameEvent::Submitted, 2);
    assert_eq!(
        observation.slots[0].delivery.load(Ordering::Acquire),
        DeliveryOutcome::Pending as u8
    );
    assert_eq!(
        observation.deliver(sample, |_| Err(())),
        DeliveryResult::SinkFailed
    );
    assert_eq!(
        observation.slots[0].delivery.load(Ordering::Acquire),
        DeliveryOutcome::SinkFailed as u8
    );
    assert_eq!(
        observation.deliver(sample, |_| panic!("retry sink")),
        DeliveryResult::Duplicate
    );
}

#[test]
fn delivery_can_run_before_queue_submission_returns() {
    let observation = Observation::new(1);
    assert_eq!(
        observation.capture(
            FrameEvent::Submitted,
            || 2,
            |payload| {
                assert_eq!(
                    observation.deliver(payload, |_| Ok(())),
                    DeliveryResult::Delivered
                );
                Ok(())
            }
        ),
        CaptureResult::Queued
    );
    assert_eq!(
        observation.slots[0].queue.load(Ordering::Acquire),
        QueueOutcome::Accepted as u8
    );
    assert_eq!(
        observation.slots[0].delivery.load(Ordering::Acquire),
        DeliveryOutcome::Delivered as u8
    );
}

#[test]
fn no_frame_teardown_creates_no_success_and_keeps_owned_state_alive() {
    let mut selection = Selection::new(true);
    let observation = selection
        .select(super::super::MAIN_QML_URL, RootKind::QuickWindow, 2)
        .unwrap();
    drop(selection);
    for slot in &observation.slots {
        assert!(!slot.claimed.load(Ordering::Acquire));
        assert_eq!(
            slot.queue.load(Ordering::Acquire),
            QueueOutcome::NotAttempted as u8
        );
        assert_eq!(
            slot.delivery.load(Ordering::Acquire),
            DeliveryOutcome::Pending as u8
        );
    }
}

#[test]
fn delayed_capture_keeps_its_time_and_does_not_enter_v1_recorder() {
    use super::super::{Event, Recorder};
    let observation = Observation::new(1);
    let sample = queued_sample(&observation, FrameEvent::Submitted, 5);
    let mut bytes = Vec::new();
    let mut recorder = Recorder::new(Some(std::ffi::OsStr::new("run")), 1, &mut bytes);
    recorder.record(Event::MainEntered, 0);
    recorder.record(Event::QmlLoadReturned, 100);
    recorder.record(Event::QmlRootCreated, 4); // v1 still rejects decreasing time.
    let mut received = None;
    assert_eq!(
        observation.deliver(sample, |payload| {
            received = Some(payload);
            Ok(())
        }),
        DeliveryResult::Delivered
    );
    assert_eq!(received.unwrap().captured_ns, 5);
    assert_eq!(std::str::from_utf8(&bytes).unwrap().lines().count(), 2);
}
