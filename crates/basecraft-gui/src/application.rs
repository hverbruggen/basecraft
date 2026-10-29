//! Application lifetime and desktop/settings identity shared by startup and its test.

use cxx_qt_lib::{QGuiApplication, QString};
use cxx_qt_lib_extras::QApplication;

// Linux native GTK dialogs have their own Wayland identity, independent of
// Qt's desktopFileName. GLib is an explicit Linux GUI link dependency.
#[cfg(target_os = "linux")]
#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_set_prgname(name: *const std::ffi::c_char);
}

pub fn new(args: &[String], gui_test: bool) -> cxx::UniquePtr<QApplication> {
    let kind = args
        .windows(2)
        .find(|a| a[0] == "--editor-kind")
        .map(|a| a[1].as_str());
    // A stable Wayland app-id (X11 class): desktop entries and window rules
    // match on it. Validation launches retain their dedicated identity.
    let app_id = match (gui_test, kind) {
        (true, _) => "org.basecraft.Validation",
        (false, Some("Sequence")) => "org.basecraft.Sequence",
        (false, Some("Alignment")) => "org.basecraft.Alignment",
        (false, Some("Tree")) => "org.basecraft.Tree",
        _ => "org.basecraft.Basecraft",
    };
    QGuiApplication::set_desktop_file_name(&QString::from(app_id));
    #[cfg(target_os = "linux")]
    if gui_test {
        // GTK's Wayland toplevels read g_get_prgname(), not desktopFileName.
        // Set it before QApplication initializes the native platform theme.
        // SAFETY: called during startup on the main thread with a static
        // NUL-terminated string; GLib copies it. Normal launches keep their
        // existing native identity and settings migration path.
        unsafe { g_set_prgname(c"org.basecraft.Validation".as_ptr()) };
    }
    // KDE's platform theme requires QApplication for native dialogs. The
    // QGuiApplication-only path falls back to Qt Quick's single-file picker,
    // even when the QML FileDialog requests OpenFiles (QTBUG-92585).
    // The supported binding retains argc/argv for the application's lifetime.
    let mut app = QApplication::new();
    // Retain the legacy QSettings identity for one-time preferences migration.
    if let Some(mut app) = app.as_mut() {
        app.as_mut()
            .set_organization_name(&QString::from("Basecraft"));
        app.as_mut()
            .set_organization_domain(&QString::from("basecraft.org"));
        app.as_mut()
            .set_application_name(&QString::from("Basecraft"));
    }
    app
}
