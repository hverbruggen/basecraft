//! Round-trips against the generated test data. Skips when it hasn't been
//! generated; run `just testdata` first.

use std::path::PathBuf;

use basecraft_io::{Source, fasta::FastaIndex, genbank, gff3};

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("BASECRAFT_TESTDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/basecraft-testdata"))
        })?;
    let d = base.join("small");
    d.join("plastid.gb").exists().then_some(d)
}

#[test]
fn genbank_and_gff3_agree() {
    let Some(d) = dir() else { return };
    for name in ["plastid", "genome"] {
        let gb = genbank::parse(&std::fs::read(d.join(format!("{name}.gb"))).unwrap()).unwrap();
        let gff = gff3::parse(&std::fs::read(d.join(format!("{name}.gff3"))).unwrap()).unwrap();
        let rec = &gb[0];
        let seq = &gff.sequences[0];
        assert_eq!(seq.circular, rec.circular, "{name}");

        // Every GenBank CDS location matches the GFF3 CDS of the same gene.
        let gb_cds: Vec<_> = rec.features.iter().filter(|f| f.kind == "CDS").collect();
        let gff_cds: Vec<_> = seq.features.iter().filter(|f| f.kind == "CDS").collect();
        assert_eq!(gb_cds.len(), gff_cds.len(), "{name}");
        for (a, b) in gb_cds.iter().zip(&gff_cds) {
            assert_eq!(a.location, b.location, "{name} {:?}", a.qualifier("gene"));
        }

        // FASTA and GenBank carry the same sequence.
        let src = Source::open(&d.join(format!("{name}.fasta"))).unwrap();
        let idx = FastaIndex::build(src.bytes()).unwrap();
        let r = &idx.records[0];
        assert_eq!(
            r.read(src.bytes(), 0, r.len).unwrap(),
            rec.sequence,
            "{name}"
        );
    }
}

#[test]
fn write_then_reparse_is_identical() {
    let Some(d) = dir() else { return };
    let gb = genbank::parse(&std::fs::read(d.join("plastid.gb")).unwrap()).unwrap();
    let mut out = Vec::new();
    genbank::write(&mut out, &gb[0]).unwrap();
    assert_eq!(genbank::parse(&out).unwrap(), gb);

    let bytes = std::fs::read(d.join("genome.gff3")).unwrap();
    let gff = gff3::parse(&bytes).unwrap();
    let len = gff.regions[0].end;
    let mut out = Vec::new();
    gff3::write(&mut out, &[(&gff.sequences[0], len)]).unwrap();
    assert_eq!(gff3::parse(&out).unwrap().sequences, gff.sequences);
}

#[test]
fn ragged_fasta_reads_like_a_naive_parse() {
    let Some(d) = dir() else { return };
    let bytes = std::fs::read(d.join("ragged.fasta")).unwrap();
    let idx = FastaIndex::build(&bytes).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for (rec, chunk) in idx.records.iter().zip(text.split('>').skip(1)) {
        let naive: String = chunk.lines().skip(1).collect();
        assert_eq!(rec.read(&bytes, 0, rec.len).unwrap(), naive.as_bytes());
        assert_eq!(
            rec.read(&bytes, 37, 211).unwrap(),
            &naive.as_bytes()[37..211]
        );
    }
}
