//! Qt/QML desktop application.

// cxx-qt's generated constructors box their return values, and QML
// invokables are methods even when they don't need `self`; its generated
// property setters compare floats exactly.
#![allow(
    clippy::unnecessary_box_returns,
    clippy::unused_self,
    clippy::float_cmp
)]

mod alignment_bridge;
mod bridge;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    let mut app = QGuiApplication::new();
    // A stable Wayland app-id (X11 class): desktop entries and window rules
    // match on it.
    QGuiApplication::set_desktop_file_name(&QString::from("org.basecraft.Basecraft"));
    // Settings are stored under these names (~/.config/Basecraft/).
    if let Some(mut app) = app.as_mut() {
        app.as_mut()
            .set_organization_name(&QString::from("Basecraft"));
        app.as_mut()
            .set_organization_domain(&QString::from("basecraft.org"));
        app.as_mut()
            .set_application_name(&QString::from("Basecraft"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/Basecraft/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
