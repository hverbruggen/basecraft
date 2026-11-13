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
mod app_bridge;
mod appearance;
mod application;
mod bridge;
mod chosen_project;
mod command_bridge;
mod delimitation_bridge;
mod document_jobs;
mod document_open;
mod editor_save;
mod figure_png;
mod find_bridge;
mod jobs;
mod metadata_bridge;
mod operations_bridge;
mod plugins;
mod provenance_bridge;
mod release_check;
mod sanger_bridge;
mod settings_bridge;
mod startup_trace;
mod submission_bridge;
mod tree_bridge;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};

fn main() {
    let startup = std::time::Instant::now();
    let trace = startup_trace::Trace::from_env(startup);
    bridge::qobject::configure_canvas_graphics();
    let args: Vec<_> = std::env::args().collect();
    // The check tools drive the window through tools/ScriptedRun.qml.
    let scripted = args.iter().any(|arg| arg == "--screenshot");
    let root = if scripted {
        "qrc:/qt/qml/Basecraft/tools/ScriptedRun.qml"
    } else {
        startup_trace::MAIN_QML_URL
    };
    let gui_test =
        std::env::var_os("BASECRAFT_GUI_TEST").is_some_and(|value| value == "1") || scripted;
    let mut app = application::new(&args, gui_test);
    trace.record(if app.is_null() {
        startup_trace::Event::ApplicationCreationFailed
    } else {
        startup_trace::Event::ApplicationCreated
    });
    let mut engine = QQmlApplicationEngine::new();
    trace.record(if engine.is_null() {
        startup_trace::Event::EngineCreationFailed
    } else {
        startup_trace::Event::EngineCreated
    });
    if let Some(mut engine) = engine.as_mut() {
        if trace.enabled() {
            let root_trace = trace.clone();
            engine
                .as_mut()
                .on_object_created(move |_, object, url| {
                    if *url == QUrl::from(root) && !object.is_null() {
                        root_trace.record(startup_trace::Event::QmlRootCreated);
                        // SAFETY: objectCreated supplies a live Qt-owned object.
                        unsafe {
                            startup_trace::frame_bridge::report_objects(object);
                            startup_trace::frame_bridge::attach(object, root_trace.clone());
                        }
                    }
                })
                .release();
        }
        let failure_trace = trace.clone();
        engine
            .as_mut()
            .on_object_creation_failed(move |_, url| {
                if failure_trace.enabled() && *url == QUrl::from(root) {
                    failure_trace.record(startup_trace::Event::QmlCreationFailed);
                }
                eprintln!("Basecraft could not load its interface; check the Qt Quick and Kirigami installation. See the QML errors above.");
                std::process::exit(1);
            })
            .release();
        engine.load(&QUrl::from(root));
        trace.record(startup_trace::Event::QmlLoadReturned);
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
