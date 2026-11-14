//! What repeated saves write: each history record holds only its own save's
//! events, so the files a save writes follow the edits made since the last
//! save, not every edit ever made (D24).
use basecraft_ops::{Session, project::Project};
use basecraft_protocol::Op;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Every file under `root` with its size and modification time.
fn files(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                folders.push(entry.path());
            } else {
                found.insert(entry.path(), (metadata.len(), metadata.modified().unwrap()));
            }
        }
    }
    found
}

/// The review's save table: 200 kb FASTA in a project, 200 one-base edits
/// between saves, eight saves. Run with --nocapture to see it.
#[test]
fn eight_saves_of_two_hundred_edits() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("gene.fasta");
    let line = [b'A'; 60].iter().chain(b"\n").copied();
    let body: Vec<u8> = std::iter::repeat_n(line, 200_000 / 60).flatten().collect();
    std::fs::write(&file, [b">gene\n".as_slice(), &body].concat()).unwrap();
    let project = Project::new(&folder).unwrap();
    let mut session = Session::recoverable(&folder)
        .unwrap()
        .in_project(Some(project));
    let doc = session.open(&file).unwrap();
    let mut written = Vec::new();
    for save in 0..8u64 {
        for at in save * 200..save * 200 + 200 {
            session
                .apply(doc, Op::seq_replace(at * 7, at * 7 + 1, "C"))
                .unwrap();
        }
        let before = files(dir.path());
        let started = Instant::now();
        session.save(doc, None).unwrap();
        let elapsed = started.elapsed().as_millis();
        // Replaced files count whole; the journal counts what was appended.
        let mut bytes = 0;
        let mut replaced = Vec::new();
        for (path, (size, modified)) in files(dir.path()) {
            match before.get(&path) {
                Some((old, _)) if path.extension().is_some_and(|e| e == "jsonl") => {
                    bytes += size.saturating_sub(*old);
                }
                Some(old) if *old == (size, modified) => {}
                _ => {
                    bytes += size;
                    replaced.push(path);
                }
            }
        }
        let history: u64 = files(&folder.join(".basecraft/provenance"))
            .values()
            .map(|v| v.0)
            .sum();
        eprintln!(
            "save {} | {elapsed} ms | {bytes} bytes written | history {history} | {replaced:?}",
            save + 1
        );
        written.push((bytes, replaced));
    }
    // The eighth save writes the sequence, one record, the pointer to it and
    // the recovery journal's new stream: no copies, no reports, no GFF3.
    let (last, replaced) = &written[7];
    let history: Vec<_> = replaced
        .iter()
        .filter(|p| p.starts_with(folder.join(".basecraft/provenance")))
        .collect();
    assert_eq!(history.len(), 2, "{history:?}");
    assert!(history.iter().any(|p| p.ends_with("manifest.json")));
    // Bytes follow the edits since the last save, not every edit ever made.
    assert!(
        *last < written[1].0 + written[1].0 / 10,
        "{last} vs {}",
        written[1].0
    );
    let bundle = basecraft_ops::records::bundle_for_result(&file, session.project());
    assert_eq!(
        std::fs::read_dir(bundle.join("records")).unwrap().count(),
        8
    );
    let (_, history) = basecraft_ops::records::saved_history(&file, session.project()).unwrap();
    assert_eq!(history.effective.len(), 1600);
    assert!(history.gaps.is_empty());
}

fn edited_and_saved(folder: &Path, saves: u64) -> PathBuf {
    let file = folder.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGTACGT\n").unwrap();
    let mut session = Session::recoverable(folder).unwrap();
    let doc = session.open(&file).unwrap();
    for save in 0..saves {
        session
            .apply(doc, Op::seq_replace(save, save + 1, "T"))
            .unwrap();
        session.save(doc, None).unwrap();
    }
    file
}

#[test]
fn a_reopened_file_shows_its_whole_history_and_a_lost_record_as_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let file = edited_and_saved(dir.path(), 3);
    let bundle = basecraft_ops::records::bundle_for_result(&file, None);
    let latest = basecraft_ops::records::inspect(&file, None).unwrap();
    assert_eq!(latest.effective_edits.len(), 1);
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    let record = session.document_record(doc).unwrap();
    assert_eq!(record.effective.len(), 3);
    assert!(record.warnings.iter().all(|w| !w.contains("incomplete")));
    // Lose the middle record: the newest is still read, the rest is a gap.
    let middle = &latest.parents[0];
    std::fs::remove_file(bundle.join("records").join(format!("{middle}.json"))).unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    let record = session.document_record(doc).unwrap();
    assert_eq!(record.effective.len(), 1);
    assert!(
        record
            .warnings
            .iter()
            .any(|w| w.contains(middle.as_str()) && w.contains("is missing")),
        "{:?}",
        record.warnings
    );
    let report = record.report().unwrap();
    assert_eq!(report.history[0].summary, "Earlier history missing");
    let issues = basecraft_ops::records::verify(&file, None).unwrap();
    assert!(
        issues.iter().any(|issue| issue.contains("is missing")),
        "{issues:?}"
    );
}

#[test]
fn after_a_save_the_journal_holds_only_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGT\n").unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.apply(doc, Op::seq_replace(1, 2, "T")).unwrap();
    session.save(doc, None).unwrap();
    session.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
    let recovery = session.recovery_root().to_path_buf();
    let journal = basecraft_ops::log::read_all(&recovery).unwrap();
    let ops: Vec<_> = journal.iter().map(|e| e.op.name()).collect();
    assert!(matches!(journal[0].op, Op::Save { .. }), "{ops:?}");
    assert_eq!(journal.len(), 2, "{ops:?}");
    // A crash now: recovery starts at the save and replays the later edit.
    std::mem::forget(session);
    let restored =
        basecraft_ops::records::recover(&recovery, &dir.path().join("out"), None).unwrap();
    assert_eq!(std::fs::read(&restored[0]).unwrap(), b">gene\nTTATACGT\n");
}

/// D27: a FASTA gets a GFF3 beside it only when there is something to say,
/// and a GFF3 that exists is written again, even empty, never deleted.
#[test]
fn a_gff3_is_written_only_when_there_are_features_or_one_exists() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    let gff = dir.path().join("gene.gff3");
    std::fs::write(&file, ">gene\nACGTACGTACGT\n").unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.save(doc, None).unwrap();
    assert!(!gff.exists(), "no features, no GFF3");
    let feature = basecraft_protocol::Feature {
        kind: "gene".into(),
        location: basecraft_protocol::Location::simple(1, 6, basecraft_protocol::Strand::Forward),
        qualifiers: Vec::new(),
    };
    let added = session.apply(doc, Op::feature_add(feature)).unwrap();
    session.save(doc, None).unwrap();
    assert!(std::fs::read_to_string(&gff).unwrap().contains("gene"));
    session
        .apply(doc, Op::feature_remove(added.feature.unwrap().0))
        .unwrap();
    session.save(doc, None).unwrap();
    let written = std::fs::read_to_string(&gff).unwrap();
    assert!(!written.contains("\tgene\t"), "{written}");
}

/// D24 for analysis runs: an extraction's record holds the extraction and
/// the input's unsaved edits; the input's saved history stays in its own
/// records, and the result's history reads it through the run's input link.
#[test]
fn a_run_links_to_its_input_history_instead_of_copying_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = edited_and_saved(dir.path(), 2);
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, Op::seq_replace(5, 6, "G")).unwrap();
    let output = dir.path().join("part.gb");
    session
        .extract(doc, (0, 8), &[], false, &output, None)
        .unwrap();
    let record = basecraft_ops::records::inspect(&output, None).unwrap();
    let saved = basecraft_ops::records::saved_history(&file, None)
        .unwrap()
        .1;
    assert!(
        record
            .events
            .iter()
            .all(|event| !saved.events.iter().any(|old| old.id == event.id)),
        "the run copied saved history"
    );
    // The two saved edits, the unsaved one and the extraction.
    let (_, history) = basecraft_ops::records::saved_history(&output, None).unwrap();
    assert_eq!(history.effective.len(), 4, "{:?}", history.effective);
    assert!(history.gaps.is_empty());
    assert_eq!(
        basecraft_ops::records::verify(&output, None).unwrap(),
        Vec::<String>::new()
    );
}
