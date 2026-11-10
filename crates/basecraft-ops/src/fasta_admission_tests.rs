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
