//! Undo, redo and numbering in real QML-created editors. A document is
//! unsaved exactly when its session says so: undoing back to the saved file
//! leaves it clean. An alignment whose rows share a name offers to number
//! them (D29), as one undo step.
use cxx_qt::casting::Downcast;
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QUrl};
use std::sync::{Arc, Mutex};

use crate::alignment_bridge::qobject::AlignmentController;
use crate::tree_bridge::qobject::TreeController;

fn candidate(path: &std::path::Path) -> crate::document_open::Candidate {
    crate::document_open::Candidate::acquire(
        path.to_str().unwrap(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap()
}

pub(crate) fn run() {
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let path = folder.path().join("history.nwk");
    // Ladderizing with small clades first swaps the root's two children.
    std::fs::write(&path, "((C:1,(D:1,E:1):1):1,(A:1,B:2):1);\n").unwrap();

    let created = Arc::new(Mutex::new(Vec::new()));
    let sink = created.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| sink.lock().unwrap().push(object as usize))
        .release();
    for name in ["TreeController", "AlignmentController"] {
        engine.pin_mut().load_data(
            &QByteArray::from(format!("import Basecraft; {name} {{}}").as_str()),
            &QUrl::default(),
        );
    }
    let objects = created.lock().unwrap().clone();
    // SAFETY: the engine owns the objects until it is dropped at the end of
    // this function; each is used alone on the application thread.
    let object = |address: usize| unsafe {
        std::pin::Pin::new_unchecked(&mut *(address as *mut cxx_qt::QObject))
    };
    let mut tree = object(objects[0]).downcast_pin::<TreeController>().unwrap();
    tree.as_mut().install_fresh(candidate(&path)).unwrap();
    assert!(!*tree.dirty(), "a freshly opened tree is clean");

    tree.as_mut().ladderize(true);
    assert!(*tree.dirty(), "an edit leaves the tree unsaved");
    tree.as_mut().undo();
    assert!(
        !*tree.dirty(),
        "undoing back to the saved tree leaves it clean"
    );
    tree.as_mut().redo();
    assert!(
        *tree.dirty(),
        "redoing the edit leaves the tree unsaved again"
    );
    tree.as_mut().close_session();

    let path = folder.path().join("named.afa");
    std::fs::write(&path, ">a\nAC-T\n>a\nACGT\n>b\nACGA\n").unwrap();
    let mut alignment = object(objects[1])
        .downcast_pin::<AlignmentController>()
        .unwrap();
    alignment.as_mut().install_fresh(candidate(&path)).unwrap();
    let offer = alignment.duplicate_names().to_string();
    assert!(offer.contains("rows share a name: a."), "{offer}");
    assert!(alignment.as_mut().number_duplicates());
    assert!(alignment.duplicate_names().is_empty());
    assert!(*alignment.dirty(), "numbering is an edit");
    let rows = format!(
        "{}{}",
        alignment.accessible_page(0),
        alignment.accessible_page(1)
    );
    assert!(
        rows.contains("Row 1: a.1.") && rows.contains("Row 2: a.2."),
        "{rows}"
    );
    alignment.as_mut().undo();
    assert!(
        !*alignment.dirty(),
        "undoing the numbering leaves the alignment clean"
    );
    alignment.as_mut().close_session();
    drop(engine);
    println!(
        "Undo back to the saved state leaves a tree clean; duplicate rows are numbered in one step"
    );
}
