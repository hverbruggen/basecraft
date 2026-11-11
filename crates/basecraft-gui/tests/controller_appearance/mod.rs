//! Real QML-created editor controllers follow the app-wide appearance: a
//! preference change reaches every open editor through the window thread, and
//! a Find scan finishes without polling. Document state stays as it was.
use cxx_qt::casting::Downcast;
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QString, QUrl};
use cxx_qt_lib_extras::QApplication;
use std::sync::{Arc, Mutex};

use crate::{alignment_bridge, appearance, bridge, sanger_bridge, tree_bridge};

trait Fixture: Sized {
    fn load(self: std::pin::Pin<&mut Self>, path: &std::path::Path);
}
macro_rules! owned_fixture {
    ($controller:ty) => {
        impl Fixture for $controller {
            fn load(self: std::pin::Pin<&mut Self>, path: &std::path::Path) {
                let candidate = crate::document_open::Candidate::acquire(
                    path.to_str().unwrap(),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
                self.install_fresh(candidate).unwrap();
            }
        }
    };
}
owned_fixture!(bridge::qobject::GenomeController);
owned_fixture!(alignment_bridge::qobject::AlignmentController);
owned_fixture!(tree_bridge::qobject::TreeController);
impl Fixture for sanger_bridge::qobject::SangerController {
    fn load(mut self: std::pin::Pin<&mut Self>, path: &std::path::Path) {
        assert!(
            self.as_mut()
                .open_file(&QString::from(path.to_str().unwrap()))
        );
        // Keep the queued alternative calls from editing the review.
        self.cancel_calls();
    }
}

/// Run the window thread's queued calls until `done` holds.
fn settle(app: &QApplication, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "{what} never happened"
        );
        app.process_events();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn run(app: &QApplication) {
    // Only the dedicated temporary bootstrap profile may be written.
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
    assert!(paths.preferences().starts_with(&profile));
    assert!(!appearance::current().color_blind);

    let created = Arc::new(Mutex::new(Vec::new()));
    let sink = created.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| sink.lock().unwrap().push(object as usize))
        .release();
    for name in [
        "GenomeController",
        "AlignmentController",
        "TreeController",
        "SangerController",
    ] {
        engine.pin_mut().load_data(
            &QByteArray::from(format!("import Basecraft; {name} {{}}").as_str()),
            &QUrl::default(),
        );
    }
    let objects = created.lock().unwrap().clone();
    assert_eq!(objects.len(), 4);
    let object = |address: usize| {
        // SAFETY: the engine owns these objects until it is dropped at the end
        // of this function; each is used alone on the application thread.
        unsafe { std::pin::Pin::new_unchecked(&mut *(address as *mut cxx_qt::QObject)) }
    };
    let folder = tempfile::tempdir_in(&profile).unwrap();
    // Each editor: the document state a theme change must not touch.
    macro_rules! each {
        (|$c:ident: $t:ty| $body:expr) => {
            objects
                .iter()
                .filter_map(|&address| {
                    object(address).downcast_pin::<$t>().map(|$c| {
                        #[allow(unused_mut)]
                        let mut $c = $c;
                        $body
                    })
                })
                .collect::<Vec<_>>()
        };
    }
    macro_rules! state {
        ($c:expr) => {
            (
                $c.file_path().to_string(),
                $c.record_revision().to_string(),
                *$c.dirty(),
                $c.accessible_page(0).to_string(),
                $c.overview_matches().to_string(),
            )
        };
    }
    macro_rules! open_and_find {
        ($t:ty, $file:expr, $bytes:expr) => {{
            let path = folder.path().join($file);
            std::fs::write(&path, $bytes).unwrap();
            let _ = each!(|c: $t| {
                c.as_mut().load(&path);
                c.as_mut().resize(800.0, 500.0);
                c.as_mut().find_query(&QString::from("A"), false);
                c.as_mut().find_step(0);
            });
            settle(app, "Find completion", || {
                each!(|c: $t| c.overview_matches().to_string() != "[]")
                    .iter()
                    .all(|found| *found)
            });
            each!(|c: $t| state!(c))
        }};
    }
    let genome = open_and_find!(
        bridge::qobject::GenomeController,
        "sequence.fa",
        ">A\nACGTACGT\n"
    );
    let alignment = open_and_find!(
        alignment_bridge::qobject::AlignmentController,
        "alignment.afa",
        ">A\nACGT\n>B\nACGA\n"
    );
    let tree = open_and_find!(
        tree_bridge::qobject::TreeController,
        "tree.nwk",
        "((A:1,B:2):3,(C:4,D:5):6);\n"
    );
    let trace = open_and_find!(
        sanger_bridge::qobject::SangerController,
        "forward.ab1",
        include_bytes!("../../../../testdata/fixtures/compact/sanger-review/forward.ab1")
    );

    // A follower redraws on change; a placeholder frame shows when it has.
    let placeholder = QByteArray::from("before the preference change");
    for &address in &objects {
        let mut o = object(address);
        if let Some(c) = o
            .as_mut()
            .downcast_pin::<bridge::qobject::GenomeController>()
        {
            c.set_frame(placeholder.clone());
        } else if let Some(c) = o
            .as_mut()
            .downcast_pin::<alignment_bridge::qobject::AlignmentController>()
        {
            c.set_frame(placeholder.clone());
        } else if let Some(c) = o
            .as_mut()
            .downcast_pin::<tree_bridge::qobject::TreeController>()
        {
            c.set_frame(placeholder.clone());
        } else if let Some(c) = o
            .as_mut()
            .downcast_pin::<sanger_bridge::qobject::SangerController>()
        {
            c.set_frame(placeholder.clone());
        }
    }
    basecraft_platform::settings::SettingsStore::new(paths.preferences())
        .update(basecraft_platform::settings::Change::DataColorScheme(
            basecraft_platform::settings::DataColorScheme::ColorBlindFriendly,
        ))
        .unwrap();
    appearance::preferences_changed();
    settle(app, "the colour-blind palette", || {
        appearance::current().color_blind
    });
    let look = appearance::current();
    macro_rules! restyled {
        ($t:ty, $before:expr) => {{
            settle(app, "restyling", || {
                each!(|c: $t| *c.frame() != placeholder)
                    .iter()
                    .all(|same| *same)
            });
            for (mut c, before) in objects
                .iter()
                .filter_map(|&address| object(address).downcast_pin::<$t>())
                .zip($before)
            {
                assert_eq!(c.accent().to_string(), look.accent);
                assert_eq!(c.background().to_string(), look.background);
                assert_eq!(state!(c), before, "appearance changed document state");
                c.as_mut().find_query(&QString::default(), false);
            }
        }};
    }
    restyled!(bridge::qobject::GenomeController, genome);
    restyled!(alignment_bridge::qobject::AlignmentController, alignment);
    restyled!(tree_bridge::qobject::TreeController, tree);
    restyled!(sanger_bridge::qobject::SangerController, trace);
    // Review recovery keeps references into the fixture folder; leave it for
    // the bootstrap profile's own removal.
    let _ = folder.keep();
    drop(engine);
    println!("Four real editors followed a preference change and finished Find without polling");
}

/// A real editor destroyed while its Find scan runs: the scan's result finds
/// no window to go to and is dropped, without a crash.
pub(crate) fn closed_editor(app: &QApplication) {
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let path = folder.path().join("large.afa");
    let row = "ACGT".repeat(5000);
    let text = (0..400)
        .map(|i| format!(">r{i}\n{row}\n"))
        .collect::<Vec<_>>()
        .concat();
    std::fs::write(&path, text).unwrap();
    let created = Arc::new(Mutex::new(Vec::new()));
    let sink = created.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| sink.lock().unwrap().push(object as usize))
        .release();
    engine.pin_mut().load_data(
        &QByteArray::from("import Basecraft; AlignmentController {}"),
        &QUrl::default(),
    );
    let address = created.lock().unwrap()[0];
    {
        // SAFETY: the engine owns the controller until it is dropped below.
        let object =
            unsafe { std::pin::Pin::new_unchecked(&mut *(address as *mut cxx_qt::QObject)) };
        let mut c = object
            .downcast_pin::<alignment_bridge::qobject::AlignmentController>()
            .unwrap();
        c.as_mut().load(&path);
        c.as_mut().find_query(&QString::from("A"), false);
        c.as_mut().find_step(0);
        assert_eq!(
            c.find_status().to_string(),
            "Finding…",
            "the scan is running"
        );
    }
    drop(engine);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < until {
        app.process_events();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!("A destroyed editor dropped its running Find result");
}
