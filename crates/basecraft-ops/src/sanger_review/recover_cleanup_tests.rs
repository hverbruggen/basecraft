use super::*;
use crate::input_evidence::{self as inputs, SavePolicy};
use crate::isolated_profile as isolated;
use std::{collections::BTreeMap, fs};

fn enter(case: &str) -> Option<PathBuf> {
    isolated::enter(&format!(
        "sanger_review::moved_and_failed_recovery_tests::recover_cleanup_tests::{case}"
    ))
}

// Do not follow symlinks. Identity is included so an empty replacement is not
// mistaken for the directory that existed before a failed call.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: (u64, u64),
    contents: Vec<u8>,
    link: Option<PathBuf>,
    directory: bool,
}
fn tree(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let identity = (0, 0);
        result.insert(
            path.strip_prefix(root).unwrap().into(),
            Entry {
                identity,
                contents: if meta.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
                link: meta.is_symlink().then(|| fs::read_link(path).unwrap()),
                directory: meta.is_dir(),
            },
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), result);
            }
        }
    }
    let mut result = BTreeMap::new();
    if fs::symlink_metadata(root).is_ok() {
        visit(root, root, &mut result);
    }
    result
}

// Storage paths/evidence are relocated by the existing saver. All biological,
// raw-draft and replay state, including inactive-set order, must stay identical.
fn biology(s: &SangerReviewSession) -> serde_json::Value {
    serde_json::json!({
        "draft": s.draft, "raw": s.draft_json, "hash": s.draft_sha256,
        "path": s.draft_path, "decisions": s.decisions, "manual": s.manual,
        "mask": s.mask, "settings": s.settings, "reference": s.reference,
        "events": s.events, "undo": format!("{:?}", s.undo),
        "redo": format!("{:?}", s.redo), "extra": s.stored_extra,
        "retained": s.retained.iter().map(biology).collect::<Vec<_>>()
    })
}

struct Fixture {
    source: PathBuf,
    checkpoint: PathBuf,
    original: BTreeMap<PathBuf, Entry>,
    sources: BTreeMap<PathBuf, Entry>,
    expected: serde_json::Value,
}
impl Fixture {
    fn new(root: &Path, third: bool) -> Self {
        let source = root.join("source");
        let mut review = pair(&source);
        let mut mask = review.mask.clone();
        mask.start = 1;
        review.apply(ReviewOp::Mask { settings: mask }).unwrap();
        review.apply(ReviewOp::Undo).unwrap();
        review.apply(ReviewOp::Redo).unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('T'),
            })
            .unwrap();
        review.apply(ReviewOp::Undo).unwrap(); // retain a nonempty redo stack
        let saved = source.join("review.json");
        review.save(&saved).unwrap();
        let run = crate::analysis_store::ArtifactRun::for_result(&saved)
            .unwrap()
            .unwrap();
        fs::write(
            run.join("reports/methods.md"),
            b"Authored laboratory methods.\n",
        )
        .unwrap();
        review.start_recovery().unwrap();
        if third {
            let mut draft = review.draft.clone();
            draft.version_output = "distinct third caller".into();
            let mut alternative =
                SangerReviewSession::from_draft(draft, &source.join("source.ab1")).unwrap();
            assert!(
                !review
                    .call_sets()
                    .iter()
                    .any(|set| set.0 == alternative.draft_sha256)
            );
            assert!(alternative.evidence.sources.is_empty());
            alternative
                .apply(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('A'),
                })
                .unwrap();
            review.retain_call_set(alternative).unwrap();
            assert_eq!(review.call_sets().len(), 3);
        }
        let checkpoint = review.recovery_path().unwrap().to_owned();
        fs::write(
            checkpoint.parent().unwrap().join("unrelated"),
            b"keep checkpoint sibling",
        )
        .unwrap();
        fs::write(source.join("unrelated"), b"keep source sibling").unwrap();
        let loaded = SangerReviewSession::load(&checkpoint).unwrap();
        assert_eq!(loaded.recovery_path(), Some(checkpoint.as_path()));
        let expected = biology(&loaded);
        Self {
            sources: tree(&source),
            original: tree(checkpoint.parent().unwrap()),
            source,
            checkpoint,
            expected,
        }
    }
    fn unchanged(&self) {
        assert_eq!(tree(self.checkpoint.parent().unwrap()), self.original);
        assert_eq!(tree(&self.source), self.sources);
        assert_eq!(
            biology(&SangerReviewSession::load(&self.checkpoint).unwrap()),
            self.expected
        );
    }
    fn output(&self, path: &Path) {
        let mut loaded = SangerReviewSession::load(path).unwrap();
        assert_eq!(biology(&loaded), self.expected);
        let ids: Vec<_> = loaded
            .call_sets()
            .into_iter()
            .map(|set| set.0.to_owned())
            .collect();
        for id in ids {
            loaded.select_call_set(&id).unwrap();
            for input in loaded.working_draft().unwrap().inputs {
                assert!(input.path.is_file());
            }
        }
    }
    fn attempt(
        &self,
        dest: &Path,
        save: impl FnOnce(&mut SangerReviewSession, &Path) -> Result<(), String>,
    ) -> String {
        let mut calls = 0;
        let mut actual = None;
        let mut saved_tree = None;
        let mut job_tree = None;
        let result = recover_with_saver(&self.checkpoint, dest, |review, target| {
            calls += 1;
            assert!(review.recovery_path().is_none());
            assert_eq!(biology(review), self.expected);
            let result = save(review, target);
            actual = result.as_ref().err().cloned();
            saved_tree = Some(tree(dest));
            job_tree = review.recovery_path().map(|path| {
                let parent = path.parent().unwrap();
                (parent.to_owned(), tree(parent))
            });
            result
        });
        assert_eq!(calls, 1);
        let actual = actual.unwrap();
        match result.unwrap_err() {
            crate::Error::Invalid(message) => assert_eq!(message, actual),
            error => panic!("wrong wrapper mapping: {error:?}"),
        }
        assert_eq!(tree(dest), saved_tree.unwrap());
        if let Some((path, before)) = job_tree {
            assert_eq!(tree(&path), before);
        }
        self.unchanged();
        actual
    }
}
fn target(f: &Fixture, dest: &Path) -> PathBuf {
    dest.join(
        SangerReviewSession::load(&f.checkpoint)
            .unwrap()
            .storage_anchor()
            .file_name()
            .unwrap(),
    )
}
fn new_job(review: &SangerReviewSession, f: &Fixture) -> PathBuf {
    let path = review.recovery_path().unwrap().to_owned();
    assert_ne!(path, f.checkpoint);
    assert_eq!(
        path.parent().unwrap().parent(),
        f.checkpoint.parent().unwrap().parent()
    );
    assert!(path.is_file());
    assert!(path.parent().unwrap().join("manifest.json").is_file());
    path
}

#[test]
fn r0_success_preserves_original_and_all_sets() {
    let Some(root) = enter("r0_success_preserves_original_and_all_sets") else {
        return;
    };
    let f = Fixture::new(&root, false);
    let jobs = f.checkpoint.parent().unwrap().parent().unwrap();
    let before: Vec<_> = fs::read_dir(jobs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let dest = root.join("recovered");
    let path = recover(&f.checkpoint, &dest).unwrap();
    assert_eq!(path, target(&f, &dest));
    f.output(&path);
    f.unchanged();
    let run = crate::analysis_store::ArtifactRun::for_result(&path)
        .unwrap()
        .unwrap();
    assert!(run.join("manifest.json").is_file());
    assert_eq!(
        fs::read(run.join("reports/methods.md")).unwrap(),
        b"Authored laboratory methods.\n"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        fs::read(run.join("visible-review.json")).unwrap()
    );
    let added: Vec<_> = fs::read_dir(jobs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !before.contains(p))
        .collect();
    assert_eq!(added.len(), 1);
    f.output(&added[0].join("review.sanger-review.json"));
}

#[test]
fn r1_refusals_never_enter_saver() {
    let Some(root) = enter("r1_refusals_never_enter_saver") else {
        return;
    };
    let f = Fixture::new(&root, false);
    let malformed = root.join("bad.json");
    fs::write(&malformed, b"invalid").unwrap();
    let ordinary = root.join("ordinary.json");
    fs::copy(&f.checkpoint, &ordinary).unwrap();
    let occupied = root.join("occupied");
    fs::create_dir(&occupied).unwrap();
    fs::write(occupied.join("sentinel"), b"keep").unwrap();
    let file = root.join("file");
    fs::write(&file, b"keep file").unwrap();
    let mut cases = vec![
        (malformed, root.join("bad-dest"), "load"),
        (ordinary, root.join("ordinary-dest"), "ordinary"),
        (f.checkpoint.clone(), occupied, "exists"),
        (f.checkpoint.clone(), file, "exists"),
        (f.checkpoint.clone(), root.join("absent/child"), "io"),
    ];
    #[cfg(unix)]
    for (name, referent) in [
        ("link", f.source.clone()),
        ("dangling", root.join("no-referent")),
    ] {
        let link = root.join(name);
        std::os::unix::fs::symlink(referent, &link).unwrap();
        cases.push((f.checkpoint.clone(), link, "exists"));
    }
    for (checkpoint, dest, kind) in cases {
        let before = tree(&root);
        let mut calls = 0;
        let error = recover_with_saver(&checkpoint, &dest, |_, _| {
            calls += 1;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(calls, 0);
        match kind {
            "exists" => assert!(
                matches!(error, crate::Error::Invalid(ref s) if s == "recovery destination already exists")
            ),
            "ordinary" => assert!(
                matches!(error, crate::Error::Invalid(ref s) if s == "not a Sanger review recovery checkpoint")
            ),
            "io" => assert!(
                matches!(error, crate::Error::Io(ref e) if e.kind() == std::io::ErrorKind::NotFound)
            ),
            _ => assert!(matches!(error, crate::Error::Invalid(_))),
        }
        assert_eq!(tree(&root), before);
        f.unchanged();
    }
}

#[test]
fn r2_real_lock_failure_retains_empty_destination() {
    let Some(root) = enter("r2_real_lock_failure_retains_empty_destination") else {
        return;
    };
    let f = Fixture::new(&root, false);
    let jobs = tree(f.checkpoint.parent().unwrap().parent().unwrap());
    let dest = root.join("early");
    f.attempt(&dest, |review, target| {
        let _guard = crate::save_guard::lock(target).unwrap();
        let result = review.save_with_evidence(target, SavePolicy::Cancel);
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("another Basecraft process is saving")
        );
        assert!(review.recovery_path().is_none());
        result
    });
    assert!(dest.is_dir());
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    assert_eq!(tree(f.checkpoint.parent().unwrap().parent().unwrap()), jobs);
    let digest = basecraft_io::hash_bytes(target(&f, &dest).as_os_str().as_encoded_bytes());
    let locks = root.join(format!(
        "basecraft-save-locks-{}",
        basecraft_io::hash_bytes(basecraft_platform::username().as_bytes())
            .trim_start_matches("b3:")
    ));
    assert!(
        locks
            .join(format!("{}.lock", digest.trim_start_matches("b3:")))
            .is_file()
    );
}

#[test]
fn r3_real_partial_registration_precedes_incomplete_refusal() {
    let Some(root) = enter("r3_real_partial_registration_precedes_incomplete_refusal") else {
        return;
    };
    let f = Fixture::new(&root, true);
    let gone = root.join("gone");
    fs::create_dir(&gone).unwrap();
    let missing = gone.join("source.fa");
    fs::write(&missing, b">gone\nACGT\n").unwrap();
    let missing_run = inputs::reference_input_with_dependencies(
        &gone.join("result.fa"),
        &missing,
        &basecraft_io::hash_file(&missing).unwrap(),
        &[],
        "missing store",
    )
    .unwrap();
    let missing_store = missing_run
        .directory()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    fs::remove_dir_all(&gone).unwrap(); // only this test-owned intentional missing store
    let dest = root.join("partial");
    let mut job = None;
    let error = f.attempt(&dest, |review, target| {
        let result = review.save_with_evidence(target, SavePolicy::Cancel);
        job = Some(new_job(review, &f));
        result
    });
    assert_eq!(
        error,
        "evidence_check_incomplete: could not check all analysis evidence"
    );
    assert!(!target(&f, &dest).exists());
    let runs = dest.join(".basecraft/analyses-v1/runs");
    let ledgers: Vec<_> = fs::read_dir(&runs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("input.json").is_file())
        .collect();
    assert!(!ledgers.is_empty());
    for ledger in ledgers {
        let input = inputs::read_input(&ledger).unwrap();
        assert_eq!(input.state, "referenced");
        assert!(ledger.join("manifest.json").is_file());
        let source = f.source.join("source.ab1");
        assert_eq!(input.submitted_size, fs::metadata(&source).unwrap().len());
        let reference = input.source.as_ref().unwrap();
        assert_eq!(
            ledger.join(&reference.path).canonicalize().unwrap(),
            source.canonicalize().unwrap()
        );
        assert_eq!(reference.hash, input.submitted_hash);
        assert_eq!(
            input.submitted_hash,
            basecraft_io::hash_file(&f.source.join("source.ab1")).unwrap()
        );
    }
    f.output(&job.unwrap());
    let registry = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-stores.json");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(registry).unwrap()).unwrap();
    let stores: Vec<PathBuf> = serde_json::from_value(value["stores"].clone()).unwrap();
    assert!(stores.contains(&missing_store));
    assert!(stores.contains(&dest.join(".basecraft/analyses-v1")));
}

#[test]
fn r4_real_binding_failure_retains_visible_data_and_repair() {
    let Some(root) = enter("r4_real_binding_failure_retains_visible_data_and_repair") else {
        return;
    };
    let f = Fixture::new(&root, false);
    let dest = root.join("late");
    let mut after = None;
    let mut job = None;
    let error = f.attempt(&dest, |review, target| {
        let source = f.source.join("source.ab1");
        inputs::reference_input_with_dependencies(
            target,
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "binding fixture",
        )
        .unwrap();
        let key = basecraft_io::hash_bytes(target.file_name().unwrap().as_encoded_bytes());
        let blocker = dest
            .join(".basecraft/analyses-v1/bindings")
            .join(key.trim_start_matches("b3:"));
        fs::create_dir_all(&blocker).unwrap();
        fs::write(blocker.join("sentinel"), b"binding blocker").unwrap();
        let result = review.save_with_evidence(target, SavePolicy::Cancel);
        let run = &review.evidence.runs[0];
        for name in [
            "manifest.json",
            "draft.json",
            "review.sanger-review.json",
            "visible-review.json",
        ] {
            assert!(run.join(name).is_file(), "{name}");
        }
        assert_eq!(
            fs::read(target).unwrap(),
            fs::read(run.join("visible-review.json")).unwrap()
        );
        f.output(target);
        job = Some(new_job(review, &f));
        after = Some(tree(&dest));
        result
    });
    assert!(error.contains("data saved; binding incomplete"), "{error}");
    assert!(error.contains("repair evidence"));
    assert_eq!(tree(&dest), after.unwrap());
    f.output(&job.unwrap());
}

#[test]
fn r5_unclassified_wrapper_errors_retain_all_bytes() {
    let Some(root) = enter("r5_unclassified_wrapper_errors_retain_all_bytes") else {
        return;
    };
    let f = Fixture::new(&root, false);
    for saved in [true, false] {
        let dest = root.join(format!("unknown-{saved}"));
        let mut after = None;
        let mut job = None;
        let error = f.attempt(&dest, |review, target| {
            if saved {
                review
                    .save_with_evidence(target, SavePolicy::Cancel)
                    .unwrap();
                f.output(target);
                let path = new_job(review, &f);
                job = Some((
                    path.parent().unwrap().to_owned(),
                    tree(path.parent().unwrap()),
                ));
            } else {
                fs::write(dest.join("sentinel"), b"unrelated content").unwrap();
            }
            after = Some(tree(&dest));
            Err("opaque wrapper control: no classification allowed".into())
        });
        assert_eq!(error, "opaque wrapper control: no classification allowed");
        assert_eq!(tree(&dest), after.unwrap());
        if let Some((path, before)) = job {
            assert_eq!(tree(&path), before);
        }
    }
}

#[test]
fn r6_replacements_are_never_removed() {
    let Some(root) = enter("r6_replacements_are_never_removed") else {
        return;
    };
    let f = Fixture::new(&root, false);
    for kind in ["empty", "nonempty", "file", "symlink"] {
        if kind == "symlink" && !cfg!(unix) {
            continue;
        }
        let parent = root.join(kind);
        fs::create_dir(&parent).unwrap();
        let dest = parent.join("destination");
        let held = parent.join("held-original");
        let mut after = None;
        f.attempt(&dest, |review, target| {
            // Only the final subcase saves: moving a registered store leaves
            // an intentionally stale registration, which must not contaminate
            // a later real saver with an unrelated incomplete-scan failure.
            if kind == "symlink" {
                review
                    .save_with_evidence(target, SavePolicy::Cancel)
                    .unwrap();
                new_job(review, &f);
            } else {
                fs::write(dest.join("retained"), b"original content").unwrap();
            }
            fs::rename(&dest, &held).unwrap();
            match kind {
                "file" => fs::write(&dest, b"replacement file").unwrap(),
                "symlink" => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&held, &dest).unwrap();
                }
                _ => {
                    fs::create_dir(&dest).unwrap();
                    if kind == "nonempty" {
                        fs::write(dest.join("sentinel"), b"replacement").unwrap();
                    }
                }
            }
            #[cfg(unix)]
            assert_ne!(
                tree(&dest)[Path::new("")].identity,
                tree(&held)[Path::new("")].identity
            );
            after = Some(tree(&parent));
            Err("controlled replacement".into())
        });
        assert_eq!(tree(&parent), after.unwrap());
    }
}

#[test]
fn r7_parent_replacement_and_disappearance() {
    let Some(root) = enter("r7_parent_replacement_and_disappearance") else {
        return;
    };
    let f = Fixture::new(&root, false);
    for replace_parent in [true, false] {
        let area = root.join(format!("area-{replace_parent}"));
        let parent = area.join("parent");
        fs::create_dir_all(&parent).unwrap();
        let dest = parent.join("destination");
        let mut after = None;
        f.attempt(&dest, |_, _| {
            fs::write(dest.join("retained"), b"old destination").unwrap();
            if replace_parent {
                fs::rename(&parent, area.join("held-parent")).unwrap();
                fs::create_dir(&parent).unwrap();
                fs::write(parent.join("unrelated"), b"replacement parent").unwrap();
            } else {
                fs::rename(&dest, parent.join("held-destination")).unwrap();
            }
            after = Some(tree(&area));
            Err("controlled disappearance".into())
        });
        assert_eq!(tree(&area), after.unwrap());
        assert!(!dest.exists());
    }
}
