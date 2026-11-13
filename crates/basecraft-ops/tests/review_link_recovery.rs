//! Independent correction review: recovery evidence must be usable, not merely present.
#![cfg(unix)]
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::AlignmentLinkExt;

use basecraft_ops::{Session, files, records, records::AlignmentLink};
use serde_json::Value;
use std::path::Path;

fn alignment(path: &Path, bases: &str) {
    std::fs::write(path, format!(">a\n{bases}\n>b\n{bases}\n")).unwrap();
}

#[test]
fn guarded_partial_save_supports_retry_and_manual_intended_recovery() {
    crate::isolated_profile::isolate!();
    // No association, inference only, and inference plus an explicit view.
    for mode in 0..3 {
        for manual in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let source = root.join("source.afa");
            let view = root.join("view.afa");
            let old = root.join("old.afa");
            alignment(&source, "AA");
            alignment(&view, "CC");
            alignment(&old, "GG");
            let input = root.join("input.nwk");
            let output = root.join("output.nwk");
            std::fs::write(&input, "(a:1,b:2);").unwrap();
            std::fs::write(&output, "(a:3,b:4);").unwrap();
            let mut session = Session::new(root);
            let doc = session.open(&input).unwrap();
            if mode > 0 {
                AlignmentLink::create(&input, &source, &source, 7, true, None).unwrap();
            }
            if mode == 2 {
                AlignmentLink::associate(&input, session.tree(doc).unwrap(), &view).unwrap();
            }
            AlignmentLink::create(&output, &old, &old, 2, true, None).unwrap();
            AlignmentLink::associate(&output, session.tree(doc).unwrap(), &old).unwrap();
            let old_link = AlignmentLink::read_view(&output).unwrap();
            let old_snapshot = old_link
                .checked_snapshot(&output, session.tree(doc).unwrap())
                .unwrap();
            let old_bytes = std::fs::read(&old_snapshot).unwrap();
            let mut referents = Vec::new();
            for metadata in [records::sidecar(&output), records::view_sidecar(&output)] {
                let referent = metadata.with_extension("previous.json");
                std::fs::rename(&metadata, &referent).unwrap();
                std::os::unix::fs::symlink(referent.file_name().unwrap(), &metadata).unwrap();
                referents.push((referent.clone(), std::fs::read(referent).unwrap()));
            }
            // Retained snapshots must suffice even after every original source disappears.
            for path in [&source, &view, &old] {
                std::fs::remove_file(path).unwrap();
            }
            let presentation = files::companion_path(None, &output);
            std::fs::create_dir_all(&presentation).unwrap();
            let error = session.save(doc, Some(&output)).unwrap_err().to_string();
            assert!(error.contains("recovery metadata at"), "{error}");
            assert!(AlignmentLink::read(&output).is_err());
            assert!(AlignmentLink::read_view(&output).is_err());
            assert_eq!(std::fs::read(&old_snapshot).unwrap(), old_bytes);
            let recovery_path = std::fs::read_dir(records::sidecar(&output).parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.to_string_lossy().ends_with(".alignment-recovery.json"))
                .unwrap();
            let recovery: Value =
                serde_json::from_slice(&std::fs::read(recovery_path).unwrap()).unwrap();
            for (key, (referent, bytes)) in ["previous_origin", "previous_view"]
                .into_iter()
                .zip(&referents)
            {
                assert_eq!(recovery[key]["entry"], "symlink");
                assert_eq!(
                    recovery[key]["value"].as_str().unwrap(),
                    referent.file_name().unwrap().to_str().unwrap()
                );
                assert_eq!(std::fs::read(referent).unwrap(), *bytes);
            }
            std::fs::remove_dir(&presentation).unwrap();
            if manual {
                // Restore the intended presentation and association state for the new main file.
                // Restoring previous links alone would be wrong after main-file replacement.
                let mut bytes = Vec::new();
                let hash = basecraft_io::hash_file(&output).unwrap();
                files::tree_companion(session.tree(doc).unwrap(), &hash)
                    .write(&mut bytes)
                    .unwrap();
                std::fs::write(&presentation, bytes).unwrap();
                for (key, path) in [
                    ("intended_origin", records::sidecar(&output)),
                    ("intended_view", records::view_sidecar(&output)),
                ] {
                    if recovery[key].is_null() {
                        std::fs::remove_file(path).unwrap();
                    } else {
                        let bytes: Vec<u8> = serde_json::from_value(recovery[key].clone()).unwrap();
                        std::fs::write(path, bytes).unwrap();
                    }
                }
            } else {
                session.save(doc, Some(&output)).unwrap();
                let replay = basecraft_ops::records::replay(root).unwrap();
                assert!(replay.ok(), "{:?}", replay.problems);
            }
            let reopened = files::load_tree(&output).unwrap().doc;
            assert_eq!(reopened.to_newick(), session.tree(doc).unwrap().to_newick());
            assert_eq!(std::fs::read(&old_snapshot).unwrap(), old_bytes);
            if mode == 0 {
                assert!(!records::metadata_present(&records::sidecar(&output)).unwrap());
                assert!(!records::metadata_present(&records::view_sidecar(&output)).unwrap());
            } else {
                let link = AlignmentLink::read_view(&output).unwrap();
                assert_eq!(link.source, if mode == 2 { view } else { source });
                assert!(link.source_changed());
                let retained = link.checked_snapshot(&output, &reopened).unwrap();
                let expected = if mode == 2 { "CC" } else { "AA" };
                assert_eq!(
                    std::fs::read_to_string(retained).unwrap(),
                    format!(">a\n{expected}\n>b\n{expected}\n")
                );
            }
        }
    }
}
