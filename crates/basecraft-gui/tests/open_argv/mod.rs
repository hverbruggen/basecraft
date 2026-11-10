//! Real argv project prelude and existing project-owner preservation.
use crate::{
    app_bridge, application, application_test_bridge, bridge, metadata_bridge, submission_bridge,
};
use cxx_qt::{QObject, casting::Downcast};
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QString, QUrl};
use std::{
    path::Path,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

type Identity = (String, basecraft_ops::DocId, u64, u64);

fn owners(root: &QObject) -> (*mut QObject, *mut QObject) {
    let mut metadata = std::ptr::null_mut();
    let mut submission = std::ptr::null_mut();
    let mut pending: Vec<_> = application_test_bridge::children(root)
        .iter()
        .map(cxx_qt_lib::QObjectMutPtr::as_mut_ptr)
        .collect();
    while let Some(pointer) = pending.pop() {
        // SAFETY: synchronous traversal on Qt's main thread while the engine
        // owns the entire object tree; no deletion or event processing occurs.
        let object = unsafe { pointer.as_ref().unwrap() };
        if object
            .downcast::<metadata_bridge::qobject::MetadataController>()
            .is_some()
        {
            metadata = pointer;
        }
        if object
            .downcast::<submission_bridge::qobject::SubmissionController>()
            .is_some()
        {
            submission = pointer;
        }
        pending.extend(
            application_test_bridge::children(object)
                .iter()
                .map(cxx_qt_lib::QObjectMutPtr::as_mut_ptr),
        );
    }
    assert!(
        !metadata.is_null() && !submission.is_null(),
        "real Main project owners"
    );
    (metadata, submission)
}

pub fn run(folder: &Path) {
    bridge::qobject::configure_canvas_graphics();
    let mut app = application::new(&std::env::args().collect::<Vec<_>>(), true);
    let mut app = app.as_mut().unwrap();
    app_bridge::barriers::configure(
        folder.join("A/sequence.gb").to_string_lossy().into_owned(),
        folder
            .join("B/alignment.afa")
            .to_string_lossy()
            .into_owned(),
    );
    let root = Arc::new(AtomicUsize::new(0));
    let initial: Arc<Mutex<Option<(Identity, Identity)>>> = Arc::new(Mutex::new(None));
    let object_root = root.clone();
    let snapshot = initial.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_creation_failed(|_, _| std::process::exit(4))
        .release();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| {
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                object_root.store(object as usize, Ordering::Relaxed);
                // SAFETY: this callback runs after construction, before the event loop
                // delivers the asynchronous canvas completions. Owners remain Qt-owned.
                let object = unsafe { object.as_ref().unwrap() };
                let (metadata, submission) = owners(object);
                let mut metadata = unsafe { Pin::new_unchecked(metadata.as_mut().unwrap()) };
                let mut metadata = metadata
                    .as_mut()
                    .downcast_pin::<metadata_bridge::qobject::MetadataController>()
                    .unwrap();
                assert!(
                    metadata
                        .as_mut()
                        .add_specimen(&QString::from("argv-fixture"))
                );
                let mut submission = unsafe { Pin::new_unchecked(submission.as_mut().unwrap()) };
                let mut submission = submission
                    .as_mut()
                    .downcast_pin::<submission_bridge::qobject::SubmissionController>()
                    .unwrap();
                assert!(submission.as_mut().set_publication(
                    &QString::from("Owner retained"),
                    &QString::from("Tester, Example")
                ));
                assert!(*metadata.can_undo() && *submission.can_undo());
                *snapshot.lock().unwrap() = Some((
                    metadata.open_identity().unwrap(),
                    submission.open_identity().unwrap(),
                ));
            }));
            if checked.is_err() {
                std::process::exit(2);
            }
        })
        .release();
    let source = include_str!("../fixtures/open-argv.qml")
        .replace("OPEN_TEST_FOLDER", &serde_json::to_string(folder).unwrap());
    engine
        .pin_mut()
        .load_data(&QByteArray::from(&source), &QUrl::default());
    assert_eq!(app.as_mut().exec(), 0, "real argv dispatcher");
    // SAFETY: engine still owns the captured root and children; event processing
    // has stopped and no references escape this test.
    let object = unsafe {
        (root.load(Ordering::Relaxed) as *mut QObject)
            .as_ref()
            .unwrap()
    };
    let (metadata, submission) = owners(object);
    let metadata = unsafe { metadata.as_ref().unwrap() }
        .downcast::<metadata_bridge::qobject::MetadataController>()
        .unwrap();
    let submission = unsafe { submission.as_ref().unwrap() }
        .downcast::<submission_bridge::qobject::SubmissionController>()
        .unwrap();
    assert_eq!(
        (
            metadata.open_identity().unwrap(),
            submission.open_identity().unwrap()
        ),
        initial.lock().unwrap().take().unwrap()
    );
    assert!(
        *metadata.can_undo() && *submission.can_undo(),
        "canvas completion cleared project-owner history"
    );
    assert_eq!(
        app_bridge::testing::take().len(),
        2,
        "legacy metadata/submission created a canvas candidate"
    );
    println!(
        "Mixed-project argv retained exact metadata/submission Session, DocId, generation, revision and undo availability"
    );
}

pub fn spawn() {
    let folder = tempfile::tempdir().unwrap();
    for root in ["A", "B"] {
        std::fs::create_dir_all(folder.path().join(root).join(".basecraft")).unwrap();
        basecraft_ops::metadata::create(&folder.path().join(root)).unwrap();
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
    let sequence = folder.path().join("A/sequence.gb");
    let alignment = folder.path().join("B/alignment.afa");
    let submission = folder.path().join("B/export.submission.json");
    std::fs::copy(fixtures.join("sequences-annotations/locus.gb"), &sequence).unwrap();
    std::fs::copy(
        fixtures.join("alignment-editing/ambiguous-gappy.afa"),
        &alignment,
    )
    .unwrap();
    basecraft_ops::submission::create(
        &mut basecraft_ops::Session::unlogged(folder.path().join("B")),
        &submission,
        &alignment,
    )
    .unwrap();
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--open-argv")
            .arg(folder.path())
            .arg(sequence)
            .arg(basecraft_ops::metadata::manifest_path(
                &folder.path().join("A")
            ))
            .arg(alignment)
            .arg(submission)
            .status()
            .unwrap()
            .success(),
        "mixed-project argv subprocess"
    );
}
