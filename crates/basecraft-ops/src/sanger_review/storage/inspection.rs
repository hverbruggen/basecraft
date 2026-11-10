//! Current-input diagnostics only, deliberately compiled through tests until a
//! caller is separately allocated. No future Source, run, receipt or permission
//! is produced. Reads are not a pinned snapshot and cannot admit/refuse a save.
use super::{CandidateProbe, SangerReviewSession, Source, probe_candidate};
use basecraft_docs::sanger::InputRead;
use basecraft_platform::paths::{AppPaths, PathError};
use basecraft_protocol::provenance::Publication;
use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

struct Inspection<'a> {
    root: &'a SangerReviewSession,
    target: &'a Path,
    sets: Vec<SetInspection<'a>>,
    // Exact current PathBuf sort/dedup, not canonical-identity/hash dedup.
    dependencies: Vec<Dependency>,
    recovery: Result<RecoveryRole<'a>, PathError>,
    previous_visible: io::Result<fs::Metadata>,
}
struct SetInspection<'a> {
    // Traversal position (selected first), never a stored identity.
    position: usize,
    set: &'a SangerReviewSession,
    inputs: Vec<InputInspection<'a>>,
    sources: Vec<SourceInspection<'a>>,
    runs: Vec<RunInspection<'a>>,
    placement_context: Option<Result<Option<PathBuf>, String>>,
    roles: Vec<PathRole<'a>>,
}
struct InputInspection<'a> {
    input: &'a InputRead,
    selection: Selection,
}
enum Selection {
    // Index into this set's actual source list, including first-match semantics.
    Existing(usize),
    Unmatched {
        probe: crate::Result<CandidateProbe>,
        provenance: ProspectiveProvenance,
    },
}
#[derive(Debug, PartialEq, Eq)]
enum ProspectiveProvenance {
    // bundle_for_result's PathBuf API masks resolver failures. Do not invoke it
    // to certify prospective ancestry, or mistake uninspected for none.
    UninspectedLegacyResolver,
}
#[derive(Debug, PartialEq, Eq)]
enum SourceRequirement {
    ExistingEntry,
    ReferenceIfUnchanged,
    UnavailableUnderLegacyFallback,
    CandidateUnresolved,
}
impl InputInspection<'_> {
    // Conditional role only, never a prediction that registration will succeed.
    fn requirement(&self) -> SourceRequirement {
        match &self.selection {
            Selection::Existing(_) => SourceRequirement::ExistingEntry,
            Selection::Unmatched { probe: Err(_), .. } => SourceRequirement::CandidateUnresolved,
            Selection::Unmatched {
                probe: Ok(probe), ..
            } => {
                if probe
                    .sha256
                    .as_ref()
                    .is_ok_and(|hash| hash.eq_ignore_ascii_case(&self.input.sha256))
                {
                    SourceRequirement::ReferenceIfUnchanged
                } else {
                    SourceRequirement::UnavailableUnderLegacyFallback
                }
            }
        }
    }
}

struct SourceInspection<'a> {
    source: &'a Source,
    checked: crate::Result<PathBuf>,
}
struct RunInspection<'a> {
    run: &'a Path,
    methods: MethodsInspection,
}
struct MethodsInspection {
    path: PathBuf,
    followed: io::Result<fs::Metadata>,
    leaf: io::Result<fs::Metadata>,
    legacy_selected: bool,
    open: OpenObservation,
}
enum OpenObservation {
    NotRequested,
    Symlink,
    UnavailableLeaf,
    Attempted(io::Result<()>),
}
struct Dependency {
    path: PathBuf,
    // One direct manifest read per distinct current path. Source::checked may
    // independently read declared ancestry; no second ancestry walk is added.
    manifest: Result<Result<Publication, serde_json::Error>, crate::Error>,
}
enum RecoveryRole<'a> {
    Existing {
        review: &'a Path,
        manifest: PathBuf,
        anchor: &'a Path,
    },
    NeedsJob {
        jobs_root: PathBuf,
        anchor: &'a Path,
    },
}
#[derive(Debug, PartialEq, Eq)]
enum Role {
    Draft,
    OriginalInput,
    SourcePath,
    SourceOriginal,
    Ledger,
    PreservedInput,
    Run,
    Methods,
    Placement,
    PlacementContext,
    CheckpointReview,
    CheckpointManifest,
}
struct PathRole<'a> {
    kind: Role,
    path: Cow<'a, Path>,
}

fn inspect<'a>(root: &'a SangerReviewSession, target: &'a Path) -> Inspection<'a> {
    inspect_with_manifest_reader(root, target, crate::provenance::read_record)
}

// Narrow observation seam: tests count calls while delegating to the real reader.
fn inspect_with_manifest_reader<'a>(
    root: &'a SangerReviewSession,
    target: &'a Path,
    mut read_manifest: impl FnMut(&Path) -> crate::Result<Vec<u8>>,
) -> Inspection<'a> {
    let mut sets = Vec::new();
    inspect_set(root, target, &mut sets);
    let paths: BTreeSet<_> = sets
        .iter()
        .flat_map(|s| {
            s.set.evidence.runs.iter().chain(
                s.set
                    .evidence
                    .sources
                    .iter()
                    .filter_map(|s| s.ledger.as_ref()),
            )
        })
        .cloned()
        .collect();
    let dependencies = paths
        .into_iter()
        .map(|path| {
            let manifest = read_manifest(&path.join("manifest.json"))
                .map(|bytes| serde_json::from_slice(&bytes));
            Dependency { path, manifest }
        })
        .collect();
    let recovery = match root.recovery.as_deref() {
        Some(review) => Ok(RecoveryRole::Existing {
            review,
            manifest: review.with_file_name("manifest.json"),
            anchor: &root.anchor,
        }),
        None => AppPaths::discover().map(|paths| RecoveryRole::NeedsJob {
            jobs_root: paths.state.join("jobs"),
            anchor: &root.anchor,
        }),
    };
    Inspection {
        root,
        target,
        sets,
        dependencies,
        recovery,
        previous_visible: fs::metadata(target),
    }
}

fn inspect_set<'a>(set: &'a SangerReviewSession, target: &Path, sets: &mut Vec<SetInspection<'a>>) {
    let position = sets.len();
    let inputs = set
        .draft
        .inputs
        .iter()
        .map(|input| {
            let selection = match set.evidence.sources.iter().position(|s| {
                s.original == input.path && s.sha256.eq_ignore_ascii_case(&input.sha256)
            }) {
                Some(index) => Selection::Existing(index),
                None => Selection::Unmatched {
                    probe: probe_candidate(&input.path, target),
                    provenance: ProspectiveProvenance::UninspectedLegacyResolver,
                },
            };
            InputInspection { input, selection }
        })
        .collect();
    let sources = set
        .evidence
        .sources
        .iter()
        .map(|source| SourceInspection {
            source,
            checked: source.checked(),
        })
        .collect();
    let mut selected = false;
    let runs = set
        .evidence
        .runs
        .iter()
        .map(|run| {
            let path = run.join("reports/methods.md");
            let followed = fs::metadata(&path);
            let leaf = fs::symlink_metadata(&path);
            let legacy_selected =
                position == 0 && !selected && followed.as_ref().is_ok_and(fs::Metadata::is_file);
            selected |= legacy_selected;
            let open = if !legacy_selected {
                OpenObservation::NotRequested
            } else {
                match &leaf {
                    Ok(metadata) if metadata.file_type().is_symlink() => OpenObservation::Symlink,
                    Ok(metadata) if metadata.is_file() => {
                        OpenObservation::Attempted(fs::File::open(&path).map(|_| ()))
                    }
                    _ => OpenObservation::UnavailableLeaf,
                }
            };
            RunInspection {
                run,
                methods: MethodsInspection {
                    path,
                    followed,
                    leaf,
                    legacy_selected,
                    open,
                },
            }
        })
        .collect();
    // This is the existing read/validation owner and its existing String error.
    // The working placement copy is dropped; it is not a future save payload.
    let placement_context = set.evidence.placement.as_ref().map(|_| {
        set.placement()
            .map(|placement| placement.map(|p| p.source_path))
    });
    let mut roles = vec![PathRole {
        kind: Role::Draft,
        path: Cow::Borrowed(&set.draft_path),
    }];
    for input in &set.draft.inputs {
        roles.push(PathRole {
            kind: Role::OriginalInput,
            path: Cow::Borrowed(&input.path),
        });
    }
    for source in &set.evidence.sources {
        roles.push(PathRole {
            kind: Role::SourcePath,
            path: Cow::Borrowed(&source.path),
        });
        roles.push(PathRole {
            kind: Role::SourceOriginal,
            path: Cow::Borrowed(&source.original),
        });
        if let Some(ledger) = &source.ledger {
            roles.push(PathRole {
                kind: Role::Ledger,
                path: Cow::Borrowed(ledger),
            });
            roles.push(PathRole {
                kind: Role::PreservedInput,
                path: Cow::Owned(ledger.join("preserved-input")),
            });
        }
    }
    for run in &set.evidence.runs {
        roles.push(PathRole {
            kind: Role::Run,
            path: Cow::Borrowed(run),
        });
        roles.push(PathRole {
            kind: Role::Methods,
            path: Cow::Owned(run.join("reports/methods.md")),
        });
    }
    if let Some(placement) = &set.evidence.placement {
        roles.push(PathRole {
            kind: Role::Placement,
            path: Cow::Borrowed(&placement.path),
        });
    }
    if let Some(Ok(Some(context))) = &placement_context {
        roles.push(PathRole {
            kind: Role::PlacementContext,
            path: Cow::Owned(context.clone()),
        });
    }
    if let Some(review) = &set.recovery {
        roles.push(PathRole {
            kind: Role::CheckpointReview,
            path: Cow::Borrowed(review),
        });
        roles.push(PathRole {
            kind: Role::CheckpointManifest,
            path: Cow::Owned(review.with_file_name("manifest.json")),
        });
    }
    sets.push(SetInspection {
        position,
        set,
        inputs,
        sources,
        runs,
        placement_context,
        roles,
    });
    for child in &set.retained {
        inspect_set(child, target, sets);
    }
}

#[cfg(test)]
mod tests;
