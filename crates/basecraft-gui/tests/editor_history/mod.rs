//! Undo and redo in a real QML-created tree editor. The tree is unsaved
//! exactly when its session says so: undoing back to the saved file leaves
//! it clean, and redoing the edit makes it unsaved again.
use cxx_qt::casting::Downcast;
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QUrl};
use std::sync::{Arc, Mutex};

use crate::tree_bridge::qobject::TreeController;

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
    engine.pin_mut().load_data(
        &QByteArray::from("import Basecraft; TreeController {}"),
        &QUrl::default(),
    );
    let address = created.lock().unwrap()[0];
    // SAFETY: the engine owns the object until it is dropped at the end of
    // this function; it is used alone on the application thread.
    let object = unsafe { std::pin::Pin::new_unchecked(&mut *(address as *mut cxx_qt::QObject)) };
    let mut tree = object.downcast_pin::<TreeController>().unwrap();
    let candidate = crate::document_open::Candidate::acquire(
        path.to_str().unwrap(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    tree.as_mut().install_fresh(candidate).unwrap();
    assert!(!*tree.dirty(), "a freshly opened tree is clean");

    tree.as_mut().ladderize(true);
    assert!(*tree.dirty(), "an edit leaves the tree unsaved");
    tree.as_mut().undo();
    assert!(!*tree.dirty(), "undoing back to the saved tree leaves it clean");
    tree.as_mut().redo();
    assert!(*tree.dirty(), "redoing the edit leaves the tree unsaved again");
    tree.as_mut().close_session();
    drop(engine);
    println!("Tree undo back to the saved state leaves the tree clean");
}
