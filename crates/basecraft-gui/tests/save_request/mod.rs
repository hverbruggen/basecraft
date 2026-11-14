//! A save request from the window, on a real QML-created editor: a document
//! no analysis used saves at once; one an analysis used returns the check to
//! show and leaves the file alone; Save As over a file an analysis used
//! returns the check too and writes nothing.
use cxx_qt::casting::Downcast;
use cxx_qt_lib::{QByteArray, QQmlApplicationEngine, QString, QUrl};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::tree_bridge::qobject::TreeController;

const TREE: &str = "((C:1,(D:1,E:1):1):1,(A:1,B:2):1);\n";

fn candidate(path: &Path) -> crate::document_open::Candidate {
    crate::document_open::Candidate::acquire(
        path.to_str().unwrap(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap()
}

/// An analysis whose recorded input is `source` as it is now.
fn used_by_analysis(source: &Path) -> basecraft_ops::records::ArtifactRun {
    basecraft_ops::records::reference_input(
        &source.with_extension("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        None,
        &[],
        "save request test",
        None,
    )
    .unwrap()
}

fn answer(reply: &QString) -> serde_json::Value {
    serde_json::from_str(&reply.to_string()).unwrap()
}

pub(crate) fn run() {
    let profile = std::env::var_os("BASECRAFT_PROFILE_DIR").expect("isolated bootstrap profile");
    let created = Arc::new(Mutex::new(Vec::new()));
    let sink = created.clone();
    let mut engine = QQmlApplicationEngine::new();
    engine
        .pin_mut()
        .on_object_created(move |_, object, _| sink.lock().unwrap().push(object as usize))
        .release();
    for _ in 0..3 {
        engine.pin_mut().load_data(
            &QByteArray::from("import Basecraft; TreeController {}"),
            &QUrl::default(),
        );
    }
    let objects = created.lock().unwrap().clone();
    // SAFETY: the engine owns the objects until it is dropped at the end of
    // this function; each is used alone on the application thread.
    let tree = |index: usize| unsafe {
        std::pin::Pin::new_unchecked(&mut *(objects[index] as *mut cxx_qt::QObject))
            .downcast_pin::<TreeController>()
            .unwrap()
    };

    // Nothing used the file: saved at once.
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let path = folder.path().join("clean.nwk");
    std::fs::write(&path, TREE).unwrap();
    let mut clean = tree(0);
    clean.as_mut().install_fresh(candidate(&path)).unwrap();
    clean.as_mut().ladderize(true);
    let reply = answer(&clean.as_mut().request_save(&QString::default()));
    assert_eq!(reply, serde_json::json!({"saved": true}), "{reply}");
    assert_ne!(std::fs::read_to_string(&path).unwrap(), TREE);
    assert!(!*clean.dirty());
    clean.as_mut().close_session();

    // An analysis used the file: the check comes back and the file stays.
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let path = folder.path().join("used.nwk");
    std::fs::write(&path, TREE).unwrap();
    used_by_analysis(&path);
    let mut used = tree(1);
    used.as_mut().install_fresh(candidate(&path)).unwrap();
    used.as_mut().ladderize(true);
    let reply = answer(&used.as_mut().request_save(&QString::default()));
    let analyses = &reply["check"]["evidence"]["conflict"]["analyses"];
    assert!(
        analyses.as_array().is_some_and(|a| !a.is_empty()) && reply["check"]["token"].is_string(),
        "{reply}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TREE);
    assert!(*used.dirty());
    used.as_mut().close_session();

    // Save As over another file an analysis used: the window asks first,
    // and that file stays as the analysis read it.
    let folder = tempfile::tempdir_in(&profile).unwrap();
    let path = folder.path().join("source.nwk");
    std::fs::write(&path, TREE).unwrap();
    let target = folder.path().join("analysed.nwk");
    std::fs::write(&target, "(A:1,B:1);\n").unwrap();
    used_by_analysis(&target);
    let mut copy = tree(2);
    copy.as_mut().install_fresh(candidate(&path)).unwrap();
    copy.as_mut().ladderize(true);
    let reply = answer(
        &copy
            .as_mut()
            .request_save(&QString::from(target.to_str().unwrap())),
    );
    let analyses = &reply["check"]["evidence"]["conflict"]["analyses"];
    assert!(
        analyses.as_array().is_some_and(|a| !a.is_empty()),
        "{reply}"
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "(A:1,B:1);\n");
    copy.as_mut().close_session();

    drop(engine);
    println!(
        "A save request saves a document no analysis used, and returns the check for one an analysis used and for Save As over one"
    );
}
