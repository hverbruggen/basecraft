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
mod application;
mod bridge;
mod delimitation_bridge;
mod figure_png;
mod operations_bridge;
mod provenance_bridge;
mod sanger_bridge;
mod settings_bridge;
mod tree_bridge;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};

fn main() {
    bridge::qobject::configure_canvas_graphics();
    let args: Vec<_> = std::env::args().collect();
    let gui_test = std::env::var_os("BASECRAFT_GUI_TEST").is_some_and(|value| value == "1")
        || args.iter().any(|arg| arg == "--screenshot");
    let mut app = application::new(&args, gui_test);
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
