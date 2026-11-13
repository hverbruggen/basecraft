//! Production scanner-entry counts, including unsuccessful and repeated visits.
use basecraft_io::counters;
use basecraft_ops::{
    Session,
    records::ArtifactRun,
    records::{self, SavePolicy},
};
use basecraft_protocol::Op;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const ORIGINAL: &[u8] = b">gene\nACGT\n";

fn setup(root: &Path) -> (Session, basecraft_ops::DocId, PathBuf) {
    let source = root.join("input.fasta");
    std::fs::write(&source, ORIGINAL).unwrap();
    let mut session = Session::recoverable(root).unwrap();
    let doc = session.open(&source).unwrap();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 1,
                end: 2,
                residues: "T".into(),
            },
        )
        .unwrap();
    (session, doc, source)
}

fn store(root: &Path, name: &str) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir(&directory).unwrap();
    let run = ArtifactRun::create(&directory.join("result.txt"), "reserved").unwrap();
    records::register(&run).unwrap();
    run.directory()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

// Count actual scanner visits; unrelated stores are excluded by the index.
fn record(stage: &str, stores: &[PathBuf], visits: u64) {
    let counts = counters::snapshot();
    let expected: BTreeMap<_, _> = stores.iter().cloned().map(|s| (s, visits)).collect();
    assert_eq!(
        counts.store_inspections, expected,
        "{stage}: attempted visits by supplied store path"
    );
    println!(
        "{stage}: attempts={} repeats={} visits={:?}",
        counts.store_inspections.values().sum::<u64>(),
        counts
            .store_inspections
            .values()
            .map(|n| n - 1)
            .sum::<u64>(),
        counts.store_inspections
    );
}

#[test]
fn default_and_unrelated_scaling() {
    let Some(root) = super::child_profile("store_budgets::default_and_unrelated_scaling") else {
        return;
    };
    let (mut session, doc, source) = setup(&root);
    counters::reset();
    session.save(doc, None).unwrap();
    record("default save without stores", &[], 0);
    // Ordinary provenance publication does not create an analysis store. Each
    // additional unrelated registered store must add no scanner visit per save.
    let mut stores = Vec::new();
    for index in 0..3 {
        stores.push(store(&root, &format!("unrelated-{index}")));
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 1,
                    end: 2,
                    residues: if index % 2 == 0 { "G" } else { "T" }.into(),
                },
            )
            .unwrap();
        counters::reset();
        session.save(doc, None).unwrap();
        record("save with unrelated stores", &[], 0);
        assert_ne!(std::fs::read(&source).unwrap(), ORIGINAL);
    }
    counters::reset();
    session.save(doc, None).unwrap();
    record("clean no-op save", &[], 0);
    let counts = counters::snapshot();
    assert!(counts.reads.is_empty());
    assert_eq!((counts.sync_data, counts.sync_all), (0, 0));
}

#[test]
fn incomplete_decision_and_recheck() {
    let Some(root) = super::child_profile("store_budgets::incomplete_decision_and_recheck") else {
        return;
    };
    let (mut session, doc, source) = setup(&root);
    // A deleted unrelated store is never visited. A damaged store listed
    // beside the source cannot be read, so its relevance is unknown.
    let missing = store(&root, "missing");
    std::fs::remove_dir_all(&missing).unwrap();
    std::fs::create_dir(root.join("corrupt")).unwrap();
    let run = records::reference_input_with_dependencies(
        &root.join("corrupt/result.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "budget-test",
        None,
    )
    .unwrap();
    let corrupt = run
        .directory()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(corrupt.join("store.json"), b"damaged").unwrap();
    let stores = [corrupt];
    counters::reset();
    let check = match session.save(doc, None).unwrap_err() {
        basecraft_ops::Error::SaveCheckRequired(check) => check,
        other => panic!("unexpected save error: {other}"),
    };
    assert!(!check.evidence().is_complete());
    record("incomplete save refused", &stores, 1);
    assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
    // Deliberately do not reset: consent resolution is scan-free, then the
    // authoritative save revisits both failing stores.
    let decision = session
        .decide_save(doc, None, check.token(), SavePolicy::Cancel, true)
        .unwrap();
    record("decision binding", &stores, 1);
    let outcome = session.save_with_decision(doc, None, decision).unwrap();
    assert_eq!(outcome.warnings.len(), 1);
    record("warned save cumulative", &stores, 2);
    counters::reset();
    let check = session.check_save(doc, None).unwrap();
    let all = stores.to_vec();
    record("advisory check", &all, 1);
    counters::reset();
    session
        .save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true))
        .unwrap();
    record("explicit decision on clean document", &all, 1);
}

#[test]
fn unrelated_registration_and_known_preserve() {
    let Some(root) =
        super::child_profile("store_budgets::unrelated_registration_and_known_preserve")
    else {
        return;
    };
    let (mut session, doc, source) = setup(&root);
    let run = records::reference_input_with_dependencies(
        &root.join("result.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "budget-test",
        None,
    )
    .unwrap();
    let nearby = root.join(".basecraft/analyses-v1");
    counters::reset();
    let check = session.check_save(doc, None).unwrap();
    assert!(check.evidence().is_complete());
    assert!(!check.evidence().conflict.analyses.is_empty());
    record("known advisory", std::slice::from_ref(&nearby), 1);
    store(&root, "new-store");
    counters::reset();
    session
        .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, false))
        .unwrap();
    record("unrelated registration preserves consent", &[nearby], 1);
    assert_eq!(
        std::fs::read(run.directory().join("preserved-input")).unwrap(),
        ORIGINAL
    );
}

#[test]
fn explicit_checks_and_early_failure() {
    let Some(root) = super::child_profile("store_budgets::explicit_checks_and_early_failure")
    else {
        return;
    };
    let (mut session, doc, source) = setup(&root);
    let missing = root.join("missing");
    counters::reset();
    let check = records::check_in(&source, &[missing.clone(), missing.clone()]).unwrap();
    assert!(!check.is_complete());
    record("explicit repeated missing store", &[missing], 2);
    let target = root.join("existing.fasta");
    std::fs::write(&target, ORIGINAL).unwrap();
    counters::reset();
    assert!(session.save_new(doc, Some(&target)).is_err());
    record("no-overwrite failure", &[], 0);
    assert_eq!(std::fs::read(source).unwrap(), ORIGINAL);
}
