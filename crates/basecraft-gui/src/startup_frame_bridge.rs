//! Observe the first frame directly; no receiver object or polling timer.
use cxx_qt::casting::Downcast;

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++Qt" {
        include!(<QtQuick/QQuickWindow>);
        #[qobject]
        type QQuickWindow;
        #[qsignal]
        #[rust_name = "frame_swapped"]
        pub(self) fn frameSwapped(self: &QQuickWindow);
    }
}

unsafe extern "C" {
    fn basecraft_startup_object_count(root: *mut cxx_qt::QObject) -> i32;
}

/// Print how many objects the window built at startup.
///
/// # Safety
/// `object` is the live root object `objectCreated` supplied.
pub unsafe fn report_objects(object: *mut cxx_qt::QObject) {
    // SAFETY: the caller passes the live root object on its own thread.
    let count = unsafe { basecraft_startup_object_count(object) };
    eprintln!("basecraft-gui-startup-objects {count}");
}

/// Called synchronously from `QQmlApplicationEngine::objectCreated`. Qt owns the
/// object. The signal closure owns only the thread-safe recorder, never a pointer.
pub unsafe fn attach(object: *mut cxx_qt::QObject, trace: super::Trace) {
    // SAFETY: the engine's creation callback supplies a live object for this call.
    let Some(object) = (unsafe { object.as_ref() }) else {
        return;
    };
    if let Some(window) = object.downcast::<ffi::QQuickWindow>() {
        window
            .connect_frame_swapped(
                move |_| trace.record(super::Event::FirstFrame),
                cxx_qt::ConnectionType::DirectConnection,
            )
            .release();
    }
}
