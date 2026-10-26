//! Isolated process: Qt application construction and its event loop run on main.

// Match the generated-bridge allowances in the GUI executable.
#![allow(
    clippy::unnecessary_box_returns,
    clippy::unused_self,
    clippy::float_cmp
)]

// The QML module's retained registration object links every controller, even
// though this test loads only a windowless item and unopened operation dialog.
#[path = "../src/alignment_bridge.rs"]
mod alignment_bridge;
#[path = "../src/application.rs"]
mod application;
#[path = "../src/application_test_bridge.rs"]
mod application_test_bridge;
#[path = "../src/bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod bridge;
#[path = "../src/delimitation_bridge.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits test bodies but retains their helpers.
mod delimitation_bridge;
#[path = "../src/operations_bridge.rs"]
mod operations_bridge;
#[path = "../src/provenance_bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod provenance_bridge;
#[path = "../src/sanger_bridge.rs"]
mod sanger_bridge;
#[path = "../src/settings_bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod settings_bridge;
#[path = "../src/tree_bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod tree_bridge;

use cxx_qt_lib::{QByteArray, QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    // The test must never open desktop windows, including when run manually.
    if std::env::var("QT_QPA_PLATFORM").as_deref() != Ok("offscreen")
        || std::env::var("QT_QPA_PLATFORMTHEME").as_deref() != Ok("")
    {
        eprintln!(
            "application_bootstrap requires QT_QPA_PLATFORM=offscreen and QT_QPA_PLATFORMTHEME="
        );
        std::process::exit(1);
    }
    let args = vec![
        "basecraft-gui".into(),
        "--editor-kind".into(),
        "Alignment".into(),
    ];
    bridge::qobject::configure_canvas_graphics();
    let mut app = application::new(&args, true);
    let mut app = app.as_mut().expect("Qt application");
    // This is KDE's native-dialog prerequisite. The former QGuiApplication
    // constructor fails this assertion even though it can run QML normally.
    // SAFETY: app owns Qt's singleton throughout this call; the C string is
    // NUL-terminated and lives across the synchronous inherits() call.
    assert!(unsafe {
        application_test_bridge::qt::QCoreApplication::instance()
            .as_ref()
            .expect("Qt singleton")
            .inherits(c"QApplication".as_ptr())
    });
    assert_eq!(app.application_name().to_string(), "Basecraft");
    assert_eq!(app.organization_name().to_string(), "Basecraft");
    assert_eq!(app.organization_domain().to_string(), "basecraft.org");
    assert_eq!(
        QGuiApplication::desktop_file_name().to_string(),
        "org.basecraft.Validation"
    );
    let font = app.font();
    assert_ne!(
        font.family().expect("application font family").to_string(),
        ""
    );

    // Exercise QML initialization and the real QApplication event loop without
    // loading the window or opening any picker. Drop the engine before the app.
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_creation_failed(|_, _| {
            eprintln!("Could not load bootstrap test QML");
            std::process::exit(1);
        })
        .release();
    engine.pin_mut().load_data(
        &QByteArray::from(
            "import QtQuick; import Basecraft; Item { width: 800; height: 600; \
             OperationPickerDialog {} \
             OperationParameters { schema: ({type: 'object', properties: {age: {type: 'number'}}, required: ['age']}) } \
             Timer { interval: 1; running: true; onTriggered: Qt.quit() } }",
        ),
        &QUrl::default(),
    );
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(15));
        eprintln!("Bootstrap QML timer did not exit the event loop within 15 seconds");
        std::process::exit(1);
    });
    assert_eq!(app.as_mut().exec(), 0);
    println!("QApplication identity, settings, font, QML initialization and event loop passed");
}
