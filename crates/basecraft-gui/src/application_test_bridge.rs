//! Test binding kept in src because CXX-Qt QML modules allow one Rust directory.

// CXX generates this binding directly against Qt's public header. Only the
// bootstrap test imports this Rust module; no handwritten C++ helper is needed.
#[cxx::bridge]
pub mod qt {
    unsafe extern "C++" {
        include!(<QtCore/QObject>);
        #[cxx_name = "QObject"]
        type TestObject;
        include!("cxx-qt-lib/core/qlist/qlist_QObjectMutPtr.h");
        type QList_QObjectMutPtr = cxx_qt_lib::QList<cxx_qt_lib::QObjectMutPtr>;
        fn children(self: &TestObject) -> &QList_QObjectMutPtr;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        unsafe fn property(self: &TestObject, name: *const c_char) -> QVariant;
        include!(<QtCore/QCoreApplication>);
        type QCoreApplication;
        #[Self = "QCoreApplication"]
        fn instance() -> *mut QCoreApplication;
        #[Self = "QCoreApplication"]
        fn quit();
        unsafe fn inherits(self: &QCoreApplication, class_name: *const c_char) -> bool;
    }
}

#[cfg(feature = "test-counters")]
pub(crate) fn children(object: &cxx_qt::QObject) -> &cxx_qt_lib::QList<cxx_qt_lib::QObjectMutPtr> {
    // SAFETY: both opaque bindings name exactly Qt's QObject. The borrowed
    // list stays tied to the live parent; test callers never process events
    // or delete children while traversing it.
    let object = unsafe { &*std::ptr::from_ref(object).cast::<qt::TestObject>() };
    object.children()
}

#[cfg(feature = "test-counters")]
pub(crate) fn property(object: &cxx_qt::QObject, name: &std::ffi::CStr) -> cxx_qt_lib::QVariant {
    // SAFETY: both bindings name Qt's QObject. The name is NUL-terminated;
    // callers use test-owned properties with passive getters and live owners.
    unsafe { (&*std::ptr::from_ref(object).cast::<qt::TestObject>()).property(name.as_ptr()) }
}
