use super::*;
use crate::sanger_review::{instrument_review, moved_and_failed_recovery_tests::pair};
use std::collections::BTreeMap;

fn enter(name: &str) -> Option<PathBuf> {
    crate::isolated_profile::enter(&format!(
        "sanger_review::storage::inspection::tests::{name}"
    ))
}
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: (u64, u64),
    directory: bool,
    link: Option<PathBuf>,
    contents: Result<Vec<u8>, (io::ErrorKind, Option<i32>)>,
}
fn inventory(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let identity = (0, 0);
        entries.insert(
            path.strip_prefix(root).unwrap().into(),
            Entry {
                identity,
                directory: meta.is_dir(),
                link: meta
                    .file_type()
                    .is_symlink()
                    .then(|| fs::read_link(path).unwrap()),
                contents: if meta.is_file() {
                    fs::read(path).map_err(|e| (e.kind(), e.raw_os_error()))
                } else {
                    Ok(vec![])
                },
            },
        );
        if meta.is_dir() {
            for item in fs::read_dir(path).unwrap() {
                visit(root, &item.unwrap().path(), entries);
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
fn state(s: &SangerReviewSession) -> serde_json::Value {
    serde_json::json!({"draft": s.draft, "path": s.draft_path, "sha": s.draft_sha256,
        "raw": s.draft_json, "decisions": s.decisions, "manual": s.manual,
        "mask": s.mask, "settings": s.settings, "reference": s.reference,
        "events": s.events, "undo": s.undo, "redo": s.redo, "evidence": s.evidence,
        "anchor": s.anchor, "recovery": s.recovery, "accepted": s.accepted.as_ref().map(crate::save_guard::Revision::snapshot),
        "extra": s.stored_extra, "root_owner": s.save_authority.is_some(),
        "children": s.retained.iter().map(state).collect::<Vec<_>>()})
}
fn unchanged(
    root: &Path,
    review: &SangerReviewSession,
    target: &Path,
    check: impl FnOnce(&Inspection<'_>),
) {
    let before = inventory(root);
    let biology = state(review);
    let report = inspect(review, target);
    assert!(std::ptr::eq(report.root, review));
    assert_eq!(report.target, target);
    check(&report);
    assert_eq!(inventory(root), before);
    assert_eq!(state(review), biology);
    drop(report);
    assert_eq!(inventory(root), before);
    assert_eq!(state(review), biology);
}
fn packaged(s: &SangerReviewSession) -> Source {
    let input = &s.draft.inputs[0];
    Source {
        extra: BTreeMap::new(),
        original: input.path.clone(),
        sha256: input.sha256.clone(),
        path: input.path.clone(),
        ledger: None,
        reference: None,
    }
}
fn reference_set(folder: &Path) -> SangerReviewSession {
    fs::create_dir_all(folder).unwrap();
    let path = folder.join("source.ab1");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &path,
    )
    .unwrap();
    instrument_review(&path).unwrap()
}

#[test]
fn current_membership_is_borrowed_without_simulation_or_authority_changes() {
    let Some(root) =
        enter("current_membership_is_borrowed_without_simulation_or_authority_changes")
    else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let first = packaged(&s);
    s.evidence.sources.push(first.clone());
    s.evidence.sources.push(first);
    // An unmatched extra source is still a current member and gets checked.
    let mut extra = packaged(&s);
    extra.original = root.join("other-original");
    s.evidence.sources.push(extra);
    let duplicate = s.retained[0].draft.inputs[0].clone();
    s.retained[0].draft.inputs.push(duplicate);
    let target = root.join("out.json");
    let check = s.check_save(&target).unwrap();
    let token = check.token().to_owned();
    let mut scope_before = serde_json::to_value(check).unwrap();
    scope_before.as_object_mut().unwrap().remove("token");
    let owner = s.save_authority.as_ref().unwrap();
    for _ in 0..2 {
        unchanged(&root, &s, &target, |r| {
            assert_eq!(r.sets.len(), 2);
            assert_eq!(r.sets[0].position, 0);
            assert_eq!(r.sets[1].position, 1);
            assert!(std::ptr::eq(r.sets[1].set, &raw const s.retained[0]));
            assert!(matches!(
                r.sets[0].inputs[0].selection,
                Selection::Existing(0)
            ));
            assert_eq!(r.sets[0].sources.len(), 3);
            assert!(r.sets[0].sources.iter().all(|v| v.checked.is_ok()));
            assert!(std::ptr::eq(
                r.sets[0].sources[0].source,
                &raw const s.evidence.sources[0]
            ));
            assert_eq!(
                r.sets[0].inputs[0].requirement(),
                SourceRequirement::ExistingEntry
            );
            for input in &r.sets[1].inputs {
                assert_eq!(input.requirement(), SourceRequirement::ReferenceIfUnchanged);
                assert_eq!(input.input.sha256, s.draft.inputs[0].sha256);
                let Selection::Unmatched { probe, provenance } = &input.selection else {
                    panic!("no simulated append")
                };
                assert_eq!(
                    provenance,
                    &ProspectiveProvenance::UninspectedLegacyResolver
                );
                assert_eq!(
                    probe.as_ref().unwrap().sha256.as_ref().unwrap(),
                    &input.input.sha256
                );
            }
            assert!(r.dependencies.is_empty());
            assert!(
                r.previous_visible
                    .as_ref()
                    .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            );
            let RecoveryRole::NeedsJob { jobs_root, anchor } = r.recovery.as_ref().unwrap() else {
                panic!()
            };
            assert!(!jobs_root.exists());
            assert_eq!(*anchor, s.storage_anchor());
        });
    }
    assert!(std::ptr::eq(owner, s.save_authority.as_ref().unwrap()));
    s.decide_save(
        &target,
        &token,
        crate::input_evidence::SavePolicy::Cancel,
        true,
    )
    .unwrap();
    let mut scope_after = serde_json::to_value(s.check_save(&target).unwrap()).unwrap();
    scope_after.as_object_mut().unwrap().remove("token");
    assert_eq!(scope_before, scope_after);
}

#[test]
fn candidate_errors_and_legacy_skip_fallback_and_read_counts_are_preserved() {
    let Some(root) =
        enter("candidate_errors_and_legacy_skip_fallback_and_read_counts_are_preserved")
    else {
        return;
    };
    let mut s = reference_set(&root.join("data"));
    let source = s.draft.inputs[0].path.clone();
    let anchor = root.join("data/review.json");
    let bad_anchor = root.join("absent/review.json");
    let expected = s.draft.inputs[0].sha256.clone();
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    let probe = probe_candidate(&source, &bad_anchor).unwrap();
    assert_eq!(probe.candidate, source);
    assert_eq!(probe.sha256.unwrap(), expected);
    #[cfg(feature = "test-counters")]
    assert_eq!(
        basecraft_io::counters::snapshot().reads[&source].full_reads,
        1
    );
    let relative = probe_candidate(Path::new("source.ab1"), &anchor).unwrap();
    assert_eq!(relative.candidate, source);
    assert!(
        matches!(probe_candidate(Path::new("source.ab1"), &bad_anchor), Err(crate::Error::Io(e)) if e.kind() == io::ErrorKind::NotFound)
    );
    let missing = root.join("missing");
    let expected_error = basecraft_io::sha256_file(&missing).unwrap_err();
    let observed = probe_candidate(&missing, &anchor)
        .unwrap()
        .sha256
        .unwrap_err();
    let (basecraft_io::Error::Io(a), basecraft_io::Error::Io(b)) = (expected_error, observed)
    else {
        panic!()
    };
    assert_eq!(a.kind(), b.kind());
    assert_eq!(a.raw_os_error(), b.raw_os_error());
    s.evidence.sources.push(packaged(&s));
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    s.ensure_sources(&bad_anchor).unwrap(); // matching Source skips even parent resolution
    #[cfg(feature = "test-counters")]
    assert!(basecraft_io::counters::snapshot().reads.is_empty());
    s.evidence.sources.clear();
    s.draft.inputs[0].sha256 = "0".repeat(64);
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    s.ensure_sources(&anchor).unwrap();
    assert!(s.evidence.sources[0].ledger.is_none());
    #[cfg(feature = "test-counters")]
    assert_eq!(
        basecraft_io::counters::snapshot().reads[&source].full_reads,
        1
    );
    s.evidence.sources.clear();
    s.draft.inputs[0].path = missing.clone();
    s.ensure_sources(&anchor).unwrap();
    assert_eq!(s.evidence.sources[0].path, missing);
    assert!(s.evidence.sources[0].reference.is_none());
    let before = inventory(&root);
    let probe = probe_candidate(&source, &anchor).unwrap();
    assert!(probe.sha256.is_ok());
    fs::remove_file(&source).unwrap();
    assert!(matches!(
        super::super::source(&probe.candidate, &expected, &anchor),
        Err(crate::Error::Io(_))
    ));
    assert!(!root.join("data/.basecraft").exists());
    assert!(!before.is_empty());
}

#[test]
fn legacy_registration_rechecks_and_stops_after_prior_real_effects() {
    let Some(root) = enter("legacy_registration_rechecks_and_stops_after_prior_real_effects")
    else {
        return;
    };
    let mut s = reference_set(&root.join("data"));
    #[cfg(feature = "test-counters")]
    let path = s.draft.inputs[0].path.clone();
    let anchor = root.join("data/review.json");
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    s.ensure_sources(&anchor).unwrap();
    assert!(
        s.evidence.sources[0]
            .ledger
            .as_ref()
            .unwrap()
            .join("input.json")
            .is_file()
    );
    #[cfg(feature = "test-counters")]
    assert_eq!(
        basecraft_io::counters::snapshot().reads[&path].full_reads,
        4
    );
    let mut other = reference_set(&root.join("other"));
    let first = other.draft.inputs[0].clone();
    let mut second = first.clone();
    second.path = root.join("other/second.ab1");
    fs::copy(&first.path, &second.path).unwrap();
    let provenance = crate::provenance::companion(&second.path);
    fs::create_dir_all(&provenance).unwrap();
    fs::write(provenance.join("manifest.json"), "{").unwrap();
    other.draft.inputs.push(second);
    assert!(matches!(
        other.ensure_sources(&root.join("other/review.json")),
        Err(crate::Error::Json(_))
    ));
    assert_eq!(other.evidence.sources.len(), 1);
    assert_eq!(other.evidence.sources[0].original, first.path);
    assert!(other.evidence.sources[0].checked().is_ok());
    assert!(
        fs::read_dir(root.join("other/.basecraft/analyses-v1/runs"))
            .unwrap()
            .count()
            >= 2
    );
}

#[test]
fn direct_manifest_membership_and_recovery_roles_are_observations() {
    let Some(root) = enter("direct_manifest_membership_and_recovery_roles_are_observations") else {
        return;
    };
    let mut s = pair(&root.join("data"));
    s.start_recovery().unwrap();
    let ledger = s.evidence.sources[0].ledger.clone().unwrap();
    s.evidence.runs = vec![ledger.clone(), ledger.clone()];
    s.retained[0].evidence.runs = vec![ledger.clone()];
    let missing = root.join("missing-run");
    let malformed = root.join("malformed-run");
    fs::create_dir(&malformed).unwrap();
    fs::write(malformed.join("manifest.json"), "{").unwrap();
    s.retained[0]
        .evidence
        .runs
        .extend([missing.clone(), malformed.clone()]);
    // Keep source memberships but avoid ancestry reads in this direct-read count control.
    for source in &mut s.evidence.sources {
        source.reference = None;
    }
    for child in &mut s.retained {
        for source in &mut child.evidence.sources {
            source.reference = None;
        }
    }
    let target = root.join("output.json");
    unchanged(&root, &s, &target, |r| {
        assert_eq!(r.sets[0].runs.len(), 2);
        assert_eq!(r.sets[1].runs.len(), 3);
        assert!(r.sets[0].sources[0].checked.is_err());
        assert!(r.dependencies.windows(2).all(|w| w[0].path < w[1].path));
        assert_eq!(
            r.dependencies.iter().filter(|d| d.path == ledger).count(),
            1
        );
        assert!(
            r.dependencies
                .iter()
                .find(|d| d.path == ledger)
                .unwrap()
                .manifest
                .as_ref()
                .unwrap()
                .is_ok()
        );
        assert!(
            r.dependencies
                .iter()
                .find(|d| d.path == missing)
                .unwrap()
                .manifest
                .is_err()
        );
        assert!(
            r.dependencies
                .iter()
                .find(|d| d.path == malformed)
                .unwrap()
                .manifest
                .as_ref()
                .unwrap()
                .is_err()
        );
        let RecoveryRole::Existing {
            review,
            manifest,
            anchor,
        } = r.recovery.as_ref().unwrap()
        else {
            panic!()
        };
        assert_eq!(Some(*review), s.recovery_path());
        assert_eq!(manifest, &review.with_file_name("manifest.json"));
        assert_eq!(*anchor, s.storage_anchor());
        assert!(
            r.sets[0]
                .roles
                .iter()
                .any(|p| p.kind == Role::CheckpointReview && p.path.as_ref() == *review)
        );
        assert!(
            r.sets[1]
                .roles
                .iter()
                .any(|p| p.kind == Role::PreservedInput)
        );
    });
    #[cfg(feature = "test-counters")]
    {
        basecraft_io::counters::reset();
        let mut reads = BTreeMap::new();
        let r = inspect_with_manifest_reader(&s, &target, |path| {
            *reads.entry(path.to_path_buf()).or_insert(0usize) += 1;
            crate::provenance::read_record(path)
        });
        let counts = basecraft_io::counters::snapshot();
        for dep in &r.dependencies {
            let p = dep.path.join("manifest.json");
            assert_eq!(reads[&p], 1);
        }
        assert_eq!(counts.sync_all, 0);
        assert_eq!(counts.sync_data, 0);
        assert!(counts.store_inspections.is_empty());
    }
}

#[test]
fn existing_sources_use_real_verification_without_reselection() {
    let Some(root) = enter("existing_sources_use_real_verification_without_reselection") else {
        return;
    };
    let mut s = reference_set(&root.join("data"));
    let target = root.join("data/review.json");
    s.ensure_sources(&target).unwrap();
    let original = s.evidence.sources[0].clone();
    let ledger = original.ledger.clone().unwrap();
    let input = crate::input_evidence::read_input(&ledger).unwrap();
    let mut plain = original.clone();
    plain.ledger = None;
    plain.reference = None;
    let mut half = original.clone();
    half.reference = None;
    let mut wrong_sha = plain.clone();
    wrong_sha.sha256 = "f".repeat(64);
    let mut wrong_size = original.clone();
    wrong_size.reference.as_mut().unwrap().stamp.size += 1;
    s.evidence
        .sources
        .extend([plain, half, wrong_sha, wrong_size]);
    unchanged(&root, &s, &target, |r| {
        assert!(matches!(
            r.sets[0].inputs[0].selection,
            Selection::Existing(0)
        ));
        assert!(r.sets[0].sources[0].checked.is_ok());
        assert!(r.sets[0].sources[1].checked.is_ok());
        assert!(r.sets[0].sources[2..].iter().all(|s| s.checked.is_err()));
    });
    crate::input_evidence::Mutation::begin()
        .unwrap()
        .preserve_input(&ledger)
        .unwrap();
    fs::remove_file(&original.path).unwrap();
    unchanged(&root, &s, &target, |r| {
        assert_eq!(
            r.sets[0].sources[0].checked.as_ref().unwrap(),
            &ledger.join("preserved-input")
        );
        assert!(r.sets[0].sources[1].checked.is_err());
        assert!(matches!(
            r.sets[0].inputs[0].selection,
            Selection::Existing(0)
        ));
    });
    let mut broken = crate::input_evidence::read_input(&ledger).unwrap();
    broken.state = "broken".into();
    crate::input_evidence::write_json(&ledger.join("input.json"), &broken).unwrap();
    unchanged(&root, &s, &target, |r| {
        assert!(r.sets[0].sources[0].checked.is_err());
    });
    let cycle = root.join("cycle");
    fs::create_dir(&cycle).unwrap();
    let mut cyclic = input.clone();
    cyclic.extra.insert("origin_run".into(), ".".into());
    crate::input_evidence::write_json(&cycle.join("input.json"), &cyclic).unwrap();
    s.evidence.sources[0].ledger = Some(cycle);
    unchanged(&root, &s, &target, |r| {
        assert!(
            matches!(&r.sets[0].sources[0].checked, Err(crate::Error::Invalid(message)) if message == "input evidence contains a cycle")
        );
    });
    for i in 0..34 {
        let dir = root.join(format!("chain{i}"));
        fs::create_dir(&dir).unwrap();
        let mut link = input.clone();
        if i < 33 {
            link.extra
                .insert("origin_run".into(), format!("../chain{}", i + 1).into());
        }
        crate::input_evidence::write_json(&dir.join("input.json"), &link).unwrap();
    }
    s.evidence.sources[0].ledger = Some(root.join("chain0"));
    unchanged(&root, &s, &target, |r| {
        assert!(
            matches!(&r.sets[0].sources[0].checked, Err(crate::Error::Invalid(message)) if message == "input evidence ancestry exceeds 32 references")
        );
    });
}

fn methods_run(root: &Path, name: &str, text: Option<&str>) -> PathBuf {
    let run = root.join(name);
    fs::create_dir_all(run.join("reports")).unwrap();
    if let Some(text) = text {
        fs::write(run.join("reports/methods.md"), text).unwrap();
    }
    run
}
#[test]
fn methods_membership_keeps_selected_precedence_and_optional_absence() {
    let Some(root) = enter("methods_membership_keeps_selected_precedence_and_optional_absence")
    else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let absent = methods_run(&root, "absent", None);
    let directory = methods_run(&root, "directory", None);
    fs::create_dir(directory.join("reports/methods.md")).unwrap();
    let first = methods_run(&root, "first", Some("authored first"));
    let later = methods_run(&root, "later", Some("authored later"));
    let inactive = methods_run(&root, "inactive", Some("inactive authored"));
    s.evidence.runs = vec![absent, directory, first.clone(), later];
    s.retained[0].evidence.runs = vec![inactive.clone()];
    unchanged(&root, &s, &root.join("out.json"), |r| {
        let runs = &r.sets[0].runs;
        assert!(
            runs[0]
                .methods
                .followed
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
        );
        assert!(runs[1].methods.followed.as_ref().unwrap().is_dir());
        assert_eq!(runs[2].run, first);
        assert_eq!(runs[2].methods.path, first.join("reports/methods.md"));
        assert!(runs[2].methods.legacy_selected);
        assert!(matches!(
            runs[2].methods.open,
            OpenObservation::Attempted(Ok(()))
        ));
        assert!(!runs[3].methods.legacy_selected);
        assert!(matches!(
            runs[3].methods.open,
            OpenObservation::NotRequested
        ));
        assert_eq!(r.sets[1].runs[0].run, inactive);
        assert!(!r.sets[1].runs[0].methods.legacy_selected);
        assert!(r.sets[1].runs[0].methods.leaf.as_ref().unwrap().is_file());
        assert!(r.dependencies.iter().all(|d| d.manifest.is_err())); // unreadable required support is not optional methods absence
    });
}

#[cfg(unix)]
#[test]
fn symlink_and_denied_diagnostics_never_fall_back_or_write() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let Some(root) = enter("symlink_and_denied_diagnostics_never_fall_back_or_write") else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let target = root.join("out.json");
    let dangling = methods_run(&root, "dangling", None);
    symlink(
        root.join("no-such-file"),
        dangling.join("reports/methods.md"),
    )
    .unwrap();
    let live = methods_run(&root, "live", None);
    let text = root.join("authored.txt");
    fs::write(&text, "authored").unwrap();
    symlink(&text, live.join("reports/methods.md")).unwrap();
    let later = methods_run(&root, "later", Some("later"));
    s.evidence.runs = vec![dangling, live, later];
    unchanged(&root, &s, &target, |r| {
        assert!(r.sets[0].runs[0].methods.followed.is_err());
        assert!(
            r.sets[0].runs[0]
                .methods
                .leaf
                .as_ref()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(r.sets[0].runs[1].methods.legacy_selected);
        assert!(matches!(
            r.sets[0].runs[1].methods.open,
            OpenObservation::Symlink
        ));
        assert!(!r.sets[0].runs[2].methods.legacy_selected);
    });
    let denied = methods_run(&root, "denied", Some("private"));
    let file = denied.join("reports/methods.md");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    let raw = fs::File::open(&file).unwrap_err();
    assert_eq!(
        raw.kind(),
        io::ErrorKind::PermissionDenied,
        "requires ordinary user"
    );
    s.evidence.runs.insert(0, denied);
    unchanged(&root, &s, &target, |r| {
        assert!(r.sets[0].runs[0].methods.legacy_selected);
        let OpenObservation::Attempted(Err(e)) = &r.sets[0].runs[0].methods.open else {
            panic!()
        };
        assert_eq!(e.kind(), raw.kind());
        assert_eq!(e.raw_os_error(), raw.raw_os_error());
        assert!(
            r.sets[0].runs[1..]
                .iter()
                .all(|r| !r.methods.legacy_selected)
        );
    });
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let source = s.draft.inputs[0].path.clone();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
    unchanged(&root, &s, &target, |r| {
        assert_eq!(
            r.sets[0].inputs[0].requirement(),
            SourceRequirement::UnavailableUnderLegacyFallback
        );
        let Selection::Unmatched {
            probe: Ok(probe), ..
        } = &r.sets[0].inputs[0].selection
        else {
            panic!()
        };
        assert!(
            matches!(&probe.sha256, Err(basecraft_io::Error::Io(e)) if e.kind() == io::ErrorKind::PermissionDenied)
        );
    });
    s.ensure_sources(&target).unwrap();
    assert!(s.evidence.sources[0].ledger.is_none());
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn placement_and_recovery_diagnostics_preserve_errors_and_remaining_facts() {
    let Some(root) =
        enter("placement_and_recovery_diagnostics_preserve_errors_and_remaining_facts")
    else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let support = root.join("placement.json");
    fs::write(&support, "not placement JSON").unwrap();
    s.retained[0].evidence.placement = Some(super::super::Support {
        extra: BTreeMap::new(),
        path: support.clone(),
        hash: basecraft_io::hash_file(&support).unwrap(),
    });
    s.recovery = Some(root.join("absent-job/review.sanger-review.json"));
    s.retained[0].draft.inputs[0].path = "relative.ab1".into();
    let target = root.join("missing-parent/out.json");
    let error = s.retained[0].placement().unwrap_err();
    unchanged(&root, &s, &target, |r| {
        assert_eq!(
            r.sets[1].inputs[0].requirement(),
            SourceRequirement::CandidateUnresolved
        );
        assert!(matches!(
            r.sets[0].inputs[0].selection,
            Selection::Unmatched { probe: Ok(_), .. }
        ));
        assert!(
            matches!(&r.sets[1].inputs[0].selection, Selection::Unmatched { probe: Err(crate::Error::Io(e)), .. } if e.kind() == io::ErrorKind::NotFound)
        );
        assert_eq!(
            r.sets[1]
                .placement_context
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err(),
            &error
        );
        assert!(
            r.sets[1]
                .roles
                .iter()
                .any(|p| p.kind == Role::Placement && p.path.as_ref() == support)
        );
        assert!(
            matches!(&r.recovery, Ok(RecoveryRole::Existing { review, .. }) if !review.exists())
        );
        assert_eq!(r.sets[0].set.draft_json, s.draft_json);
    });
    let alignment = root.join("context.afa");
    fs::write(&alignment, ">a\nAC\n>b\nAC\n").unwrap();
    let sha = basecraft_io::sha256_file(&alignment).unwrap();
    let context = super::super::source(&alignment, &sha, &root.join("out.json")).unwrap();
    s.retained[0].evidence.sources.push(context);
    let placement = basecraft_docs::sanger::SangerPlacement {
        status: "candidate_requires_review".into(),
        engine: "MAFFT".into(),
        version: "fixture".into(),
        source_path: alignment.clone(),
        source_sha256: sha,
        source_rows: 2,
        source_width: 2,
        fragment: "AC".into(),
        map: vec![Some(0), Some(1)],
        aligned_probe: "AC".into(),
        insertions: 0,
        parameters: vec!["--mapout".into()],
    };
    placement.validate().unwrap();
    crate::input_evidence::write_json(&support, &placement).unwrap();
    s.retained[0].evidence.placement.as_mut().unwrap().hash =
        basecraft_io::hash_file(&support).unwrap();
    unchanged(&root, &s, &target, |r| {
        assert_eq!(
            r.sets[1]
                .placement_context
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap(),
            &alignment
        );
        assert!(
            r.sets[1]
                .roles
                .iter()
                .any(|p| p.kind == Role::PlacementContext && p.path.as_ref() == alignment)
        );
    });
    s.retained[0].evidence.placement.as_mut().unwrap().hash = "changed".into();
    unchanged(&root, &s, &target, |r| {
        assert!(r.sets[1].placement_context.as_ref().unwrap().is_err());
    });
    fs::remove_file(&support).unwrap();
    unchanged(&root, &s, &target, |r| {
        assert!(r.sets[1].placement_context.as_ref().unwrap().is_err());
    });
}

#[test]
fn path_discovery_failure_is_a_component_not_an_empty_report() {
    let name = "path_discovery_failure_is_a_component_not_an_empty_report";
    let Some(root) = enter(name) else {
        return;
    };
    if std::env::var_os("BASECRAFT_INSPECTION_PATH_ERROR").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("sanger_review::storage::inspection::tests::{name}"),
                "--nocapture",
            ])
            .env("BASECRAFT_INSPECTION_PATH_ERROR", "1")
            .env("BASECRAFT_STATE_DIR", "relative-state")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let s = pair(&root.join("data"));
    let expected = AppPaths::discover().unwrap_err();
    unchanged(&root, &s, &root.join("out.json"), |r| {
        assert_eq!(r.recovery.as_ref().err().unwrap().0, expected.0);
        assert_eq!(r.sets.len(), 2);
        assert!(matches!(
            r.sets[0].inputs[0].selection,
            Selection::Unmatched { probe: Ok(_), .. }
        ));
    });
}
