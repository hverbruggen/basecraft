//! Deterministic interleavings at the shared production admission boundaries.
use std::cell::RefCell;

use crate::{DocId, Session, files};

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}
type Hook = Box<dyn FnMut(&str)>;

pub(crate) fn boundary(stage: &str) {
    HOOK.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook(stage);
        }
    });
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        HOOK.with_borrow_mut(|hook| *hook = None);
    }
}

#[test]
fn persistent_changes_refuse_admission_at_both_boundaries() {
    let main = b">a description\nACGT\n>b retained\nTTAA\n";
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1\n";
    for stage in ["acquired", "parsed"] {
        for name in ["records.fasta", "records.afa", "records.aln.fasta"] {
            for mutation in [
                "main-write",
                "main-replace",
                "main-remove",
                "main-restored",
                "gff-write",
                "gff-replace",
                "gff-remove",
                "gff-restored",
                "gff-appear",
                "main-directory",
                "gff-directory",
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join(name);
                let sidecar = files::gff_sidecar(&path);
                std::fs::write(&path, main).unwrap();
                if mutation != "gff-appear" {
                    std::fs::write(&sidecar, gff).unwrap();
                }
                let mut session = Session::new(dir.path());
                let (first, _) = session.open_classified(&path).unwrap();
                let before = session.state(first);
                let target = if mutation.starts_with("main-") {
                    path.clone()
                } else {
                    sidecar.clone()
                };
                let mut replacement = if mutation.starts_with("main-") {
                    main.to_vec()
                } else {
                    gff.to_vec()
                };
                // Valid same-size replacements isolate the content guard from parsing.
                if mutation.starts_with("main-") {
                    replacement[15] = b'T';
                } else {
                    replacement[16] = b'b';
                }
                HOOK.with_borrow_mut(|hook| {
                    *hook = Some(Box::new(move |at| {
                        if at != stage {
                            return;
                        }
                        match mutation {
                            "main-remove" | "gff-remove" => std::fs::remove_file(&target).unwrap(),
                            "main-directory" | "gff-directory" => {
                                std::fs::remove_file(&target).unwrap();
                                std::fs::create_dir(&target).unwrap();
                            }
                            "main-replace" | "gff-replace" => {
                                let temp = target.with_extension("replacement");
                                std::fs::write(&temp, &replacement).unwrap();
                                std::fs::rename(temp, &target).unwrap();
                            }
                            "main-restored" | "gff-restored" => {
                                let modified = target.metadata().unwrap().modified().unwrap();
                                std::fs::write(&target, &replacement).unwrap();
                                std::fs::File::options()
                                    .write(true)
                                    .open(&target)
                                    .unwrap()
                                    .set_modified(modified)
                                    .unwrap();
                            }
                            _ => std::fs::write(&target, &replacement).unwrap(),
                        }
                    }))
                });
                let reset = Reset;
                assert!(
                    session.open_classified(&path).is_err(),
                    "{stage} {name} {mutation}"
                );
                drop(reset);
                assert_eq!(session.state(first), before);
                assert!(!dir.path().join(".basecraft").exists());
                if path.is_dir() {
                    std::fs::remove_dir(&path).unwrap();
                }
                if sidecar.is_dir() {
                    std::fs::remove_dir(&sidecar).unwrap();
                }
                std::fs::write(&path, main).unwrap();
                std::fs::write(&sidecar, gff).unwrap();
                assert_eq!(session.open(&path).unwrap(), DocId(1), "no half admission");
            }
        }
    }
}

#[test]
fn capture_hashes_and_writers_use_the_owned_pair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.fasta");
    let sidecar = files::gff_sidecar(&path);
    let main = b">a description\nACGT\n>b retained\nTTAA\n";
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1;Name=example\n";
    std::fs::write(&path, main).unwrap();
    std::fs::write(&sidecar, gff).unwrap();
    let legacy = files::open(&path).unwrap();
    let target = path.clone();
    HOOK.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move |stage| {
            assert_eq!(stage, "acquired");
            std::fs::remove_file(&target).unwrap();
            std::fs::remove_file(&sidecar).unwrap();
        }))
    });
    let reset = Reset;
    let captured = files::capture_load(&path, files::Format::Fasta)
        .unwrap()
        .unwrap();
    drop(reset);
    assert_eq!(captured.main_hash, basecraft_io::hash_bytes(main));
    assert_eq!(
        captured.companion_hashes,
        vec![Some(basecraft_io::hash_bytes(gff))]
    );
    let files::Loaded::Sequence(opened) = captured.content else {
        panic!("captured presence selects sequence")
    };
    assert_eq!(opened.doc.to_genbank(), legacy.doc.to_genbank());
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    files::write_main(&legacy, &mut expected).unwrap();
    files::write_main(&opened, &mut actual).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual, main);
    expected.clear();
    actual.clear();
    files::write_sidecar(&legacy, &mut expected)
        .unwrap()
        .unwrap();
    files::write_sidecar(&opened, &mut actual).unwrap().unwrap();
    assert_eq!(actual, expected);
}
