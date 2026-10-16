//! Test binding kept in src because CXX-Qt QML modules allow one Rust directory.

// CXX generates this binding directly against Qt's public header. Only the
// bootstrap test imports this Rust module; no handwritten C++ helper is needed.
#[cxx::bridge]
pub mod qt {
    unsafe extern "C++" {
        include!(<QtCore/QCoreApplication>);
        type QCoreApplication;
        #[Self = "QCoreApplication"]
        fn instance() -> *mut QCoreApplication;
        unsafe fn inherits(self: &QCoreApplication, class_name: *const c_char) -> bool;
    }
}
