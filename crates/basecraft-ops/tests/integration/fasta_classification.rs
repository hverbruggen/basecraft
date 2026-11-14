use std::path::Path;

use crate::indexed_kind::indexed_kind;
use basecraft_io::fasta;
use basecraft_ops::files::{self, Kind, LONGEST_ALIGNMENT_ROW};

fn compare(path: &Path) {
    let old = indexed_kind(path);
    let new = files::kind(path);
    match (old, new) {
        (Ok(old), Ok(new)) => assert_eq!(old, new, "{}", path.display()),
        (Err(old), Err(new)) => assert_eq!(old.to_string(), new.to_string(), "{}", path.display()),
        (old, new) => panic!("{}: old={old:?}, new={new:?}", path.display()),
    }
}

#[test]
fn all_fasta_fixtures_agree_with_index() {
    fn visit(dir: &Path, count: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, count);
            } else if matches!(files::Format::from_path(&path), Ok(files::Format::Fasta)) {
                compare(&path);
                *count += 1;
            }
        }
    }
    crate::isolated_profile::isolate!();
    let mut count = 0;
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures"),
        &mut count,
    );
    assert!(count >= 23, "fixture discovery unexpectedly empty: {count}");
    println!("compared {count} FASTA fixtures");
}

#[test]
fn generated_classification_and_errors() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let mut cases = vec![
        (Vec::new(), Some(Kind::Sequence)),
        (b" \t\r\n".to_vec(), Some(Kind::Sequence)),
        (b">\n".to_vec(), Some(Kind::Sequence)),
        (b">a\n>b".to_vec(), Some(Kind::Alignment)),
        (
            b">a description\r\nAC  \r\n\r\n>b\r\nGG\t".to_vec(),
            Some(Kind::Alignment),
        ),
        (
            b">a\n;comment\n>b\n#comment\n".to_vec(),
            Some(Kind::Alignment),
        ),
        (b";comment\n>a\nAC\n".to_vec(), None),
        (b">a\nAC\n>b\nGG\n>c\nA C".to_vec(), None),
        (b">a\nAC\n>b\nGG\n>c\n A".to_vec(), None),
        (b">a\nAC\rGT".to_vec(), None),
        (b">a\n\xff\x00\n>b\nT".to_vec(), Some(Kind::Alignment)),
    ];
    for length in [
        LONGEST_ALIGNMENT_ROW - 1,
        LONGEST_ALIGNMENT_ROW,
        LONGEST_ALIGNMENT_ROW + 1,
    ] {
        let mut data = b">a\nA\n>b\nT\n>later\n".to_vec();
        data.extend(std::iter::repeat_n(b'A', usize::try_from(length).unwrap()));
        cases.push((
            data,
            Some(if length > LONGEST_ALIGNMENT_ROW {
                Kind::Sequence
            } else {
                Kind::Alignment
            }),
        ));
    }
    // Headers and ragged line counts must not determine auxiliary memory use.
    let mut data = vec![b'>'; 200_000];
    data.extend_from_slice(b" description\nA\n>b\nT\n");
    for _ in 0..10_000 {
        data.extend_from_slice(b">c\nAA\nA\nAAA\n");
    }
    cases.push((data, Some(Kind::Alignment)));
    for (data, expected) in cases {
        for name in ["sample.fasta", "sample.AFA", "sample.ALN.FNA"] {
            let path = dir.path().join(name);
            std::fs::write(&path, &data).unwrap();
            for sidecar in [false, true] {
                let gff = files::gff_sidecar(&path);
                if sidecar {
                    std::fs::write(&gff, b"##gff-version 3\n").unwrap();
                }
                compare(&path);
                match expected {
                    None => assert!(files::kind(&path).is_err()),
                    Some(kind) => assert_eq!(
                        files::kind(&path).unwrap(),
                        if name != "sample.fasta" {
                            Kind::Alignment
                        } else if sidecar {
                            Kind::Sequence
                        } else {
                            kind
                        }
                    ),
                }
                if sidecar {
                    std::fs::remove_file(gff).unwrap();
                }
            }
        }
    }
    for path in [
        dir.path().join("missing.fasta"),
        dir.path().join("directory.fasta"),
    ] {
        if path.file_stem().unwrap() == "directory" {
            std::fs::create_dir(&path).unwrap();
        }
        assert!(matches!(
            files::kind(&path),
            Err(basecraft_ops::Error::Format(basecraft_io::Error::Io(_)))
        ));
    }
}

#[test]
fn summary_facts_match_index() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rows.fa");
    let data = b">a\nAA\nA\nAAA\n>b\nT\n>empty";
    std::fs::write(&path, data).unwrap();
    assert_eq!(
        fasta::summary(&path).unwrap(),
        fasta::Summary {
            records: 3,
            longest: 6
        }
    );
    #[cfg(feature = "test-counters")]
    {
        basecraft_io::counters::reset();
        files::kind(&path).unwrap();
        let counts = basecraft_io::counters::snapshot();
        assert_eq!(counts.unresolved_reads, 0);
        assert_eq!(
            counts.reads[&path.canonicalize().unwrap()],
            basecraft_io::counters::Reads {
                full_reads: 1,
                bytes: data.len() as u64
            }
        );
    }
}
