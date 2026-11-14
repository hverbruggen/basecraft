//! Requested heap of the streaming FASTA classifier. The counting allocator
//! covers the whole process, so this suite is its own binary.
#![cfg(feature = "test-heap")]
#[path = "support/indexed_kind.rs"]
mod indexed_kind;
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::files;
use indexed_kind::indexed_kind;

#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

#[test]
fn bounded_classifier_heap() {
    use std::io::Write;
    crate::isolated_profile::isolate!();

    // Instrumentation is process-wide: isolate from the other test threads.
    if std::env::var_os("BASECRAFT_CLASSIFIER_HEAP_CHILD").is_none() {
        let output = crate::isolated_profile::relaunch(&crate::isolated_profile::test_name())
            .env("BASECRAFT_CLASSIFIER_HEAP_CHILD", "1")
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&output.stdout));
        crate::isolated_profile::assert_one_passed(&crate::isolated_profile::test_name(), &output);
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for shape in ["records", "header", "sequence"] {
        for count in [100, 100_000] {
            let path = dir.path().join("rows.fasta");
            let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            match shape {
                "header" => file.write_all(b">").unwrap(),
                "sequence" => file.write_all(b">a\n").unwrap(),
                _ => {}
            }
            for _ in 0..count {
                file.write_all(if shape == "records" {
                    b">a description\nACGT\nA\nACGTAC\n"
                } else {
                    b"ACGTACGTACGTACGT"
                })
                .unwrap();
            }
            if shape != "records" {
                file.write_all(b"\n>b\nT\n").unwrap();
            }
            file.flush().unwrap();
            drop(file);
            #[cfg(feature = "test-counters")]
            basecraft_io::counters::reset();
            let measurement = basecraft_io::heap::Measurement::start().unwrap();
            let kind = files::kind(&path).unwrap();
            let new = measurement.finish().unwrap();
            #[cfg(feature = "test-counters")]
            {
                let counts = basecraft_io::counters::snapshot();
                assert_eq!(counts.unresolved_reads, 0);
                assert_eq!(
                    counts.reads[&path.canonicalize().unwrap()],
                    basecraft_io::counters::Reads {
                        full_reads: 1,
                        bytes: path.metadata().unwrap().len(),
                    }
                );
            }
            assert_eq!(indexed_kind(&path).unwrap(), kind);
            println!(
                "shape={shape} count={count} file_bytes={} stream_requested_peak={}",
                path.metadata().unwrap().len(),
                new.incremental_peak_bytes
            );
            assert!(new.incremental_peak_bytes < 32 * 1024);
        }
    }
}
