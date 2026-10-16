//! Application lifetime and desktop/settings identity shared by startup and its test.

use cxx_qt_lib::{QGuiApplication, QString};
use cxx_qt_lib_extras::QApplication;

pub fn new(args: &[String], gui_test: bool) -> cxx::UniquePtr<QApplication> {
    // KDE's platform theme requires QApplication for native dialogs. The
    // QGuiApplication-only path falls back to Qt Quick's single-file picker,
    // even when the QML FileDialog requests OpenFiles (QTBUG-92585).
    // The supported binding retains argc/argv for the application's lifetime.
    let mut app = QApplication::new();
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
