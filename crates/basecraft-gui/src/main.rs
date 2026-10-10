//! Qt/QML desktop application.

// cxx-qt's generated constructors box their return values, and QML
// invokables are methods even when they don't need `self`.
#![allow(clippy::unnecessary_box_returns, clippy::unused_self)]

mod bridge;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/Basecraft/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
