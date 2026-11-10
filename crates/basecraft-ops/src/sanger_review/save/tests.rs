use super::*;
use crate::sanger_review::moved_and_failed_recovery_tests::pair;

fn enter(name: &str) -> Option<PathBuf> {
    crate::isolated_profile::enter(&format!("sanger_review::save::tests::{name}"))
}
fn decision(s: &SangerReviewSession, path: &Path) -> SaveDecision {
    s.check_save(path).unwrap().decide(SavePolicy::Cancel, true)
}
// Exercise only the real common component boundary, never a simulated writer.
fn consume(s: &SangerReviewSession, d: &SaveDecision) -> bool {
    let (owner, _) = s.authority().unwrap();
    if PendingSaveAuthority::origin(&owner.origin, d) != Some(0) {
        return false;
    }
    owner.pending.consume(d).unwrap()
}
fn inventory(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, all: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        let meta = std::fs::symlink_metadata(path).unwrap();
        assert!(!meta.is_symlink());
        all.insert(
            path.strip_prefix(root).unwrap().into(),
            meta.is_file().then(|| std::fs::read(path).unwrap()),
        );
        if meta.is_dir() {
            for item in std::fs::read_dir(path).unwrap() {
                visit(root, &item.unwrap().path(), all);
            }
        }
    }
    let mut all = BTreeMap::new();
    visit(root, root, &mut all);
    all
}

#[test]
fn advisory_is_read_only_and_receipts_supersede() {
    let Some(root) = enter("advisory_is_read_only_and_receipts_supersede") else {
        return;
    };
    let s = pair(&root.join("data"));
    let path = root.join("new.json");
    let before = inventory(&root);
    let state = s.state_digest().unwrap();
    let old = decision(&s, &path);
    let new = s.check_save(&path).unwrap();
    let d = s
        .decide_save(&path, new.token(), SavePolicy::PreserveCopy, false)
        .unwrap();
    assert!(!consume(&s, &old));
    assert!(consume(&s, &d));
    assert!(!consume(&s, &d));
    assert_eq!(state, s.state_digest().unwrap());
    assert_eq!(before, inventory(&root));
    let newer = s.check_save(&path).unwrap();
    assert!(matches!(
        s.decide_save(&path, new.token(), SavePolicy::Cancel, true),
        Err(crate::Error::SaveDecisionStale(_))
    ));
    assert!(!consume(&s, &newer.decide(SavePolicy::Cancel, true)));
    assert_eq!(before, inventory(&root));
}

#[test]
fn root_clone_fork_load_move_and_foreign_owners() {
    let Some(root) = enter("root_clone_fork_load_move_and_foreign_owners") else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let target = root.join("new.json");
    assert!(s.retained.iter().all(|c| c.save_authority.is_none()));
    assert!(s.retained[0].check_save(&target).is_err());
    let original_check = s.check_save(&target).unwrap();
    let d = original_check.clone().decide(SavePolicy::Cancel, true);
    let copy = s.clone();
    let fork = s.fork_for_operation();
    assert_eq!(s.state_digest().unwrap(), copy.state_digest().unwrap());
    for other in [&copy, &fork] {
        assert_ne!(
            s.authority().unwrap().0.origin,
            other.authority().unwrap().0.origin
        );
        assert!(other.retained.iter().all(|c| c.save_authority.is_none()));
        assert!(!consume(other, &d));
    }
    assert!(fork.recovery.is_none());
    let sequence = root.join("input.fasta");
    std::fs::write(&sequence, ">x\nACGT\n").unwrap();
    let mut session = crate::Session::new(&root);
    let id = session.open(&sequence).unwrap();
    let foreign = session
        .check_save(id, None)
        .unwrap()
        .decide(SavePolicy::Cancel, true);
    assert!(!consume(&s, &foreign));
    assert!(matches!(
        session.save_with_decision(id, None, d),
        Err(crate::Error::SaveDecisionStale(_))
    ));
    // Foreign Session did not consume Sanger's pending origin.
    assert!(consume(
        &s,
        &original_check.decide(SavePolicy::Cancel, true)
    ));
    let pending = s
        .authority()
        .unwrap()
        .0
        .pending
        .decide(
            s.check_save(&target).unwrap().token(),
            SavePolicy::Cancel,
            true,
        )
        .unwrap()
        .unwrap();
    let moved = s;
    assert!(consume(&moved, &pending));
    s = moved;
    let saved = root.join("saved.json");
    s.save(&saved).unwrap();
    let d = decision(&s, &target);
    let loaded = SangerReviewSession::load(&saved).unwrap();
    assert!(!consume(&loaded, &d));
    assert!(consume(&s, &d));
    assert!(loaded.retained.iter().all(|c| c.save_authority.is_none()));
}

#[test]
fn selection_attachment_failed_mutation_and_epoch_exhaustion() {
    let Some(root) = enter("selection_attachment_failed_mutation_and_epoch_exhaustion") else {
        return;
    };
    let mut s = pair(&root.join("data"));
    let path = root.join("new.json");
    let origin = s.authority().unwrap().0.origin.clone();
    let first = s.draft_sha256.clone();
    let other = s.retained[0].draft_sha256.clone();
    let d = decision(&s, &path);
    s.select_call_set(&other).unwrap();
    s.select_call_set(&first).unwrap();
    assert_eq!(origin, s.authority().unwrap().0.origin);
    assert!(!consume(&s, &d));
    assert!(s.retained.iter().all(|c| c.save_authority.is_none()));
    let d = decision(&s, &path);
    assert!(s.select_call_set("missing").is_err());
    assert!(!consume(&s, &d));
    let d = decision(&s, &path);
    assert!(
        s.apply(ReviewOp::Set {
            columns: vec![],
            choice: ReviewChoice::Accept
        })
        .is_err()
    );
    assert!(!consume(&s, &d));
    let d = decision(&s, &path);
    s.apply(ReviewOp::Undo).unwrap();
    s.apply(ReviewOp::Redo).unwrap();
    assert!(!consume(&s, &d));
    let mut draft = s.draft.clone();
    draft.version_output.push_str(" another");
    let child = SangerReviewSession::from_draft(draft, &s.draft_path).unwrap();
    let child_d = decision(&child, &root.join("child.json"));
    s.retain_call_set(child).unwrap();
    let id = s.retained.last().unwrap().draft_sha256.clone();
    s.select_call_set(&id).unwrap();
    assert!(!consume(&s, &child_d));
    assert_eq!(origin, s.authority().unwrap().0.origin);
    let d = decision(&s, &path);
    assert!(s.attach_operation_run(&root.join("missing")).is_err());
    assert!(!consume(&s, &d));
    let previous = s.clone();
    let d = decision(&s, &path);
    s.inherit_review_ancestry(&previous);
    assert!(!consume(&s, &d));
    s.save_authority.as_mut().unwrap().epoch = Some(u64::MAX);
    let d = decision(&s, &path);
    s.inherit_review_ancestry(&previous);
    assert!(s.save_authority.as_ref().unwrap().epoch.is_none());
    assert!(s.check_save(&path).is_err());
    assert!(
        s.decide_save(&path, "anything", SavePolicy::Cancel, true)
            .is_err()
    );
    assert!(
        !s.save_authority
            .as_ref()
            .unwrap()
            .pending
            .consume(&d)
            .unwrap()
    );
    s.invalidate_save_authority();
    assert!(s.check_save(&path).is_err());
}

#[test]
fn binding_covers_public_inactive_inverse_evidence_and_operational_state() {
    let Some(root) = enter("binding_covers_public_inactive_inverse_evidence_and_operational_state")
    else {
        return;
    };
    let s = pair(&root.join("data"));
    let changes: &[fn(&mut SangerReviewSession)] = &[
        |s| s.draft.engine.push('x'),
        |s| s.draft_path.push("x"),
        |s| s.draft_sha256.push('x'),
        |s| s.draft_json.push(' '),
        |s| s.decisions[0] = ReviewChoice::Reject,
        |s| s.manual[0] = !s.manual[0],
        |s| s.mask.start += 1,
        |s| s.settings.strong_score += 1,
        |s| {
            s.reference = Some(ReviewReference {
                source_path: "reference".into(),
                source_sha256: "a".repeat(64),
                counts: vec![Some([1, 2, 3, 4])],
            })
        },
        |s| s.events.push(ReviewOp::Undo),
        |s| s.undo[0].columns.push(99),
        |s| s.undo[0].before.push(ReviewChoice::Reject),
        |s| s.undo[0].after.push(ReviewChoice::Accept),
        |s| s.undo[0].manual_before.push(true),
        |s| s.undo[0].manual_after.push(true),
        |s| s.undo[0].mask = Some((s.mask.clone(), s.mask.clone())),
        |s| s.undo[0].triage = Some((s.settings.clone(), None, s.settings.clone(), None)),
        |s| s.redo.push(s.undo[0].clone()),
        |s| s.evidence.schema_version += 1,
        |s| s.evidence.runs.push(PathBuf::from("support")),
        |s| {
            s.evidence.extra.insert("extension".into(), true.into());
        },
        |s| {
            s.stored_extra.insert("extension".into(), true.into());
        },
        |s| s.anchor.push("other"),
        |s| s.recovery = Some(PathBuf::from("job")),
    ];
    for (i, change) in changes.iter().enumerate() {
        for inactive in [false, true] {
            let mut copy = s.clone();
            let before = copy.state_digest().unwrap();
            change(if inactive {
                &mut copy.retained[0]
            } else {
                &mut copy
            });
            assert_ne!(
                before,
                copy.state_digest().unwrap(),
                "change {i}, inactive={inactive}"
            );
        }
    }
    let path = root.join("accepted.json");
    std::fs::write(&path, "accepted").unwrap();
    let mut copy = s.clone();
    let before = copy.state_digest().unwrap();
    copy.retained[0].accepted =
        Some(crate::save_guard::Revision::accepted(&path, b"accepted").unwrap());
    assert_ne!(before, copy.state_digest().unwrap());
    // Publishing/superseding receipts does not feed tokens into the digest.
    let before = copy.state_digest().unwrap();
    copy.check_save(&root.join("absent.json")).unwrap();
    assert_eq!(before, copy.state_digest().unwrap());
}

#[test]
fn source_placement_extensions_and_failed_selection_are_bound() {
    let Some(root) = enter("source_placement_extensions_and_failed_selection_are_bound") else {
        return;
    };
    let mut s = pair(&root.join("data"));
    // Obtain real registered source/reference and placement-shaped borrowed data.
    s.start_recovery().unwrap();
    assert!(!s.evidence.sources.is_empty());
    let changes: &[fn(&mut SangerReviewSession)] = &[
        |s| s.evidence.sources[0].original.push("different"),
        |s| s.evidence.sources[0].path.push("different"),
        |s| s.evidence.sources[0].sha256.push('0'),
        |s| s.evidence.sources[0].ledger = Some("other-ledger".into()),
        |s| {
            s.evidence.sources[0]
                .reference
                .as_mut()
                .unwrap()
                .hash
                .push('0')
        },
        |s| {
            s.evidence.sources[0]
                .extra
                .insert("extension".into(), false.into());
        },
        |s| {
            s.evidence.placement = Some(crate::sanger_review::storage::Support {
                extra: BTreeMap::new(),
                path: "placement".into(),
                hash: "hash".into(),
            })
        },
    ];
    for change in changes {
        for inactive in [false, true] {
            let mut copy = s.clone();
            let before = copy.state_digest().unwrap();
            change(if inactive {
                &mut copy.retained[0]
            } else {
                &mut copy
            });
            assert_ne!(before, copy.state_digest().unwrap());
        }
    }
    let target = root.join("new.json");
    // Existing checkpoint failure rolls selection back but must not revive consent.
    let original = s.draft_sha256.clone();
    let alternative = s.retained[0].draft_sha256.clone();
    let checkpoint = s.recovery.clone().unwrap();
    std::fs::remove_file(&checkpoint).unwrap();
    std::fs::create_dir(&checkpoint).unwrap();
    let d = decision(&s, &target);
    assert!(s.select_call_set(&alternative).is_err());
    assert_eq!(original, s.draft_sha256);
    assert!(!consume(&s, &d));
    assert!(s.retained.iter().all(|c| c.save_authority.is_none()));
}

fn non_finite_binding_control(root: &Path, overlap: bool) {
    let mut review = pair(&root.join("data"));
    let target = root.join("new.json");
    {
        let set = &mut review;
        // Two entries prove validation traverses the whole overlap list.
        set.draft.overlaps = vec![
            basecraft_docs::sanger::Overlap {
                reads: [0, 0],
                comparable_bases: 1,
                identity: 0.75,
                accepted: true,
            };
            2
        ];
        set.retained[0].draft.overlaps = set.draft.overlaps.clone();
    }
    let initial_digest = review.state_digest().unwrap();
    review.check_save(&target).unwrap();
    let before = inventory(root);
    for inactive in [false, true] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut changed = review.clone();
            let set = if inactive {
                &mut changed.retained[0]
            } else {
                &mut changed
            };
            let field = if overlap {
                &mut set.draft.overlaps[1].identity
            } else {
                &mut set.draft.minimum_identity
            };
            let finite = *field;
            *field = value;
            assert!(matches!(
                changed.state_digest(),
                Err(crate::Error::Invalid(_))
            ));
            assert!(matches!(
                changed.check_save(&target),
                Err(crate::Error::Invalid(_))
            ));
            assert_eq!(before, inventory(root));
            let set = if inactive {
                &mut changed.retained[0]
            } else {
                &mut changed
            };
            if overlap {
                set.draft.overlaps[1].identity = finite;
            } else {
                set.draft.minimum_identity = finite;
            }
            assert_eq!(initial_digest, changed.state_digest().unwrap());
            changed.check_save(&target).unwrap();
            let set = if inactive {
                &mut changed.retained[0]
            } else {
                &mut changed
            };
            if overlap {
                set.draft.overlaps[1].identity = finite + 0.125;
            } else {
                set.draft.minimum_identity = finite + 0.125;
            }
            assert_ne!(initial_digest, changed.state_digest().unwrap());
            changed.check_save(&target).unwrap();
            assert_eq!(before, inventory(root));
        }
    }
}

#[test]
fn non_finite_root_and_inactive_minimum_identity_refuse_binding() {
    let Some(root) = enter("non_finite_root_and_inactive_minimum_identity_refuse_binding") else {
        return;
    };
    non_finite_binding_control(&root, false);
}

#[test]
fn non_finite_root_and_inactive_nested_overlap_refuse_binding() {
    let Some(root) = enter("non_finite_root_and_inactive_nested_overlap_refuse_binding") else {
        return;
    };
    non_finite_binding_control(&root, true);
}
