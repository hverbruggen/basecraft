use super::{observation::*, *};
use crate::analysis_store::publication_observation::{
    ObservationPhase, ParentSync, PersistFact, StagedWitness, TargetObservation,
};
use crate::{Error, metadata};
use basecraft_platform::files::{OrdinaryKind, object_identity};
use basecraft_protocol::metadata::{FieldTarget, HeaderPattern};
use std::{collections::BTreeMap, fs, io};

type Inventory = BTreeMap<PathBuf, Option<Vec<u8>>>;
fn inventory(root: &Path) -> Inventory {
    fn walk(root: &Path, at: &Path, out: &mut Inventory) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if entry.file_type().unwrap().is_dir() {
                out.insert(relative, None);
                walk(root, &path, out);
            } else {
                out.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut out = Inventory::new();
    walk(root, root, &mut out);
    out
}
struct Fixture {
    root: PathBuf,
    doc: OpenedMetadata,
    target: PathBuf,
    bytes: Vec<Vec<u8>>,
}
impl Fixture {
    fn new(root: &Path) -> Self {
        fs::create_dir_all(root).unwrap();
        let source = metadata::create(&root.join("source")).unwrap();
        let mut doc = metadata::load(&source).unwrap();
        let pattern = HeaderPattern {
            name: "Unicode import".into(),
            separator: "|".into(),
            fields: vec![FieldTarget::Specimen, FieldTarget::Taxon],
        };
        let plan = metadata::plan_headers(
            &doc.metadata,
            "input.fasta",
            &[
                ("r1".into(), "s1|Cáulérpa taxifolia".into()),
                ("r2".into(), "s2|Codium fragile".into()),
            ],
            &pattern,
            metadata::ImportOptions::default(),
        )
        .unwrap();
        basecraft_docs::metadata::apply(
            &mut doc.metadata,
            &basecraft_protocol::metadata::MetadataEdit::Apply {
                changes: plan.changes,
            },
        )
        .unwrap();
        doc.metadata.import.header_patterns.push(pattern);
        doc.metadata
            .extra
            .insert("future".into(), serde_json::json!({"retained": ["α", 42]}));
        doc.metadata
            .partitions
            .push(basecraft_protocol::delimitation::Hypothesis {
                id: "partition".into(),
                name: "Répartition".into(),
                origin: basecraft_protocol::delimitation::HypothesisOrigin::Manual,
                parent: None,
                groups: Vec::new(),
                unassigned: ["s1".into(), "s2".into()].into(),
                scores: Vec::new(),
                provenance: basecraft_protocol::delimitation::Provenance::default(),
                attributes: BTreeMap::new(),
            });
        assert_eq!(doc.metadata.specimens.rows.len(), 2);
        assert_eq!(doc.metadata.taxa.len(), 2);
        assert_eq!(doc.metadata.identifications.len(), 2);
        assert_eq!(doc.metadata.links.len(), 2);
        let bytes = metadata::encode(&doc.metadata).unwrap();
        fs::write(root.join("unrelated"), b"keep sibling").unwrap();
        Self {
            root: root.into(),
            doc,
            target: root.join("output").join(metadata::MANIFEST),
            bytes,
        }
    }
    fn paths(&self) -> Vec<PathBuf> {
        metadata::files(&self.target)
    }
    fn seed(&self) {
        fs::create_dir_all(self.target.parent().unwrap()).unwrap();
        for (i, path) in self.paths().iter().enumerate() {
            fs::write(path, format!("old {i}\n")).unwrap();
        }
    }
    fn block(&self, index: usize) {
        let path = &self.paths()[index];
        fs::rename(path, path.with_extension("held")).unwrap();
        fs::create_dir(path).unwrap();
    }
    fn observe(&self, hooks: Hooks) -> ObservedMetadataWrite {
        write_observed(&self.doc, &self.target, Format::Metadata, hooks)
    }
    fn legacy(&self) -> Result<Vec<(PathBuf, String)>> {
        self.doc.write(
            &self.target,
            Format::Metadata,
            &self.root.join("recovery"),
            &crate::documents::WritePublication::default(),
        )
    }
    fn roster(&self, result: &ObservedMetadataWrite) {
        let expected = result.expected().unwrap();
        assert_eq!(expected.len(), 5);
        assert_eq!(result.files().len(), 5);
        for (i, output) in expected.iter().enumerate() {
            assert_eq!(output.path, self.paths()[i]);
            assert_eq!(output.bytes, self.bytes[i]);
            assert_eq!(result.files()[i].destination(), output.path);
            assert_eq!(result.files()[i].parent_sync(), &ParentSync::NotAttempted);
            if let Some(witness) = result.files()[i].persist().witness() {
                assert_eq!(witness.hash(), basecraft_io::hash_bytes(&output.bytes));
                assert_eq!(witness.size(), output.bytes.len() as u64);
            }
        }
    }
    fn drop_unchanged(&self, result: ObservedMetadataWrite) {
        let before = inventory(&self.root);
        drop(result);
        assert_eq!(inventory(&self.root), before);
    }
    fn changed_inventory(
        &self,
        mut before: Inventory,
        committed: impl Iterator<Item = usize>,
    ) -> Inventory {
        for i in committed {
            before.insert(
                self.paths()[i].strip_prefix(&self.root).unwrap().into(),
                Some(self.bytes[i].clone()),
            );
        }
        before
    }
}
fn committed(result: &ObservedMetadataWrite, index: usize) -> &StagedWitness {
    let PersistFact::Committed { witness } = result.files()[index].persist() else {
        panic!("{result:?}")
    };
    witness
}
fn raw_format(error: &Error) -> &io::Error {
    let Error::Format(basecraft_io::Error::Io(error)) = error else {
        panic!("{error:?}")
    };
    assert!(error.raw_os_error().is_some());
    error
}
fn no_attempts(result: &ObservedMetadataWrite) {
    for fact in result.files() {
        assert!(matches!(
            fact.persist(),
            PersistFact::NotAttempted { staged: None }
        ));
        assert!(matches!(fact.target(), TargetObservation::NotRequested));
    }
}
macro_rules! isolated {
    ($name:ident, $root:ident, $body:block) => {
        #[test]
        fn $name() {
            let Some($root) = crate::isolated_profile::enter(
                concat!(module_path!(), "::", stringify!($name))
                    .trim_start_matches("basecraft_ops::"),
            ) else {
                return;
            };
            $body
        }
    };
}

isolated!(
    complete_roster_precedes_effects_and_success_roundtrips,
    root,
    {
        let f = Fixture::new(&root.join("case"));
        let baseline = inventory(&root);
        let mut final_inventory = baseline.clone();
        final_inventory.insert(PathBuf::from("case/output"), None);
        for (path, bytes) in f.paths().iter().zip(&f.bytes) {
            final_inventory.insert(
                path.strip_prefix(&root).unwrap().into(),
                Some(bytes.clone()),
            );
        }
        let check_root = root.clone();
        let expected_paths = f.paths();
        let expected_bytes = f.bytes.clone();
        let result = f.observe(Hooks {
            prepared: Some(Box::new(move |outputs| {
                assert_eq!(outputs.len(), 5);
                assert!(!expected_paths[0].parent().unwrap().exists());
                assert_eq!(inventory(&check_root), baseline);
                for (i, output) in outputs.iter().enumerate() {
                    assert_eq!(output.path, expected_paths[i]);
                    assert_eq!(output.bytes, expected_bytes[i]);
                }
            })),
            ..Default::default()
        });
        f.roster(&result);
        assert_eq!(result.parent(), &ParentPreparation::CallReturnedOk);
        assert_eq!(result.step(), &MetadataStep::ReturnHashes);
        assert!(result.diagnostics().is_empty());
        for (i, path) in f.paths().iter().enumerate() {
            assert_eq!(fs::read(path).unwrap(), f.bytes[i]);
            assert_eq!(
                committed(&result, i).identity(),
                &object_identity(path, OrdinaryKind::File).unwrap()
            );
            assert!(
                matches!(result.files()[i].target(), TargetObservation::Present { identity, hash, size }
            if identity == committed(&result, i).identity() && hash == committed(&result, i).hash() && *size == f.bytes[i].len() as u64)
            );
        }
        assert_eq!(inventory(&root), final_inventory);
        assert_eq!(f.bytes[0].last(), Some(&b'\n'));
        assert_eq!(metadata::load(&f.target).unwrap().metadata, f.doc.metadata);
        assert_eq!(&f.legacy().unwrap(), result.producer().as_ref().unwrap());
        f.drop_unchanged(result);
    }
);

isolated!(wrong_format_and_parent_failure_preserve_inventory, root, {
    let f = Fixture::new(&root.join("case"));
    let before = inventory(&root);
    let result = write_observed(
        &f.doc,
        &f.target,
        Format::Fasta,
        Hooks {
            prepared: Some(Box::new(|_| panic!("format rejection prepared outputs"))),
            ..Default::default()
        },
    );
    assert!(result.expected().is_none());
    assert!(result.files().is_empty());
    assert_eq!(result.step(), &MetadataStep::ValidateFormat);
    assert_eq!(result.parent(), &ParentPreparation::NotAttempted);
    assert!(
        matches!(result.producer(), Err(Error::Invalid(text)) if text == "project metadata is saved as metadata.basecraft.json")
    );
    assert_eq!(inventory(&root), before);
    fs::write(f.target.parent().unwrap(), b"blocking parent").unwrap();
    let before = inventory(&root);
    let result = f.observe(Hooks::default());
    f.roster(&result);
    no_attempts(&result);
    assert_eq!(
        result.parent(),
        &ParentPreparation::CallEnteredWithoutSuccess
    );
    assert_eq!(result.step(), &MetadataStep::PrepareParent);
    let Err(Error::Io(raw)) = result.producer() else {
        panic!("{result:?}")
    };
    let Error::Io(legacy) = f.legacy().unwrap_err() else {
        panic!()
    };
    assert_eq!(raw.raw_os_error(), legacy.raw_os_error());
    assert!(raw.raw_os_error().is_some());
    assert_eq!(raw.kind(), legacy.kind());
    assert_eq!(inventory(&root), before);
    f.drop_unchanged(result);
});

isolated!(
    every_real_persist_failure_retains_truthful_partial_set,
    root,
    {
        for failed in 0..5 {
            let f = Fixture::new(&root.join(format!("slot-{failed}")));
            f.seed();
            f.block(failed);
            let before = inventory(&f.root);
            let result = f.observe(Hooks::default());
            f.roster(&result);
            assert_eq!(result.step(), &MetadataStep::WriteOutput { index: failed });
            raw_format(result.producer().as_ref().unwrap_err());
            for i in 0..5 {
                match i.cmp(&failed) {
                    std::cmp::Ordering::Greater => {
                        assert_eq!(
                            committed(&result, i).identity(),
                            &object_identity(&f.paths()[i], OrdinaryKind::File).unwrap()
                        );
                    }
                    std::cmp::Ordering::Equal => {
                        assert!(matches!(
                            result.files()[i].persist(),
                            PersistFact::NoCommitWitness { .. }
                        ));
                    }
                    std::cmp::Ordering::Less => {
                        assert!(matches!(
                            result.files()[i].persist(),
                            PersistFact::NotAttempted { staged: None }
                        ));
                        assert!(matches!(
                            result.files()[i].target(),
                            TargetObservation::NotRequested
                        ));
                    }
                }
            }
            // Directory inspection is a separate failure, never the primary persist cause.
            assert_eq!(result.diagnostics().len(), 1);
            assert_eq!(result.diagnostics()[0].index(), failed);
            assert_eq!(
                inventory(&f.root),
                f.changed_inventory(before, failed + 1..5)
            );
            f.drop_unchanged(result);
        }
    }
);

isolated!(staged_and_before_persist_failures_do_not_latch, root, {
    for staged in [true, false] {
        let f = Fixture::new(&root.join(format!("stage-{staged}")));
        f.seed();
        let before = inventory(&f.root);
        let identity = object_identity(&f.paths()[2], OrdinaryKind::File).unwrap();
        let mut hooks = Hooks::default();
        if staged {
            hooks.stage = Some((2, Box::new(|_| Err(io::Error::from_raw_os_error(2)))));
        } else {
            hooks.before = Some((
                2,
                Box::new(|fact| {
                    assert!(fact.persist().witness().is_some());
                    Err(io::Error::from_raw_os_error(2))
                }),
            ));
        }
        let result = f.observe(hooks);
        f.roster(&result);
        assert_eq!(
            raw_format(result.producer().as_ref().unwrap_err()).raw_os_error(),
            Some(2)
        );
        assert!(
            matches!(result.files()[2].persist(), PersistFact::NotAttempted { staged: witness } if witness.is_some() != staged)
        );
        assert_eq!(
            identity,
            object_identity(&f.paths()[2], OrdinaryKind::File).unwrap()
        );
        for i in 3..5 {
            committed(&result, i);
        }
        for i in 0..2 {
            assert!(matches!(
                result.files()[i].persist(),
                PersistFact::NotAttempted { staged: None }
            ));
        }
        assert!(result.diagnostics().is_empty());
        assert_eq!(inventory(&f.root), f.changed_inventory(before, 3..5));
        f.drop_unchanged(result);
    }
});

isolated!(
    inspection_error_is_supplemental_to_success_or_raw_primary,
    root,
    {
        for fail in [false, true] {
            let f = Fixture::new(&root.join(format!("primary-{fail}")));
            f.seed();
            if fail {
                f.block(2);
            }
            let before = inventory(&f.root);
            let result = f.observe(Hooks {
                inspect: Some((4, Box::new(|_| Err(io::Error::from_raw_os_error(2).into())))),
                ..Default::default()
            });
            f.roster(&result);
            if fail {
                raw_format(result.producer().as_ref().unwrap_err());
            } else {
                assert!(result.producer().is_ok());
            }
            committed(&result, 4);
            assert!(matches!(
                result.files()[4].target(),
                TargetObservation::Unavailable
            ));
            let diagnostic = result
                .diagnostics()
                .iter()
                .find(|d| d.index() == 4)
                .unwrap();
            assert_eq!(
                diagnostic.failure().phase(),
                &ObservationPhase::PostWriteInspection
            );
            assert!(
                matches!(diagnostic.failure().cause(), Error::Io(e) if e.raw_os_error() == Some(2))
            );
            assert_eq!(result.diagnostics().len(), if fail { 2 } else { 1 });
            assert_eq!(
                inventory(&f.root),
                f.changed_inventory(before, if fail { 3..5 } else { 0..5 })
            );
            f.drop_unchanged(result);
        }
    }
);

isolated!(
    equal_bytes_replacement_preserves_two_temporal_identities,
    root,
    {
        let f = Fixture::new(&root.join("case"));
        f.seed();
        let before = inventory(&f.root);
        let replacement = f.bytes[4].clone();
        let result = f.observe(Hooks {
            inspect: Some((
                4,
                Box::new(move |path| {
                    fs::hard_link(path, path.with_extension("held"))?;
                    basecraft_io::write_atomic(path, |w| w.write_all(&replacement))?;
                    Ok(())
                }),
            )),
            ..Default::default()
        });
        assert!(result.producer().is_ok());
        f.roster(&result);
        let witness = committed(&result, 4);
        let TargetObservation::Present {
            identity,
            hash,
            size,
        } = result.files()[4].target()
        else {
            panic!()
        };
        assert_ne!(witness.identity(), identity);
        assert_eq!(witness.hash(), hash);
        assert_eq!(witness.size(), *size);
        assert_eq!(
            witness.identity(),
            &object_identity(&f.paths()[4].with_extension("held"), OrdinaryKind::File).unwrap()
        );
        let mut expected = f.changed_inventory(before, 0..5);
        expected.insert(
            PathBuf::from("output/records.held"),
            Some(f.bytes[4].clone()),
        );
        assert_eq!(inventory(&f.root), expected);
        f.drop_unchanged(result);
    }
);

isolated!(legacy_exact_messages_order_and_counters, root, {
    for failed in [None, Some(4), Some(3), Some(2), Some(1), Some(0)] {
        let f = Fixture::new(&root.join(format!("legacy-{failed:?}")));
        f.seed();
        if let Some(i) = failed {
            f.block(i);
        }
        #[cfg(feature = "test-counters")]
        basecraft_io::counters::reset();
        let result = f.legacy();
        #[cfg(feature = "test-counters")]
        {
            let counts = basecraft_io::counters::snapshot();
            assert_eq!(counts.sync_all, failed.map_or(5, |i| 5 - i) as u64);
            assert_eq!(counts.sync_data, 0);
            assert!(counts.reads.is_empty());
            assert_eq!(counts.unresolved_reads, 0);
            assert!(counts.store_inspections.is_empty());
        }
        match failed {
            None => assert_eq!(
                result.unwrap(),
                f.paths()
                    .into_iter()
                    .zip(f.bytes.iter().map(|b| basecraft_io::hash_bytes(b)))
                    .collect::<Vec<_>>()
            ),
            Some(4) => {
                raw_format(&result.unwrap_err());
            }
            Some(i) => {
                let raw = f.observe(Hooks::default());
                let original = raw.producer().as_ref().unwrap_err();
                let replaced = f.paths()[i + 1..]
                    .iter()
                    .rev()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let expected = format!(
                    "partial metadata save: replaced {replaced}; failed at {}: {original}. Metadata generation is incomplete. Keep the open document and retry saving after fixing the destination, or save to a new folder. Temporary recovery, when enabled, is at {}",
                    f.paths()[i].display(),
                    f.root.join("recovery").display()
                );
                assert!(matches!(result, Err(Error::Invalid(text)) if text == expected));
            }
        }
    }
});
