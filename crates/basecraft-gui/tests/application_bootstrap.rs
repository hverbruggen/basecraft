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
#[path = "../src/figure_png.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod figure_png;
#[path = "../src/metadata_bridge.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod metadata_bridge;
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
#[path = "../src/submission_bridge.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod submission_bridge;
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

    let figure = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect width=\"200\" height=\"100\" fill=\"#c02030\"/><text x=\"8\" y=\"50\" font-size=\"20\">A&amp;β</text></svg>";
    let png = figure_png::render(figure, 1500.0).expect("resvg renders editable SVG text");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1500);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 750);

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
    let sanger_folder = tempfile::tempdir().expect("Sanger flow folder");
    let trace_path = sanger_folder.path().join("trace.ab1");
    std::fs::write(
        &trace_path,
        include_bytes!("../../../testdata/fixtures/compact/sanger-review/forward.ab1"),
    )
    .unwrap();
    let saved_path = sanger_folder.path().join("saved-review.json");
    let sanger_qml = format!(
        r#"
        SangerController {{ id: sanger }}
        SangerController {{ id: reopened }}
        Component.onCompleted: {{
            function check(value, message) {{ if (!value) {{ console.error(message); Qt.exit(2) }} }}
            check(sanger.open_file({}), "AB1 open failed");
            check(sanger.review_mode, "AB1 must enter review immediately");
            check(sanger.calling, "Alternative calling must be queued");
            sanger.cancel_calls();
            check(!sanger.calling, "Cancel must detach queued work");
            check(sanger.review_set("edit", "G"), "Instrument review edit failed");
            check(sanger.save_review({}), "Review save failed");
            check(reopened.open_file({}), "Normal open of saved review failed");
            check(reopened.review_decision.indexOf("G") >= 0, "Manual decision lost on reopen");
        }}
    "#,
        serde_json::to_string(&trace_path).unwrap(),
        serde_json::to_string(&saved_path).unwrap(),
        serde_json::to_string(&saved_path).unwrap()
    );
    let figure_folder = tempfile::tempdir().expect("figure export folder");
    for (name, bytes) in [
        (
            "sequence.fasta",
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.fasta")
                .as_slice(),
        ),
        (
            "alignment.afa",
            include_bytes!("../../../testdata/fixtures/compact/species-delimitation/specimens.afa")
                .as_slice(),
        ),
        (
            "tree.nwk",
            include_bytes!(
                "../../../testdata/fixtures/compact/species-delimitation/substitutions.nwk"
            )
            .as_slice(),
        ),
        (
            "hypotheses.spart",
            include_bytes!(
                "../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
            )
            .as_slice(),
        ),
        (
            "trace.ab1",
            include_bytes!("../../../testdata/fixtures/compact/sanger-review/forward.ab1")
                .as_slice(),
        ),
    ] {
        std::fs::write(figure_folder.path().join(name), bytes).unwrap();
    }
    for index in 0..5 {
        std::fs::write(
            figure_folder
                .path()
                .join(format!("figure-{index}-protected.png")),
            b"existing export",
        )
        .unwrap();
    }
    let figure_qml = include_str!("fixtures/figure-controllers.qml").replace(
        "FIGURE_TEST_FOLDER",
        &serde_json::to_string(figure_folder.path()).unwrap(),
    );
    engine.pin_mut().load_data(
        &QByteArray::from(&format!(
            "import QtQuick; import Basecraft; Item {{ width: 800; height: 600; \
             OperationPickerDialog {{}} \
             OperationParameters {{ schema: ({{type: 'object', properties: {{age: {{type: 'number'}}}}, required: ['age']}}) }} \
             {sanger_qml} {figure_qml} \
             Timer {{ interval: 1; running: true; onTriggered: Qt.quit() }} }}",
        )),
        &QUrl::default(),
    );
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(15));
        eprintln!("Bootstrap QML timer did not exit the event loop within 15 seconds");
        std::process::exit(1);
    });
    assert_eq!(app.as_mut().exec(), 0);
    for index in 0..5 {
        let prefix = figure_folder.path().join(format!("figure-{index}"));
        let svg = std::fs::read_to_string(prefix.with_extension("svg")).unwrap();
        assert!(
            svg.contains("<text"),
            "controller {index} must retain editable labels"
        );
        assert!(
            !svg.contains("<image"),
            "controller {index} must remain vector"
        );
        for width in [1500, 320] {
            let png = std::fs::read(
                figure_folder
                    .path()
                    .join(format!("figure-{index}-{width}.png")),
            )
            .unwrap();
            assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), width);
            assert_eq!(
                png,
                figure_png::render(&svg, f64::from(width)).unwrap(),
                "controller {index}: PNG must rasterize the exact exported SVG"
            );
        }
        assert_eq!(
            std::fs::read(
                figure_folder
                    .path()
                    .join(format!("figure-{index}-protected.png"))
            )
            .unwrap(),
            b"existing export"
        );
        if let Some(folder) = std::env::var_os("BASECRAFT_FIGURE_EVIDENCE") {
            std::fs::create_dir_all(&folder).unwrap();
            for suffix in [".svg", "-1500.png", "-320.png"] {
                let name = format!("figure-{index}{suffix}");
                std::fs::copy(
                    figure_folder.path().join(&name),
                    std::path::Path::new(&folder).join(name),
                )
                .unwrap();
            }
        }
    }
    println!(
        "QApplication identity, settings, font, QML event loop and five-controller figure exports passed"
    );
}
