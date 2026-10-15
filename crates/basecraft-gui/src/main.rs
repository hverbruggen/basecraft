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
mod delimitation_bridge;
mod provenance_bridge;
mod sanger_bridge;
mod settings_bridge;
mod tree_bridge;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    bridge::qobject::configure_canvas_graphics();
    let mut app = QGuiApplication::new();
    // A stable Wayland app-id (X11 class): desktop entries and window rules
    // match on it.
    let args: Vec<_> = std::env::args().collect();
    let kind = args
        .windows(2)
        .find(|a| a[0] == "--editor-kind")
        .map(|a| a[1].as_str());
    let app_id = match kind {
        Some("Sequence") => "org.basecraft.Sequence",
        Some("Alignment") => "org.basecraft.Alignment",
        Some("Tree") => "org.basecraft.Tree",
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
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        engine
            .as_mut()
            .on_object_creation_failed(|_, _| {
                eprintln!("Basecraft could not load its interface; check the Qt Quick and Kirigami installation. See the QML errors above.");
                std::process::exit(1);
            })
            .release();
        engine.load(&QUrl::from("qrc:/qt/qml/Basecraft/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
