//! Refuse saves that would silently discard records or their annotations.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{Session, files::Kind};
use basecraft_protocol::Op;

#[test]
fn multi_record_genbank_and_annotated_fasta_refuse_save_and_save_as() {
    let Some(root) =
        isolated_profile::enter("multi_record_genbank_and_annotated_fasta_refuse_save_and_save_as")
    else {
        return;
    };
    for extension in ["gb", "fa", "afa"] {
        let path = root.join(format!("records.{extension}"));
        let original = if extension == "gb" {
            let record =
                include_str!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb");
            format!("{record}\n{record}")
        } else {
            ">first\nACGT\n>second\nTTTT\n".into()
        };
        std::fs::write(&path, &original).unwrap();
        let gff = basecraft_ops::files::gff_sidecar(&path);
        let annotations = b"##gff-version 3\nfirst\t.\tgene\t1\t4\t.\t+\t.\tID=one\nsecond\t.\tgene\t1\t4\t.\t+\t.\tID=two\n";
        std::fs::write(&gff, annotations).unwrap();
        let mut session = Session::recoverable(&root).unwrap();
        let (doc, kind) = session.open_classified(&path).unwrap();
        let new = root.join(format!("copy.{extension}"));
        let existing = root.join(format!("existing.{extension}"));
        std::fs::write(&existing, "keep this destination").unwrap();
        for edited in [false, true] {
            if edited {
                let op = if kind == Kind::Alignment {
                    Op::AlnReplace {
                        row: 0,
                        start: 0,
                        end: 1,
                        residues: "G".into(),
                    }
                } else {
                    Op::SeqReplace {
                        start: 0,
                        end: 1,
                        residues: "G".into(),
                    }
                };
                session.apply(doc, op).unwrap();
            }
            for target in [None, Some(new.as_path()), Some(existing.as_path())] {
                let error = session.save(doc, target).unwrap_err();
                assert!(matches!(error, basecraft_ops::Error::Unsupported(_)));
                let message = error.to_string();
                assert!(message.contains("other records would be lost"), "{message}");
                if extension != "gb" {
                    assert!(message.contains("GFF3"));
                }
                assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
                assert_eq!(std::fs::read(&gff).unwrap(), annotations);
                assert_eq!(
                    std::fs::read_to_string(&existing).unwrap(),
                    "keep this destination"
                );
                assert!(!new.exists());
                assert!(!session.recovery_root().join("pending-save.json").exists());
            }
        }
        session.flush().unwrap();
        let recovery = root.join(format!("recovered-{extension}"));
        let error = basecraft_ops::replay::recover(session.recovery_root(), &recovery).unwrap_err();
        assert!(
            error.to_string().contains("other records would be lost"),
            "{error}"
        );
        assert!(!recovery.exists());
        assert!(session.recovery_root().exists());
    }
}
