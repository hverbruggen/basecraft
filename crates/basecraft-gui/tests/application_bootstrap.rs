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
#[path = "../src/app_bridge.rs"]
#[allow(unused_imports)] // harness=false omits pure predicate test bodies.
mod app_bridge;
#[path = "../src/appearance.rs"]
#[allow(dead_code, unused_imports)] // harness=false omits the included module's test bodies.
mod appearance;
#[path = "../src/application.rs"]
mod application;
#[path = "../src/application_test_bridge.rs"]
mod application_test_bridge;
#[path = "../src/bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod bridge;
#[path = "../src/chosen_project.rs"]
#[allow(dead_code)] // harness=false omits the included module's test bodies.
mod chosen_project;
#[path = "../src/command_bridge.rs"]
mod command_bridge;
#[path = "controller_appearance/mod.rs"]
mod controller_appearance;
#[path = "../src/delimitation_bridge.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits test bodies but retains their helpers.
mod delimitation_bridge;
#[path = "../src/document_jobs.rs"]
mod document_jobs;
#[path = "../src/document_open.rs"]
mod document_open;
#[path = "../src/editor_save.rs"]
mod editor_save;
#[path = "../src/figure_png.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod figure_png;
#[path = "../src/find_bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod find_bridge;
#[path = "../src/jobs.rs"]
#[allow(dead_code, unused_imports)] // harness=false omits the included module's test bodies.
mod jobs;
#[path = "../src/metadata_bridge.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod metadata_bridge;
#[cfg(feature = "test-counters")]
mod open_argv;
#[cfg(feature = "test-counters")]
mod open_hooks;
#[path = "../src/operations_bridge.rs"]
mod operations_bridge;
#[path = "../src/plugins.rs"]
#[allow(unused_imports, dead_code)] // harness=false omits the included module's test bodies.
mod plugins;
#[path = "../src/provenance_bridge.rs"]
#[allow(unused_imports)] // harness=false omits the included module's #[test] bodies.
mod provenance_bridge;
#[path = "../src/release_check.rs"]
mod release_check;
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

#[path = "../src/startup_trace.rs"]
#[allow(dead_code, unused_imports)]
mod startup_trace;

use cxx_qt_lib::{QByteArray, QGuiApplication, QQmlApplicationEngine, QUrl};
#[cfg(feature = "test-counters")]
use std::path::Path;

#[cfg(target_os = "linux")]
#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_set_prgname(name: *const std::ffi::c_char);
    fn g_get_prgname() -> *const std::ffi::c_char;
}

/// The production window in its own process, with one real document of each
/// kind; see fixtures/ribbon-walk.qml. Exits with the event loop's code.
fn ribbon_walk(folder: &str) -> ! {
    bridge::qobject::configure_canvas_graphics();
    let mut app = application::new(&["basecraft-gui".into()], true);
    let mut app = app.as_mut().expect("Qt application");
    let mut engine = QQmlApplicationEngine::new();
    let source = include_str!("fixtures/ribbon-walk.qml").replace(
        "RIBBON_TEST_FOLDER",
        &serde_json::to_string(folder).unwrap(),
    );
    engine
        .pin_mut()
        .on_object_creation_failed(|_, _| std::process::exit(4))
        .release();
    engine
        .pin_mut()
        .load_data(&QByteArray::from(source.as_str()), &QUrl::default());
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(180));
        eprintln!("RIBBON_WALK did not finish within 180 seconds");
        std::process::exit(5);
    });
    std::process::exit(app.as_mut().exec());
}

/// Every ribbon tab of every document kind on the real window, with no QML
/// warning on the way.
fn ribbon_journey() {
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
    for (set, file) in [
        ("sequences-annotations", "locus.gb"),
        ("alignment-editing", "homologues.afa"),
        ("species-delimitation", "time.nwk"),
        ("species-delimitation", "hypotheses.spart"),
        ("species-delimitation", "specimens.afa"),
        ("sanger-review", "forward.ab1"),
    ] {
        std::fs::copy(fixtures.join(set).join(file), folder.path().join(file)).unwrap();
    }
    // The project the journey switches to last.
    std::fs::create_dir(folder.path().join("second")).unwrap();
    // A folder the journey cannot save into, and a plugin whose run never ends.
    // Outside the chosen project: a project file keeps its recovery in the
    // project's `.basecraft`, so only a loose file meets a read-only folder.
    let locked = std::path::PathBuf::from(format!("{}-locked", folder.path().display()));
    std::fs::create_dir(&locked).unwrap();
    std::fs::copy(
        folder.path().join("homologues.afa"),
        locked.join("locked.afa"),
    )
    .unwrap();
    for file in ["locus.gb", "time.nwk", "forward.ab1"] {
        std::fs::copy(folder.path().join(file), locked.join(file)).unwrap();
    }
    // A rooted tree for the label questions a reroot asks (D11).
    std::fs::write(folder.path().join("rooted.nwk"), "[&R] ((a,b),(c,d));\n").unwrap();
    // A few letters only proteins use, 1% or fewer: undetermined (D12).
    std::fs::write(
        folder.path().join("unclear.fasta"),
        format!(">unclear\n{}E{}\n", "ACGT".repeat(25), "ACGT".repeat(25)),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let plugin = folder.path().join("fixture-plugin");
    std::fs::create_dir(&plugin).unwrap();
    for file in ["plugin.toml", "operations.json", "plugin.py"] {
        std::fs::copy(
            fixtures.join("../registry-plugin").join(file),
            plugin.join(file),
        )
        .unwrap();
    }
    basecraft_plugin_host::registry::register_editable(
        &basecraft_plugin_host::registry::RegistryConfig::discover().unwrap(),
        &plugin,
    )
    .unwrap();
    let original = std::fs::read_to_string(folder.path().join("homologues.afa")).unwrap();
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ribbon-walk", folder.path().to_str().unwrap()])
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log = String::from_utf8_lossy(&run.stderr);
    // Layout loops that predate this test: forms sized from their content and
    // the Sanger review's canvas. Anything else fails the journey.
    let known = |line: &str| {
        line.contains("Dialog: Binding loop detected for property \"implicitWidth\"")
            || line.contains("SangerView.qml")
            || line.contains("QQuickAnchors: Binding loop detected for property \"bottomMargin\"")
    };
    let warnings: Vec<_> = log
        .lines()
        .filter(|line| {
            [
                "TypeError",
                "ReferenceError",
                "Binding loop",
                "Unable to assign",
                "is not a function",
                "failed",
            ]
            .iter()
            .any(|word| line.contains(word))
                && !known(line)
        })
        .collect();
    assert!(
        run.status.success() && log.contains("RIBBON_WALK passed") && warnings.is_empty(),
        "ribbon journey failed ({:?}); warnings: {warnings:#?}\n{log}",
        run.status
    );
    // The journey saved one inserted gap: same records and residues, the text
    // changed. Nothing reached the read-only folder.
    let saved = std::fs::read_to_string(folder.path().join("homologues.afa")).unwrap();
    let records = |text: &str| -> Vec<(String, String)> {
        text.split('>')
            .skip(1)
            .map(|record| {
                let (name, rows) = record.split_once('\n').unwrap_or((record, ""));
                (
                    name.trim().to_owned(),
                    rows.chars().filter(|c| !c.is_whitespace()).collect(),
                )
            })
            .collect()
    };
    let ungapped = |records: Vec<(String, String)>| -> Vec<(String, String)> {
        records
            .into_iter()
            .map(|(name, row)| (name, row.replace('-', "")))
            .collect()
    };
    assert_ne!(
        records(&saved),
        records(&original),
        "the journey's save wrote nothing"
    );
    assert_eq!(ungapped(records(&saved)), ungapped(records(&original)));
    let gaps = |text: &str| {
        records(text)
            .iter()
            .map(|(_, row)| row.matches('-').count())
            .sum::<usize>()
    };
    assert!(
        gaps(&saved) > gaps(&original),
        "the saved alignment has no new gap"
    );
    assert_eq!(
        std::fs::read(locked.join("locked.afa")).unwrap(),
        original.as_bytes()
    );
    // D13: the copy holds the edit the read-only folder refused, and saved.
    let copied = std::fs::read_to_string(folder.path().join("copied.afa")).unwrap();
    assert_eq!(ungapped(records(&copied)), ungapped(records(&original)));
    assert!(
        gaps(&copied) > gaps(&original),
        "the copy lacks the redone gap"
    );
    for file in ["locus.gb", "time.nwk", "forward.ab1"] {
        assert_eq!(
            std::fs::read(locked.join(file)).unwrap(),
            std::fs::read(folder.path().join(file)).unwrap(),
            "{file} in the read-only folder changed"
        );
    }
    let _ = folder.keep();
    println!("Every ribbon tab of every document kind showed its commands, without QML warnings");
}

fn check_identity_case(case: &str) {
    let (gui_test, kind, expected) = match case {
        "validation" => (true, None, "org.basecraft.Validation"),
        "validation-editor" => (true, Some("Alignment"), "org.basecraft.Validation"),
        "normal" => (false, None, "org.basecraft.Basecraft"),
        "sequence" => (false, Some("Sequence"), "org.basecraft.Sequence"),
        "alignment" => (false, Some("Alignment"), "org.basecraft.Alignment"),
        "tree" => (false, Some("Tree"), "org.basecraft.Tree"),
        "unknown" => (false, Some("Unknown"), "org.basecraft.Basecraft"),
        _ => panic!("unknown identity case: {case}"),
    };
    let mut args = vec!["basecraft-gui".into()];
    if let Some(kind) = kind {
        args.extend(["--editor-kind".into(), kind.into()]);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: a static NUL-terminated string, set on the process main thread
    // before Qt/GTK initialization. Each case has its own process.
    unsafe {
        g_set_prgname(c"basecraft-identity-sentinel".as_ptr());
    }
    let app = application::new(&args, gui_test);
    let app = app.as_ref().expect("Qt application");
    assert_eq!(app.application_name().to_string(), "Basecraft");
    assert_eq!(app.organization_name().to_string(), "Basecraft");
    assert_eq!(app.organization_domain().to_string(), "basecraft.org");
    assert_eq!(QGuiApplication::desktop_file_name().to_string(), expected);
    #[cfg(target_os = "linux")]
    {
        // SAFETY: GLib owns this NUL-terminated string, seeded above, and no
        // thread changes it while we read. No GTK window/display is created.
        let name = unsafe { std::ffi::CStr::from_ptr(g_get_prgname()) };
        assert_eq!(
            name.to_str().unwrap(),
            if gui_test {
                "org.basecraft.Validation"
            } else {
                "basecraft-identity-sentinel"
            },
            "native dialog identity for {case}"
        );
    }
    println!("Identity case {case} passed");
}

// One real-Main journey, with Rust-only state inspection between event-loop
// checkpoints. No Qt events or deletion occur while controller borrows are held.
#[cfg(feature = "test-counters")]
fn linked_taxa_journey(mut app: std::pin::Pin<&mut cxx_qt_lib_extras::QApplication>) {
    use cxx_qt::casting::Downcast;
    use cxx_qt_lib::QObjectExt;
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    fn files(folder: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut result = BTreeMap::new();
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.extend(files(&path));
            } else {
                result.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        result
    }
    let folder = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact/species-delimitation");
    for (source, name) in [
        ("specimens.afa", "alignment.afa"),
        ("specimens.afa", "unrelated.afa"),
        ("specimens.afa", "pending.afa"),
        ("substitutions.nwk", "tree.nwk"),
        ("substitutions.nwk", "unlinked.nwk"),
        ("hypotheses.spart", "workspace.spart"),
    ] {
        std::fs::copy(fixtures.join(source), folder.path().join(name)).unwrap();
    }
    let workspace = folder.path().join("workspace.spart");
    let mut work = basecraft_ops::files::load_delimitation(&workspace).unwrap();
    work.exchange_fields.insert(
        "basecraft_alignment_snapshot".into(),
        folder
            .path()
            .join("alignment.afa")
            .to_string_lossy()
            .into_owned(),
    );
    work.view.display_tree = Some(
        folder
            .path()
            .join("tree.nwk")
            .to_string_lossy()
            .into_owned(),
    );
    let hypothesis = &work.hypotheses[0];
    let group = &hypothesis.groups[0];
    let source = include_str!("fixtures/linked-taxa-controllers.qml")
        .replace(
            "LINKED_TEST_FOLDER",
            &serde_json::to_string(folder.path()).unwrap(),
        )
        .replace(
            "LINKED_HYPOTHESIS",
            &serde_json::to_string(&hypothesis.id).unwrap(),
        )
        .replace("LINKED_GROUP", &serde_json::to_string(&group.id).unwrap())
        .replace(
            "LINKED_MEMBERS",
            &serde_json::to_string(&group.members).unwrap(),
        );
    let record = basecraft_ops::files::companion_path(None, &workspace);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    basecraft_ops::files::delimitation_companion(
        &work,
        &basecraft_io::hash_file(&workspace).unwrap(),
        &workspace,
    )
    .unwrap()
    .write(&mut std::fs::File::create(record).unwrap())
    .unwrap();
    open_hooks::hold(
        folder
            .path()
            .join("pending.afa")
            .to_string_lossy()
            .into_owned(),
        String::new(),
    );
    let root = Arc::new(AtomicUsize::new(0));
    let created = root.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_creation_failed(|_, _| std::process::exit(4))
        .release();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| {
            created.store(object as usize, Ordering::Relaxed);
        })
        .release();
    engine
        .pin_mut()
        .load_data(&QByteArray::from(&source), &QUrl::default());
    let mut baseline = BTreeMap::new();
    for checkpoint in 0..9 {
        let status = app.as_mut().exec();
        // SAFETY: the engine owns Main; this synchronous read emits no signals.
        let diagnostic = unsafe {
            (root.load(Ordering::Relaxed) as *mut cxx_qt::QObject)
                .as_ref()
                .unwrap()
        }
        .object_name()
        .to_string();
        assert_eq!(
            status, 0,
            "linked taxa checkpoint {checkpoint}: {diagnostic}"
        );
        assert_eq!(
            diagnostic,
            format!("checkpoint:{}", checkpoint + 1),
            "journey skipped a checkpoint"
        );
        if checkpoint == 0 {
            baseline = files(folder.path());
        } else {
            // Closing the tree's tab ends its session, which deletes that
            // session's recovery log. Nothing else may change.
            let recovery = folder.path().join(".basecraft/recovery");
            let now = files(folder.path());
            let changed: Vec<_> = baseline
                .keys()
                .chain(now.keys())
                .filter(|path| baseline.get(*path) != now.get(*path))
                .filter(|path| now.contains_key(*path) || !path.starts_with(&recovery))
                .collect();
            assert!(
                changed.is_empty(),
                "routing wrote document/sidecar/journal files at {checkpoint}: {changed:#?}"
            );
        }
        // SAFETY: Main and its fixture-owned references remain live throughout
        // this synchronous inspection; no event processing or mutation occurs.
        let main = unsafe {
            (root.load(Ordering::Relaxed) as *mut cxx_qt::QObject)
                .as_ref()
                .unwrap()
        };
        let mut checked = 0;
        for property in [
            c"a",
            c"duplicate",
            c"tree",
            c"workspace",
            c"unrelated",
            c"unlinked",
            c"displayTree",
        ] {
            let value = application_test_bridge::property(main, property);
            let pointer = value
                .value::<cxx_qt_lib::QObjectMutPtr>()
                .expect("typed receiver property")
                .as_mut_ptr();
            if pointer.is_null() {
                assert!(
                    property == c"tree" && matches!(checkpoint, 6 | 7),
                    "only the closed receiver may be null"
                );
                continue;
            }
            // SAFETY: the fixture clears its reference before removing a tab.
            // Reacquire each current receiver after every event-loop checkpoint.
            let object = unsafe { pointer.as_ref().unwrap() };
            let name = object.object_name().to_string();
            if let Some(expected) = name.strip_prefix("linked-check:") {
                let expected: Vec<String> = serde_json::from_str(expected).unwrap();
                let actual = if let Some(ctl) =
                    object.downcast::<alignment_bridge::qobject::AlignmentController>()
                {
                    ctl.linked_taxa_snapshot()
                } else if let Some(ctl) = object.downcast::<tree_bridge::qobject::TreeController>()
                {
                    ctl.linked_taxa_snapshot()
                } else if let Some(ctl) =
                    object.downcast::<delimitation_bridge::qobject::DelimitationController>()
                {
                    serde_json::from_str::<Vec<String>>(&ctl.selection_json().to_string()).unwrap()
                } else {
                    panic!("unexpected marked controller");
                };
                assert_eq!(
                    actual, expected,
                    "receiver state at checkpoint {checkpoint}: {name}"
                );
                checked += 1;
            }
        }
        assert_eq!(
            checked,
            if matches!(checkpoint, 6 | 7) { 6 } else { 7 },
            "all live receivers checked: {diagnostic}"
        );
    }
    open_hooks::take_admissions();
    println!(
        "Real Main linked taxa: directions, lifecycle, clear, identity, focus and unchanged files passed"
    );
}

/// The real application loads Main.qml offscreen, draws and quits. A QML load
/// error or a startup hang fails here instead of in someone's hands.
fn application_starts_offscreen_and_draws_its_window() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let shot = root.path().join("start.png");
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft-gui"))
        .arg("--screenshot")
        .arg(&shot)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QPA_PLATFORMTHEME", "")
        .env("QT_QUICK_BACKEND", "software")
        .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
        .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
        .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
        .env("HOME", root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(120) {
            child.kill().unwrap();
            panic!("the application did not quit within 120 s");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(status.success(), "{stderr}");
    assert!(!stderr.contains("failed to load component"), "{stderr}");
    assert!(shot.is_file(), "no window was drawn: {stderr}");
}

/// Quitting while the release check waits on a server that never answers:
/// the window's teardown drops the check, which must end the request well
/// before its 10 s timeout and leave the application to exit cleanly.
fn release_check_cancels_at_quit() -> ! {
    #[derive(Clone)]
    struct Gone;
    impl jobs::Owner<()> for Gone {
        fn deliver(&self, _: impl FnOnce(std::pin::Pin<&mut ()>) + Send + 'static) -> bool {
            false
        }
    }
    let app = application::new(&["basecraft-gui".into()], true);
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest", server.local_addr().unwrap());
    let (connected, accepted) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let held = server.accept().unwrap();
        connected.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        drop(held);
    });
    let check = release_check::ReleaseCheck::start(Gone, url, |_, _| {
        panic!("a cancelled check delivered a reply")
    })
    .expect("release check started");
    accepted
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the request reached the server");
    let quit = std::time::Instant::now();
    drop(check);
    let waited = quit.elapsed();
    assert!(
        waited < std::time::Duration::from_secs(2),
        "quitting waited {waited:?}"
    );
    drop(app);
    println!("RELEASE_QUIT {}", waited.as_millis());
    std::process::exit(0);
}

fn main() {
    #[cfg(feature = "test-counters")]
    open_hooks::install();
    if std::env::args().nth(1).as_deref() == Some("--app-starts") {
        application_starts_offscreen_and_draws_its_window();
        println!("The application started offscreen and drew its window");
        return;
    }
    // The test must never open desktop windows, including when run manually.
    if std::env::var("QT_QPA_PLATFORM").as_deref() != Ok("offscreen")
        || std::env::var("QT_QPA_PLATFORMTHEME").as_deref() != Ok("")
    {
        eprintln!(
            "application_bootstrap requires QT_QPA_PLATFORM=offscreen and QT_QPA_PLATFORMTHEME="
        );
        std::process::exit(1);
    }
    if std::env::args().nth(1).as_deref() == Some("--release-quit") {
        release_check_cancels_at_quit();
    }
    if std::env::args().nth(1).as_deref() == Some("--ribbon-walk") {
        ribbon_walk(&std::env::args().nth(2).expect("document folder"));
    }
    if std::env::args().nth(1).as_deref() == Some("--startup-window") {
        let trace = startup_trace::Trace::from_env(std::time::Instant::now());
        let mut app = application::new(&["startup-test".into()], true);
        let mut engine = QQmlApplicationEngine::new();
        let captured = trace.clone();
        engine
            .pin_mut()
            .on_object_created(move |_, object, _| {
                captured.record(startup_trace::Event::QmlRootCreated);
                // SAFETY: the engine owns the live object during this callback.
                unsafe { startup_trace::frame_bridge::attach(object, captured.clone()) };
            })
            .release();
        engine.pin_mut().load_data(&QByteArray::from(
            "import QtQuick; Window { visible: true; width: 100; height: 100; Timer { interval: 250; running: true; onTriggered: Qt.quit() } }"
        ), &QUrl::default());
        trace.record(startup_trace::Event::QmlLoadReturned);
        assert_eq!(app.as_mut().unwrap().exec(), 0);
        return;
    }
    let startup = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--startup-window")
        .env("BASECRAFT_STARTUP_TRACE_TOKEN", "test")
        .env("QT_QUICK_BACKEND", "software")
        .output()
        .unwrap();
    assert!(
        startup.status.success(),
        "{}",
        String::from_utf8_lossy(&startup.stderr)
    );
    let events: Vec<serde_json::Value> = String::from_utf8_lossy(&startup.stderr)
        .lines()
        .filter_map(|line| line.strip_prefix(startup_trace::PREFIX))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for event in [
        "main_entered",
        "qml_root_created",
        "qml_load_returned",
        "first_frame",
    ] {
        assert_eq!(
            events.iter().filter(|row| row["event"] == event).count(),
            1,
            "missing/duplicate {event}: {events:?}"
        );
    }
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0]["elapsed_ns"].as_u64() <= pair[1]["elapsed_ns"].as_u64())
    );
    #[cfg(feature = "test-counters")]
    if std::env::args().nth(1).as_deref() == Some("--open-argv") {
        open_argv::run(Path::new(&std::env::args().nth(2).unwrap()));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--identity-case") {
        check_identity_case(&std::env::args().nth(2).expect("identity case"));
        return;
    }
    for case in [
        "normal",
        "sequence",
        "alignment",
        "tree",
        "unknown",
        "validation",
        "validation-editor",
    ] {
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--identity-case", case])
                .status()
                .unwrap()
                .success(),
            "isolated identity case {case} failed"
        );
    }
    let quit = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--release-quit")
        .output()
        .unwrap();
    let quit_log = String::from_utf8_lossy(&quit.stdout);
    assert!(
        quit.status.success() && quit_log.contains("RELEASE_QUIT"),
        "{quit_log}{}",
        String::from_utf8_lossy(&quit.stderr)
    );
    println!("Quitting during a slow release check ended the request and exited cleanly");
    ribbon_journey();
    #[cfg(feature = "test-counters")]
    open_argv::spawn();
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

    #[cfg(feature = "test-counters")]
    if std::env::args().nth(1).as_deref() == Some("--linked-taxa") {
        linked_taxa_journey(app.as_mut());
        return;
    }

    #[cfg(feature = "test-counters")]
    {
        let folder = tempfile::tempdir().unwrap();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
        std::fs::copy(
            fixtures.join("alignment-editing/ambiguous-gappy.afa"),
            folder.path().join("alignment.afa"),
        )
        .unwrap();
        std::fs::copy(
            fixtures.join("sequences-annotations/locus.gb"),
            folder.path().join("sequence.gb"),
        )
        .unwrap();
        std::fs::copy(
            fixtures.join("tree-inference/unrooted.nwk"),
            folder.path().join("tree.nwk"),
        )
        .unwrap();
        let source = include_str!("fixtures/open-dispatch.qml").replace(
            "OPEN_TEST_FOLDER",
            &serde_json::to_string(folder.path()).unwrap(),
        );
        let mut engine = QQmlApplicationEngine::new();
        engine
            .pin_mut()
            .on_object_creation_failed(|_, _| std::process::exit(4))
            .release();
        basecraft_io::counters::reset();
        engine
            .pin_mut()
            .load_data(&QByteArray::from(&source), &QUrl::default());
        assert_eq!(app.as_mut().exec(), 0, "real shell open-dispatch harness");
        let gui = basecraft_io::counters::snapshot();
        let admissions = open_hooks::take_admissions();
        assert_eq!(
            admissions.len(),
            3,
            "pending/ready duplicates must not acquire again"
        );
        for (path, worker) in admissions {
            let expected = match path.file_name().unwrap().to_str().unwrap() {
                "alignment.afa" => 1600,
                "sequence.gb" => 940,
                "tree.nwk" => 21,
                other => panic!("unexpected admission {other}"),
            };
            assert_eq!(std::fs::metadata(&path).unwrap().len(), expected);
            assert_eq!(
                worker.reads[&path].full_reads,
                1,
                "{} worker reads",
                path.display()
            );
            assert_eq!(worker.reads[&path].bytes, expected);
            assert!(
                !gui.reads.contains_key(&path),
                "GUI reopened {}",
                path.display()
            );
            assert_eq!(
                (worker.sync_data, worker.sync_all, worker.unresolved_reads),
                (0, 0, 0)
            );
            println!(
                "GUI route {}: the worker read it once ({expected} bytes), the window not at all",
                path.file_name().unwrap().to_string_lossy()
            );
        }
        assert_eq!(gui.unresolved_reads, 0);
        assert_eq!(
            (gui.sync_data, gui.sync_all),
            (0, 0),
            "passive shell open wrote durable state"
        );
        println!(
            "GUI callback aggregate sync_data={} sync_all={} (includes shell preference/project prelude)",
            gui.sync_data, gui.sync_all
        );
    }

    #[cfg(feature = "test-counters")]
    let mut consent_folders = Vec::new();
    #[cfg(feature = "test-counters")]
    for scenario in [
        "close-discard",
        "close-save",
        "saveas",
        "evidence",
        "quit-save",
        "quit-discard",
        "quit-batch",
        "stale-close-discard",
        "stale-close-save",
        "stale-saveas",
        "stale-evidence",
        "stale-quit-save",
    ] {
        let folder = tempfile::tempdir().unwrap();
        let original =
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb");
        for name in ["B.gb", "C.gb", "D.gb"] {
            std::fs::write(folder.path().join(name), original).unwrap();
        }
        if scenario.contains("evidence") || scenario == "quit-batch" {
            let chosen = folder.path().join(if scenario == "quit-batch" {
                "B.gb"
            } else {
                "chosen.gb"
            });
            std::fs::write(&chosen, original).unwrap();
            basecraft_ops::records::reference_input_with_dependencies(
                &folder.path().join("result.txt"),
                &chosen,
                &basecraft_io::hash_file(&chosen).unwrap(),
                &[],
                "consent regression",
                None,
            )
            .unwrap();
            assert!(
                !basecraft_ops::records::conflicts(&chosen)
                    .unwrap()
                    .analyses
                    .is_empty()
            );
        }
        // The last ready sibling acknowledges on the GUI callback that opens consent.
        // Releasing A here cannot deliver its queued failure until that callback returns.
        open_hooks::hold(
            folder
                .path()
                .join("missing.gb")
                .to_string_lossy()
                .into_owned(),
            folder
                .path()
                .join(if scenario == "quit-batch" {
                    "D.gb"
                } else {
                    "C.gb"
                })
                .to_string_lossy()
                .into_owned(),
        );
        let source = include_str!("fixtures/open-consent.qml")
            .replace(
                "OPEN_TEST_FOLDER",
                &serde_json::to_string(folder.path()).unwrap(),
            )
            .replace(
                "OPEN_TEST_SCENARIO",
                &serde_json::to_string(scenario).unwrap(),
            );
        let mut engine = QQmlApplicationEngine::new();
        engine
            .pin_mut()
            .on_object_creation_failed(|_, _| std::process::exit(4))
            .release();
        engine
            .pin_mut()
            .load_data(&QByteArray::from(&source), &QUrl::default());
        assert_eq!(app.as_mut().exec(), 0, "real consent {scenario}");
        assert_eq!(
            open_hooks::take_admissions().len(),
            if scenario == "quit-batch" { 3 } else { 2 }
        );
        assert_eq!(
            std::fs::read(folder.path().join(if scenario == "quit-batch" {
                "D.gb"
            } else {
                "C.gb"
            }))
            .unwrap(),
            original
        );
        if scenario.starts_with("stale-")
            || scenario == "close-discard"
            || scenario == "quit-discard"
        {
            assert_eq!(std::fs::read(folder.path().join("B.gb")).unwrap(), original);
        }
        if scenario == "stale-saveas" {
            assert!(!folder.path().join("chosen.gb").exists());
        }
        if scenario == "stale-evidence" {
            let chosen = folder.path().join("chosen.gb");
            assert_eq!(std::fs::read(&chosen).unwrap(), original);
            assert!(
                !basecraft_ops::records::conflicts(&chosen)
                    .unwrap()
                    .analyses
                    .is_empty(),
                "stale consent mutated evidence"
            );
        }
        consent_folders.push(folder);
    }

    #[cfg(feature = "test-counters")]
    for scenario in [
        "loader-error",
        "close-before-ack",
        "double-queue-next",
        "double-queue-close",
    ] {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("sequence.gb"),
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
        )
        .unwrap();
        std::fs::write(
            folder.path().join("tree.nwk"),
            include_bytes!("../../../testdata/fixtures/compact/tree-inference/unrooted.nwk"),
        )
        .unwrap();
        if scenario.starts_with("double-queue") {
            let path = folder
                .path()
                .join("sequence.gb")
                .to_string_lossy()
                .into_owned();
            open_hooks::fail(path.clone(), open_hooks::Fault::DoubleQueue);
            open_hooks::hold(path, String::new());
        }
        let source = include_str!("fixtures/open-boundaries.qml")
            .replace(
                "OPEN_TEST_FOLDER",
                &serde_json::to_string(folder.path()).unwrap(),
            )
            .replace(
                "OPEN_TEST_SCENARIO",
                &serde_json::to_string(scenario).unwrap(),
            );
        let mut engine = QQmlApplicationEngine::new();
        engine
            .pin_mut()
            .on_object_creation_failed(|_, _| std::process::exit(4))
            .release();
        engine
            .pin_mut()
            .load_data(&QByteArray::from(&source), &QUrl::default());
        if scenario.starts_with("double-queue") {
            open_hooks::release_and_wait();
        }
        assert_eq!(app.as_mut().exec(), 0, "real lifecycle boundary {scenario}");
        assert_eq!(
            open_hooks::take_admissions().len(),
            if scenario.starts_with("double-queue") {
                1
            } else {
                2
            }
        );
    }

    // Saved legacy reviews register their evidence in the shared test profile.
    // Retain those fixture stores until every bootstrap save assertion finishes.
    #[cfg(feature = "test-counters")]
    let mut lifecycle_folders = Vec::new();
    #[cfg(feature = "test-counters")]
    for scenario in [
        "reorder",
        "cancel-reopen",
        "save-as",
        "fail-spawn",
        "fail-worker",
        "fail-queue",
        "fail-construction",
        "legacy",
    ] {
        let folder = tempfile::tempdir().unwrap();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
        std::fs::copy(
            fixtures.join("sequences-annotations/locus.gb"),
            folder.path().join("sequence.gb"),
        )
        .unwrap();
        std::fs::copy(
            fixtures.join("tree-inference/unrooted.nwk"),
            folder.path().join("tree.nwk"),
        )
        .unwrap();
        if scenario == "legacy" {
            std::fs::copy(
                fixtures.join("species-delimitation/hypotheses.spart"),
                folder.path().join("hypotheses.spart"),
            )
            .unwrap();
            let trace = folder.path().join("trace.ab1");
            std::fs::copy(fixtures.join("sanger-review/forward.ab1"), &trace).unwrap();
            let review = basecraft_ops::sanger_review::instrument_review(&trace).unwrap();
            let mut session = basecraft_ops::Session::new(folder.path());
            let id = session.create_sanger_review(&review).unwrap();
            session
                .save(id, Some(&folder.path().join("trace.submission.json")))
                .unwrap();
            std::fs::write(folder.path().join("unknown.data"), "unsupported").unwrap();
        }
        if let Some(fault) = match scenario {
            "fail-spawn" => Some(open_hooks::Fault::Spawn),
            "fail-worker" => Some(open_hooks::Fault::Worker),
            "fail-queue" => Some(open_hooks::Fault::Queue),
            _ => None,
        } {
            open_hooks::fail(
                folder
                    .path()
                    .join("sequence.gb")
                    .to_string_lossy()
                    .into_owned(),
                fault,
            );
        }
        if matches!(scenario, "reorder" | "cancel-reopen" | "fail-construction") {
            // A failed construction ends the request, which releases its
            // worker; the tree being ready must not release it first, or its
            // document can be installed before the failure is injected.
            let release = if scenario == "fail-construction" {
                String::new()
            } else {
                folder.path().join("tree.nwk").to_string_lossy().into_owned()
            };
            open_hooks::hold(
                folder
                    .path()
                    .join("sequence.gb")
                    .to_string_lossy()
                    .into_owned(),
                release,
            );
        }
        let source = include_str!("fixtures/open-lifecycle.qml")
            .replace(
                "OPEN_TEST_FOLDER",
                &serde_json::to_string(folder.path()).unwrap(),
            )
            .replace(
                "OPEN_TEST_SCENARIO",
                &serde_json::to_string(scenario).unwrap(),
            );
        let mut engine = QQmlApplicationEngine::new();
        engine
            .pin_mut()
            .on_object_creation_failed(|_, _| std::process::exit(4))
            .release();
        engine
            .pin_mut()
            .load_data(&QByteArray::from(&source), &QUrl::default());
        assert_eq!(app.as_mut().exec(), 0, "real shell lifecycle {scenario}");
        let admissions = open_hooks::take_admissions();
        assert_eq!(
            admissions.len(),
            if scenario == "legacy" {
                0
            } else if matches!(
                scenario,
                "cancel-reopen" | "fail-spawn" | "fail-worker" | "fail-queue" | "fail-construction"
            ) {
                1
            } else {
                2
            },
            "{scenario} acquisition count"
        );
        lifecycle_folders.push(folder);
    }

    #[cfg(feature = "test-counters")]
    {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("shutdown.gb");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
            &path,
        )
        .unwrap();
        open_hooks::hold(path.to_string_lossy().into_owned(), String::new());
        let mut engine = QQmlApplicationEngine::new();
        engine
            .pin_mut()
            .on_object_creation_failed(|_, _| std::process::exit(4))
            .release();
        engine.pin_mut().load_data(
            &QByteArray::from(&format!(
                "import QtQuick; import Basecraft; Main {{ visible: false; Component.onCompleted: openPath({}) }}",
                serde_json::to_string(&path).unwrap()
            )),
            &QUrl::default(),
        );
        drop(engine);
        open_hooks::release_and_wait();
        assert!(
            open_hooks::take_admissions().is_empty(),
            "destroyed shell installed a late candidate"
        );
        println!(
            "Destroyed real shell rejected late worker delivery without joining on the GUI path"
        );
    }

    #[cfg(feature = "test-counters")]
    linked_taxa_journey(app.as_mut());

    controller_appearance::run(&app);
    controller_appearance::closed_editor(&app);

    // Real QML-created controllers; inject invalid owned IDs without exposing
    // production invokables or constructing placeholder document payloads.
    {
        use cxx_qt::casting::Downcast;
        let mut engine = QQmlApplicationEngine::new();
        let checked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = checked.clone();
        engine
            .pin_mut()
            .on_object_created(move |_, object, _| {
                // SAFETY: Qt owns the freshly created object, alive for this
                // synchronous callback on the application thread. No alias is used
                // while its pinned controller reference is held.
                let mut object =
                    unsafe { std::pin::Pin::new_unchecked(object.as_mut().expect("controller")) };
                if let Some(controller) = object
                    .as_mut()
                    .downcast_pin::<tree_bridge::qobject::TreeController>()
                {
                    tree_bridge::refusal_tests::run(controller);
                } else if let Some(controller) = object
                    .as_mut()
                    .downcast_pin::<alignment_bridge::qobject::AlignmentController>(
                ) {
                    alignment_bridge::refusal_tests::run(controller);
                } else {
                    panic!("unexpected controller type");
                }
                count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .release();
        for controller in ["TreeController", "AlignmentController"] {
            engine.pin_mut().load_data(
                &QByteArray::from(format!("import Basecraft; {controller} {{}}").as_str()),
                &QUrl::default(),
            );
        }
        assert_eq!(
            checked.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "all real controller regressions ran"
        );
    }

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
        property bool sangerDone: false
        property int pairFailures: 0
        property string pairFile: ""
        property string editedRevision: ""
        property string manualDecision: ""
        property string manualMasks: ""
        function check(value, message) {{ if (!value) {{ console.error(message); Qt.exit(2) }} }}
        Connections {{
            target: sanger
            function onStageFinished(stage, success, payload) {{
                check(stage === "prepare" && !success && payload === "", "Missing pair must finish with prepare failure");
                check(!sanger.busy && !sanger.calling, "Failed pair must clear activity");
                check(sanger.file_path === pairFile && sanger.review_mode, "Failed pair changed document identity");
                check(sanger.record_revision() === editedRevision, "Failed pair changed review revision");
                check(sanger.review_decision === manualDecision, "Failed pair changed manual decision");
                check(sanger.mask_settings === manualMasks && sanger.dirty, "Failed pair changed masks or dirty state");
                check(sanger.message_kind === "fail" && sanger.message.length > 0, "Pair error must be explicit");
                const pairError = sanger.message;
                check(sanger.message === pairError && !sanger.calling, "A late result replaced pair failure");
                pairFailures++;
                if (pairFailures === 1) {{
                    check(sanger.prepare_reads(JSON.stringify([pairFile, pairFile + ".missing"]), 20), "Retry must be accepted asynchronously");
                    check(sanger.busy, "Accepted retry must show activity");
                    return;
                }}
                check(pairFailures === 2, "Unexpected duplicate pair completion");
                sanger.review_undo();
                check(sanger.mask_settings !== manualMasks, "Failed pair lost mask undo");
                sanger.review_redo();
                check(sanger.mask_settings === manualMasks, "Failed pair lost mask redo");
                check(sanger.save_review({}), "Review save failed: " + sanger.message);
                check(reopened.open_file({}), "Normal open of saved review failed: " + reopened.message);
                check(reopened.review_decision.indexOf("G") >= 0, "Manual decision lost on reopen");
                sangerDone = true;
            }}
        }}
        Component.onCompleted: {{
            check(sanger.open_file({}), "AB1 open failed");
            check(sanger.review_mode, "AB1 must enter review immediately");
            check(sanger.calling, "Alternative calling must be queued");
            sanger.cancel_calls();
            check(!sanger.calling, "Cancel must detach queued work");
            check(sanger.review_set("edit", "G"), "Instrument review edit failed");
            check(sanger.mask_ends(2, sanger.consensus_columns - 1), "Mask failed");
            const decision = sanger.review_decision;
            const masks = sanger.mask_settings;
            const file = sanger.file_path;
            check(sanger.review_details.indexOf("ABI score") >= 0, "Instrument evidence must name its recorded caller");
            const navigationRevision = sanger.record_revision();
            const originalColumn = sanger.selected_column;
            check(sanger.go_to_column(false, "2"), "Draft navigation failed");
            check(sanger.selected_column === 2, "Draft navigation selected wrong column");
            check(sanger.review_details.indexOf("Draft consensus column 2") >= 0, "Selected evidence did not refresh");
            check(!sanger.go_to_column(true, "1"), "Missing reference navigation must fail");
            check(!sanger.go_to_column(false, "0"), "Invalid coordinate must fail");
            check(!sanger.go_to_column(false, "999999999"), "Outside coordinate must fail");
            check(sanger.selected_column === 2, "Failed navigation changed selection");
            check(sanger.go_to_column(false, String(originalColumn)), "Restore selection failed");
            check(sanger.record_revision() === navigationRevision, "Navigation changed review revision");
            check(sanger.mask_settings === masks && sanger.dirty, "Navigation changed masks or dirty state");
            check(sanger.review_decision === decision, "Navigation changed review decision");
            // A queued first-read caller must not run after a failed pair.
            check(sanger.open_file(file), "Reopen for queued calling failed");
            check(sanger.calling, "Reopen should queue calls");
            // The legacy route must cancel before parsing additional reads,
            // while retaining the current review and its undo history.
            check(sanger.review_set("edit", "G"), "Legacy route edit failed");
            check(sanger.mask_ends(2, sanger.consensus_columns - 1), "Legacy route mask failed");
            const legacyRevision = sanger.record_revision();
            check(sanger.calling, "Manual review should retain queued calls");
            check(!sanger.draft_consensus("not-json", 20), "Malformed additional reads must fail");
            check(!sanger.calling, "Legacy validation must detach queued calling");
            const legacyError = sanger.message;
            check(legacyError.length > 0 && sanger.message_kind === "fail", "Legacy error must be explicit");
            check(!sanger.calling, "Legacy failure restarted calls");
            check(sanger.message === legacyError && sanger.message_kind === "fail", "Legacy failure message was replaced");
            check(sanger.file_path === file && sanger.review_mode, "Legacy failure changed document identity");
            check(sanger.record_revision() === legacyRevision, "Legacy failure changed review revision");
            check(sanger.review_decision === decision, "Legacy failure changed manual decision");
            check(sanger.mask_settings === masks && sanger.dirty, "Legacy failure changed masks or dirty state");
            sanger.review_undo();
            check(sanger.mask_settings !== masks, "Legacy failure lost mask undo");
            sanger.review_redo();
            check(sanger.mask_settings === masks, "Legacy failure lost mask redo");
            check(sanger.open_file(file), "Reopen after legacy failure failed");
            check(sanger.calling, "Reopen after legacy failure should queue calls");
            check(!sanger.prepare_reads(JSON.stringify([file, file]), 0), "Invalid cutoff must fail");
            check(!sanger.calling, "Pair preparation must detach queued calling");
            const error = sanger.message;
            check(sanger.message === error, "Pair failure message was replaced");
            // Restore a manual edit/mask, then fail preparation repeatedly.
            check(sanger.review_set("edit", "G"), "Second edit failed");
            check(sanger.mask_ends(2, sanger.consensus_columns - 1), "Second mask failed");
            editedRevision = sanger.record_revision();
            pairFile = file;
            manualDecision = decision;
            manualMasks = masks;
            check(!sanger.prepare_reads("not-json", 20), "Malformed pair JSON must reject immediately");
            check(!sanger.prepare_reads(JSON.stringify([file]), 20), "Invalid read count must reject immediately");
            check(!sanger.busy, "Rejected input must not start work");
            const missingPair = JSON.stringify([file, file + ".missing"]);
            check(sanger.prepare_reads(missingPair, 20), "Missing pair must be accepted for asynchronous preparation");
            check(sanger.busy, "Accepted pair must show activity until completion");
        }}
    "#,
        serde_json::to_string(&saved_path).unwrap(),
        serde_json::to_string(&saved_path).unwrap(),
        serde_json::to_string(&trace_path).unwrap()
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
            "time-tree.nwk",
            include_bytes!(
                "../../../testdata/fixtures/compact/species-delimitation/time-binary.nwk"
            )
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
    basecraft_ops::metadata::create(figure_folder.path()).unwrap();
    basecraft_ops::submission::create(
        &mut basecraft_ops::Session::unlogged(figure_folder.path()),
        &figure_folder.path().join("review.submission.json"),
        &figure_folder.path().join("alignment.afa"),
    )
    .unwrap();
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
    let find_qml = include_str!("fixtures/find-overview-controllers.qml").replace(
        "FIND_TEST_FOLDER",
        &serde_json::to_string(figure_folder.path()).unwrap(),
    );
    engine.pin_mut().load_data(
        &QByteArray::from(&format!(
            "import QtQuick; import Basecraft; Item {{ width: 800; height: 600; \
             OperationPickerDialog {{}} \
             OperationParameters {{ schema: ({{type: 'object', properties: {{age: {{type: 'number'}}}}, required: ['age']}}) }} \
             {sanger_qml} {figure_qml} {find_qml} \
             Timer {{ interval: 1; running: sangerDone && speciesDone && findDone; onTriggered: Qt.quit() }} \
             }}",
        )),
        &QUrl::default(),
    );
    // Turns a scene that never quits into a failure. It guards the event loop
    // only; the checks after it may take longer. Saves on the window thread
    // wait for the disk, which on a busy build host has taken over a minute,
    // so the limit is generous. On expiry, print each thread's blocking call.
    let exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watched = exited.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(300));
        if !watched.load(std::sync::atomic::Ordering::Acquire) {
            for task in std::fs::read_dir("/proc/self/task")
                .into_iter()
                .flatten()
                .flatten()
            {
                let read = |name: &str| {
                    std::fs::read_to_string(task.path().join(name)).unwrap_or_default()
                };
                eprintln!(
                    "TASK {} {} | {} | {}",
                    task.file_name().to_string_lossy(),
                    read("comm").trim(),
                    read("wchan").trim(),
                    read("syscall").trim()
                );
            }
            eprintln!("Bootstrap QML timer did not exit the event loop within 300 seconds");
            std::process::exit(1);
        }
    });
    assert_eq!(app.as_mut().exec(), 0);
    exited.store(true, std::sync::atomic::Ordering::Release);
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
