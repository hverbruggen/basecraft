//! Synthetic annotated genomes, written as FASTA + GFF3 and as GenBank.

use std::io::{self, Write};

use crate::fasta::write_wrapped;
use crate::rng::Rng;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Strand {
    Plus,
    Minus,
}

impl Strand {
    fn gff(self) -> char {
        match self {
            Strand::Plus => '+',
            Strand::Minus => '-',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Protein,
    Trna,
    Rrna,
}

/// A gene with one transcript. Coordinates are 1-based and inclusive, in
/// genomic order. On a circular genome, a feature crossing the origin has
/// `end > length`, as in GFF3.
pub struct Gene {
    pub name: String,
    pub strand: Strand,
    pub kind: Kind,
    pub exons: Vec<(u64, u64)>,
    /// `(start, end, phase)`, empty for RNA genes.
    pub cds: Vec<(u64, u64, u8)>,
    pub product: Option<String>,
}

impl Gene {
    fn start(&self) -> u64 {
        self.exons[0].0
    }

    fn end(&self) -> u64 {
        self.exons[self.exons.len() - 1].1
    }
}

pub struct Genome {
    pub id: String,
    pub circular: bool,
    pub seq: Vec<u8>,
    pub genes: Vec<Gene>,
}

pub struct GenomeSpec {
    pub id: &'static str,
    pub length: u64,
    pub genes: u64,
    pub circular: bool,
    pub gc: f64,
}

pub fn generate(spec: &GenomeSpec, rng: &mut Rng) -> Genome {
    let mut seq_rng = rng.fork(1);
    #[allow(clippy::cast_possible_truncation)]
    let seq = (0..spec.length).map(|_| seq_rng.base(spec.gc)).collect();

    let mut gene_rng = rng.fork(2);
    let slot = spec.length / spec.genes;
    let mut genes = Vec::new();
    for i in 0..spec.genes {
        let slot_start = i * slot + 1;
        let kind = if gene_rng.chance(0.08) {
            Kind::Trna
        } else {
            Kind::Protein
        };
        let len = match kind {
            Kind::Trna | Kind::Rrna => gene_rng.range(70, 90),
            Kind::Protein => gene_rng.range(slot / 3, slot * 3 / 4),
        };
        let start = slot_start + gene_rng.below(slot - len);
        let strand = if gene_rng.chance(0.5) {
            Strand::Plus
        } else {
            Strand::Minus
        };
        genes.push(make_gene(
            format!("gene{:05}", i + 1),
            kind,
            strand,
            start,
            start + len - 1,
            &mut gene_rng,
        ));
    }

    if spec.circular {
        // One gene across the origin, to exercise wrap-around handling.
        let start = spec.length - 299;
        genes.push(make_gene(
            "orisp".to_string(),
            Kind::Protein,
            Strand::Plus,
            start,
            start + 899,
            &mut gene_rng,
        ));
        // Drop genes that would overlap it at either end of the sequence.
        let (head, tail) = (900 - 299, spec.length - 299);
        genes.retain(|g| g.name == "orisp" || (g.start() > head && g.end() < tail));
        genes.sort_by_key(Gene::start);
    }

    Genome {
        id: spec.id.to_string(),
        circular: spec.circular,
        seq,
        genes,
    }
}

fn make_gene(
    name: String,
    kind: Kind,
    strand: Strand,
    start: u64,
    end: u64,
    rng: &mut Rng,
) -> Gene {
    let len = end - start + 1;
    let max_exons = (len / 300).clamp(1, 6);
    let n_exons = match kind {
        Kind::Trna | Kind::Rrna => 1,
        Kind::Protein => rng.range(1, max_exons),
    };

    // Split the gene into 2n-1 roughly equal segments, alternating exon and
    // intron, with jittered boundaries.
    let segments = 2 * n_exons - 1;
    let seg = len / segments;
    let mut bounds = vec![start];
    for i in 1..segments {
        let jitter = rng.below(seg / 2 + 1);
        bounds.push(start + i * seg - seg / 4 + jitter);
    }
    bounds.push(end + 1);
    #[allow(clippy::cast_possible_truncation)]
    let exons: Vec<(u64, u64)> = (0..n_exons as usize)
        .map(|i| (bounds[2 * i], bounds[2 * i + 1] - 1))
        .collect();

    let cds = match kind {
        Kind::Trna | Kind::Rrna => Vec::new(),
        Kind::Protein => make_cds(&exons, strand, rng),
    };
    Gene {
        name,
        strand,
        kind,
        exons,
        cds,
        product: None,
    }
}

/// CDS segments inside the exons: trim UTRs at both ends, make the total
/// length a multiple of 3, then assign GFF3 phases in transcription order.
fn make_cds(exons: &[(u64, u64)], strand: Strand, rng: &mut Rng) -> Vec<(u64, u64, u8)> {
    let mut parts: Vec<(u64, u64)> = exons.to_vec();
    let first = parts[0];
    let last = parts[parts.len() - 1];
    let trim_left = rng.below(((first.1 - first.0) / 3).min(60) + 1);
    let trim_right = rng.below(((last.1 - last.0) / 3).min(60) + 1);
    parts[0].0 += trim_left;
    let n = parts.len();
    parts[n - 1].1 -= trim_right;

    let total: u64 = parts.iter().map(|(s, e)| e - s + 1).sum();
    let excess = total % 3;
    // Remove the excess from the 3' end in transcription order.
    match strand {
        Strand::Plus => parts[n - 1].1 -= excess,
        Strand::Minus => parts[0].0 += excess,
    }

    let order: Vec<usize> = match strand {
        Strand::Plus => (0..n).collect(),
        Strand::Minus => (0..n).rev().collect(),
    };
    let mut phases = vec![0u8; n];
    let mut done = 0u64;
    for i in order {
        #[allow(clippy::cast_possible_truncation)]
        let phase = ((3 - done % 3) % 3) as u8;
        phases[i] = phase;
        done += parts[i].1 - parts[i].0 + 1;
    }
    parts
        .into_iter()
        .zip(phases)
        .map(|((s, e), p)| (s, e, p))
        .collect()
}

pub fn write_fasta(g: &Genome, w: &mut impl Write) -> io::Result<()> {
    writeln!(w, ">{}", g.id)?;
    write_wrapped(w, &g.seq, 60)
}

pub fn write_gff3(g: &Genome, w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "##gff-version 3")?;
    writeln!(w, "##sequence-region {} 1 {}", g.id, g.seq.len())?;
    if g.circular {
        writeln!(
            w,
            "{}\tbasecraft-testgen\tregion\t1\t{}\t.\t+\t.\tID={};Is_circular=true",
            g.id,
            g.seq.len(),
            g.id
        )?;
    }
    for gene in &g.genes {
        let id = &g.id;
        let s = gene.strand.gff();
        let name = &gene.name;
        let (start, end) = (gene.start(), gene.end());
        writeln!(
            w,
            "{id}\tbasecraft-testgen\tgene\t{start}\t{end}\t.\t{s}\t.\tID={name};Name={name}"
        )?;
        let (tx_type, tx_id) = match gene.kind {
            Kind::Protein => ("mRNA", format!("{name}.t1")),
            Kind::Trna => ("tRNA", format!("{name}.trna")),
            Kind::Rrna => ("rRNA", format!("{name}.rrna")),
        };
        writeln!(
            w,
            "{id}\tbasecraft-testgen\t{tx_type}\t{start}\t{end}\t.\t{s}\t.\tID={tx_id};Parent={name}"
        )?;
        for (i, (es, ee)) in gene.exons.iter().enumerate() {
            writeln!(
                w,
                "{id}\tbasecraft-testgen\texon\t{es}\t{ee}\t.\t{s}\t.\tID={tx_id}.exon{};Parent={tx_id}",
                i + 1
            )?;
        }
        for (cs, ce, phase) in &gene.cds {
            writeln!(
                w,
                "{id}\tbasecraft-testgen\tCDS\t{cs}\t{ce}\t.\t{s}\t{phase}\tID={tx_id}.cds;Parent={tx_id}"
            )?;
        }
    }
    Ok(())
}

/// Segments in transcription order: genomic order on the plus strand,
/// reversed on the minus strand.
fn transcription_order(segs: &[(u64, u64)], strand: Strand) -> Vec<(u64, u64)> {
    let mut v = segs.to_vec();
    if strand == Strand::Minus {
        v.reverse();
    }
    v
}

/// Plastid genes: name, kind, and number of introns, following the usual
/// land-plant plastome gene set and its intron-containing genes.
const PLASTID_GENES: &[(&str, Kind, usize)] = &[
    ("psbA", Kind::Protein, 0),
    ("trnK-UUU", Kind::Trna, 1),
    ("matK", Kind::Protein, 0),
    ("rps16", Kind::Protein, 1),
    ("trnQ-UUG", Kind::Trna, 0),
    ("psbK", Kind::Protein, 0),
    ("psbI", Kind::Protein, 0),
    ("trnS-GCU", Kind::Trna, 0),
    ("trnG-UCC", Kind::Trna, 1),
    ("trnR-UCU", Kind::Trna, 0),
    ("atpA", Kind::Protein, 0),
    ("atpF", Kind::Protein, 1),
    ("atpH", Kind::Protein, 0),
    ("atpI", Kind::Protein, 0),
    ("rps2", Kind::Protein, 0),
    ("rpoC2", Kind::Protein, 0),
    ("rpoC1", Kind::Protein, 1),
    ("rpoB", Kind::Protein, 0),
    ("trnC-GCA", Kind::Trna, 0),
    ("petN", Kind::Protein, 0),
    ("psbM", Kind::Protein, 0),
    ("trnD-GUC", Kind::Trna, 0),
    ("trnY-GUA", Kind::Trna, 0),
    ("trnE-UUC", Kind::Trna, 0),
    ("trnT-GGU", Kind::Trna, 0),
    ("psbD", Kind::Protein, 0),
    ("psbC", Kind::Protein, 0),
    ("trnS-UGA", Kind::Trna, 0),
    ("psbZ", Kind::Protein, 0),
    ("trnG-GCC", Kind::Trna, 0),
    ("trnfM-CAU", Kind::Trna, 0),
    ("rps14", Kind::Protein, 0),
    ("psaB", Kind::Protein, 0),
    ("psaA", Kind::Protein, 0),
    ("ycf3", Kind::Protein, 2),
    ("trnS-GGA", Kind::Trna, 0),
    ("rps4", Kind::Protein, 0),
    ("trnT-UGU", Kind::Trna, 0),
    ("trnL-UAA", Kind::Trna, 1),
    ("trnF-GAA", Kind::Trna, 0),
    ("ndhJ", Kind::Protein, 0),
    ("ndhK", Kind::Protein, 0),
    ("ndhC", Kind::Protein, 0),
    ("trnV-UAC", Kind::Trna, 1),
    ("trnM-CAU", Kind::Trna, 0),
    ("atpE", Kind::Protein, 0),
    ("atpB", Kind::Protein, 0),
    ("rbcL", Kind::Protein, 0),
    ("accD", Kind::Protein, 0),
    ("psaI", Kind::Protein, 0),
    ("ycf4", Kind::Protein, 0),
    ("cemA", Kind::Protein, 0),
    ("petA", Kind::Protein, 0),
    ("psbJ", Kind::Protein, 0),
    ("psbL", Kind::Protein, 0),
    ("psbF", Kind::Protein, 0),
    ("psbE", Kind::Protein, 0),
    ("petL", Kind::Protein, 0),
    ("petG", Kind::Protein, 0),
    ("trnW-CCA", Kind::Trna, 0),
    ("trnP-UGG", Kind::Trna, 0),
    ("psaJ", Kind::Protein, 0),
    ("rpl33", Kind::Protein, 0),
    ("rps18", Kind::Protein, 0),
    ("rpl20", Kind::Protein, 0),
    ("clpP", Kind::Protein, 2),
    ("psbB", Kind::Protein, 0),
    ("psbT", Kind::Protein, 0),
    ("psbN", Kind::Protein, 0),
    ("psbH", Kind::Protein, 0),
    ("petB", Kind::Protein, 1),
    ("petD", Kind::Protein, 1),
    ("rpoA", Kind::Protein, 0),
    ("rps11", Kind::Protein, 0),
    ("rpl36", Kind::Protein, 0),
    ("infA", Kind::Protein, 0),
    ("rps8", Kind::Protein, 0),
    ("rpl14", Kind::Protein, 0),
    ("rpl16", Kind::Protein, 1),
    ("rps3", Kind::Protein, 0),
    ("rpl22", Kind::Protein, 0),
    ("rps19", Kind::Protein, 0),
    ("rpl23", Kind::Protein, 0),
    ("trnI-CAU", Kind::Trna, 0),
    ("ycf2", Kind::Protein, 0),
    ("trnL-CAA", Kind::Trna, 0),
    ("ndhB", Kind::Protein, 1),
    ("rps7", Kind::Protein, 0),
    ("trnV-GAC", Kind::Trna, 0),
    ("rrn16", Kind::Rrna, 0),
    ("trnI-GAU", Kind::Trna, 1),
    ("trnA-UGC", Kind::Trna, 1),
    ("rrn23", Kind::Rrna, 0),
    ("rrn4.5", Kind::Rrna, 0),
    ("rrn5", Kind::Rrna, 0),
    ("trnR-ACG", Kind::Trna, 0),
    ("trnN-GUU", Kind::Trna, 0),
    ("ycf1", Kind::Protein, 0),
    ("ndhF", Kind::Protein, 0),
    ("rpl32", Kind::Protein, 0),
    ("trnL-UAG", Kind::Trna, 0),
    ("ccsA", Kind::Protein, 0),
    ("ndhD", Kind::Protein, 0),
    ("psaC", Kind::Protein, 0),
    ("ndhE", Kind::Protein, 0),
    ("ndhG", Kind::Protein, 0),
    ("ndhI", Kind::Protein, 0),
    ("ndhA", Kind::Protein, 1),
    ("ndhH", Kind::Protein, 0),
    ("rps15", Kind::Protein, 0),
    ("trnH-GUG", Kind::Trna, 0),
    // Last: placed across the origin, to exercise wrap-around with introns.
    ("rpl2", Kind::Protein, 1),
];

fn plastid_product(name: &str, kind: Kind) -> String {
    match kind {
        Kind::Trna => {
            let aa = name
                .trim_start_matches("trn")
                .split('-')
                .next()
                .unwrap_or("");
            let three = match aa {
                "A" => "Ala",
                "C" => "Cys",
                "D" => "Asp",
                "E" => "Glu",
                "F" => "Phe",
                "G" => "Gly",
                "H" => "His",
                "I" => "Ile",
                "K" => "Lys",
                "L" => "Leu",
                "M" | "fM" => "Met",
                "N" => "Asn",
                "P" => "Pro",
                "Q" => "Gln",
                "R" => "Arg",
                "S" => "Ser",
                "T" => "Thr",
                "V" => "Val",
                "W" => "Trp",
                "Y" => "Tyr",
                _ => "Xaa",
            };
            format!("tRNA-{three}")
        }
        Kind::Rrna => format!(
            "{} ribosomal RNA",
            name.trim_start_matches("rrn").to_string() + "S"
        ),
        Kind::Protein => {
            let family = [
                ("psb", "photosystem II protein"),
                ("psa", "photosystem I protein"),
                ("atp", "ATP synthase subunit"),
                ("ndh", "NADH dehydrogenase subunit"),
                ("pet", "cytochrome b6/f complex subunit"),
                ("rpl", "ribosomal protein L"),
                ("rps", "ribosomal protein S"),
                ("rpo", "RNA polymerase subunit"),
            ];
            match name {
                "rbcL" => "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit".into(),
                "matK" => "maturase K".into(),
                "clpP" => "ATP-dependent Clp protease proteolytic subunit".into(),
                "accD" => "acetyl-CoA carboxylase carboxyltransferase beta subunit".into(),
                _ => family
                    .iter()
                    .find(|(p, _)| name.starts_with(p))
                    .map_or_else(
                        || format!("hypothetical protein {name}"),
                        |(p, d)| format!("{d} {}", &name[p.len()..]),
                    ),
            }
        }
    }
}

/// Exon lengths in transcription order and intron lengths for one gene.
fn plastid_structure(
    name: &str,
    kind: Kind,
    introns: usize,
    rng: &mut Rng,
) -> (Vec<u64>, Vec<u64>) {
    let total = match (kind, name) {
        (Kind::Rrna, "rrn16") => 1491,
        (Kind::Rrna, "rrn23") => 2810,
        (Kind::Rrna, "rrn4.5") => 103,
        (Kind::Rrna, _) => 121,
        (Kind::Trna, _) => rng.range(71, 76),
        (Kind::Protein, "rbcL") => 1428,
        (Kind::Protein, "psbA") => 1062,
        (Kind::Protein, "matK") => 1515,
        (Kind::Protein, "ycf2" | "ycf1") => 3 * rng.range(1200, 1800),
        (Kind::Protein, _) => 3 * rng.range(40, 500),
    };
    // Split the mature length into introns + 1 exons, each at least a third
    // of an equal share, in whole codons for proteins.
    let n = introns as u64 + 1;
    let unit = if kind == Kind::Protein { 3 } else { 1 };
    let mut exons = Vec::new();
    let mut left = total;
    for i in 0..n {
        let remaining = n - i;
        let share = left / remaining;
        let len = if remaining == 1 {
            left
        } else {
            let lo = (share / 3).max(unit * 4);
            (rng.range(lo, (share * 5 / 3).min(left - lo * (remaining - 1))) / unit) * unit
        };
        exons.push(len);
        left -= len;
    }
    let intron_range = if kind == Kind::Trna {
        (450, 2500)
    } else {
        (500, 1100)
    };
    let introns = (0..introns)
        .map(|_| rng.range(intron_range.0, intron_range.1))
        .collect();
    (exons, introns)
}

/// A synthetic plastome with real plastid gene names, introns where real
/// plastid genes have them, start and stop codons at CDS ends, and the last
/// gene spanning the origin.
pub fn generate_plastid(id: &str, length: u64, gc: f64, rng: &mut Rng) -> Genome {
    let mut seq_rng = rng.fork(1);
    let mut seq: Vec<u8> = (0..length).map(|_| seq_rng.base(gc)).collect();
    let mut gene_rng = rng.fork(2);

    let layouts: Vec<(Vec<u64>, Vec<u64>)> = PLASTID_GENES
        .iter()
        .map(|&(name, kind, introns)| plastid_structure(name, kind, introns, &mut gene_rng))
        .collect();
    let gene_len =
        |i: usize| -> u64 { layouts[i].0.iter().sum::<u64>() + layouts[i].1.iter().sum::<u64>() };
    let last = PLASTID_GENES.len() - 1;
    let overhang = gene_len(last) / 2;
    let first_start = overhang + 150;
    let region_end = length - (gene_len(last) - overhang) - 100;
    let body: u64 = (0..last).map(gene_len).sum();
    let gap = (region_end - first_start).saturating_sub(body) / last as u64;

    let mut genes = Vec::new();
    let mut pos = first_start;
    for (i, &(name, kind, _)) in PLASTID_GENES.iter().enumerate() {
        let start = if i == last {
            length - overhang + 1
        } else {
            pos
        };
        let strand = if gene_rng.chance(0.5) {
            Strand::Plus
        } else {
            Strand::Minus
        };
        let (exon_lens, intron_lens) = &layouts[i];
        // Genomic order: on the minus strand exon 1 is the rightmost.
        let (mut exon_g, mut intron_g) = (exon_lens.clone(), intron_lens.clone());
        if strand == Strand::Minus {
            exon_g.reverse();
            intron_g.reverse();
        }
        let mut exons = Vec::new();
        let mut p = start;
        for (k, &len) in exon_g.iter().enumerate() {
            exons.push((p, p + len - 1));
            p += len;
            if let Some(&il) = intron_g.get(k) {
                p += il;
            }
        }
        let cds = if kind == Kind::Protein {
            cds_from_exons(&exons, strand)
        } else {
            Vec::new()
        };
        if kind == Kind::Protein {
            write_codons(&mut seq, &exons, strand);
        }
        genes.push(Gene {
            name: name.to_string(),
            strand,
            kind,
            exons,
            cds,
            product: Some(plastid_product(name, kind)),
        });
        // Never more than the even share, so the genes fit before the
        // origin-spanning last one.
        let spacing = gene_rng.range(gap * 6 / 10, gap.max(1));
        pos = p + spacing;
    }
    Genome {
        id: id.to_string(),
        circular: true,
        seq,
        genes,
    }
}

/// The CDS is the exons; phases follow the cumulative length in
/// transcription order.
fn cds_from_exons(exons: &[(u64, u64)], strand: Strand) -> Vec<(u64, u64, u8)> {
    let order: Vec<usize> = match strand {
        Strand::Plus => (0..exons.len()).collect(),
        Strand::Minus => (0..exons.len()).rev().collect(),
    };
    let mut phases = vec![0u8; exons.len()];
    let mut done = 0u64;
    for i in order {
        #[allow(clippy::cast_possible_truncation)]
        let phase = ((3 - done % 3) % 3) as u8;
        phases[i] = phase;
        done += exons[i].1 - exons[i].0 + 1;
    }
    exons
        .iter()
        .zip(phases)
        .map(|(&(s, e), p)| (s, e, p))
        .collect()
}

/// Put ATG at the CDS start and TAA at its end, on the gene's strand.
/// Positions wrap around the origin.
fn write_codons(seq: &mut [u8], exons: &[(u64, u64)], strand: Strand) {
    let len = seq.len() as u64;
    // 1-based genomic positions of the mature transcript, 5' to 3'.
    let mut positions: Vec<u64> = exons.iter().flat_map(|&(s, e)| s..=e).collect();
    if strand == Strand::Minus {
        positions.reverse();
    }
    let n = positions.len();
    let put = |seq: &mut [u8], i: usize, b: u8| {
        #[allow(clippy::cast_possible_truncation)]
        let at = ((positions[i] - 1) % len) as usize;
        seq[at] = if strand == Strand::Minus {
            match b {
                b'A' => b'T',
                b'T' => b'A',
                b'G' => b'C',
                _ => b'G',
            }
        } else {
            b
        };
    };
    for (i, &b) in b"ATG".iter().enumerate() {
        put(seq, i, b);
    }
    for (i, &b) in b"TAA".iter().enumerate() {
        put(seq, n - 3 + i, b);
    }
}

/// GenBank location string, splitting segments that cross the origin.
fn genbank_location(segments: &[(u64, u64)], strand: Strand, length: u64) -> String {
    let mut parts = Vec::new();
    for &(s, e) in segments {
        if s > length {
            parts.push(format!("{}..{}", s - length, e - length));
        } else if e > length {
            parts.push(format!("{s}..{length}"));
            parts.push(format!("1..{}", e - length));
        } else {
            parts.push(format!("{s}..{e}"));
        }
    }
    let loc = if parts.len() == 1 {
        parts.pop().unwrap_or_default()
    } else {
        format!("join({})", parts.join(","))
    };
    match strand {
        Strand::Plus => loc,
        Strand::Minus => format!("complement({loc})"),
    }
}

pub fn write_genbank(g: &Genome, w: &mut impl Write) -> io::Result<()> {
    let len = g.seq.len() as u64;
    let topology = if g.circular { "circular" } else { "linear" };
    writeln!(
        w,
        "LOCUS       {:<16} {len:>11} bp    DNA     {topology:<8} SYN 10-OCT-2026",
        g.id
    )?;
    writeln!(
        w,
        "DEFINITION  Synthetic test genome from basecraft-testgen."
    )?;
    writeln!(w, "ACCESSION   {}", g.id)?;
    writeln!(w, "FEATURES             Location/Qualifiers")?;
    writeln!(w, "     source          1..{len}")?;
    for gene in &g.genes {
        let span = genbank_location(&[(gene.start(), gene.end())], gene.strand, len);
        writeln!(w, "     gene            {span}")?;
        writeln!(w, "                     /gene=\"{}\"", gene.name)?;
        // complement(join(...)) lists segments in genomic order; the
        // complement reverses them into transcription order.
        let exons_loc = genbank_location(&gene.exons, gene.strand, len);
        let (key, loc) = match gene.kind {
            Kind::Protein => {
                let segs: Vec<(u64, u64)> = gene.cds.iter().map(|&(s, e, _)| (s, e)).collect();
                ("CDS", genbank_location(&segs, gene.strand, len))
            }
            Kind::Trna => ("tRNA", exons_loc),
            Kind::Rrna => ("rRNA", exons_loc),
        };
        writeln!(w, "     {key:<16}{loc}")?;
        writeln!(w, "                     /gene=\"{}\"", gene.name)?;
        if gene.kind == Kind::Protein {
            writeln!(w, "                     /codon_start=1")?;
            writeln!(w, "                     /transl_table=11")?;
        }
        let product = gene
            .product
            .clone()
            .unwrap_or_else(|| "hypothetical protein".into());
        writeln!(w, "                     /product=\"{product}\"")?;
        // Spliced genes also get numbered exon and intron features, in
        // transcription order, as NCBI writes them for organelle genomes.
        if gene.exons.len() > 1 {
            let ordered = transcription_order(&gene.exons, gene.strand);
            for (i, &(s, e)) in ordered.iter().enumerate() {
                let loc = genbank_location(&[(s, e)], gene.strand, len);
                writeln!(w, "     exon            {loc}")?;
                writeln!(w, "                     /gene=\"{}\"", gene.name)?;
                writeln!(w, "                     /number={}", i + 1)?;
                if let Some(&(ns, ne)) = ordered.get(i + 1) {
                    let (is, ie) = match gene.strand {
                        Strand::Plus => (e + 1, ns - 1),
                        Strand::Minus => (ne + 1, s - 1),
                    };
                    let loc = genbank_location(&[(is, ie)], gene.strand, len);
                    writeln!(w, "     intron          {loc}")?;
                    writeln!(w, "                     /gene=\"{}\"", gene.name)?;
                    writeln!(w, "                     /number={}", i + 1)?;
                }
            }
        }
    }
    writeln!(w, "ORIGIN")?;
    for (i, line) in g.seq.chunks(60).enumerate() {
        write!(w, "{:>9}", i * 60 + 1)?;
        for block in line.chunks(10) {
            w.write_all(b" ")?;
            w.write_all(&block.to_ascii_lowercase())?;
        }
        w.write_all(b"\n")?;
    }
    writeln!(w, "//")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Genome {
        let spec = GenomeSpec {
            id: "t",
            length: 200_000,
            genes: 60,
            circular: true,
            gc: 0.4,
        };
        generate(&spec, &mut Rng::new(7))
    }

    #[test]
    fn cds_is_codon_aligned_and_inside_exons() {
        for gene in small().genes.iter().filter(|g| g.kind == Kind::Protein) {
            let total: u64 = gene.cds.iter().map(|(s, e, _)| e - s + 1).sum();
            assert_eq!(total % 3, 0, "{}", gene.name);
            for &(cs, ce, _) in &gene.cds {
                assert!(gene.exons.iter().any(|&(es, ee)| es <= cs && ce <= ee));
            }
        }
    }

    #[test]
    fn genes_do_not_overlap() {
        let g = small();
        for pair in g.genes.windows(2) {
            assert!(
                pair[0].end() < pair[1].start(),
                "{} {}",
                pair[0].name,
                pair[1].name
            );
        }
    }

    #[test]
    fn plastid_genes_are_spliced_coding_and_disjoint() {
        let g = generate_plastid("p", 150_000, 0.37, &mut Rng::new(5));
        assert!(
            g.genes.iter().any(|x| x.exons.len() == 3),
            "clpP and ycf3 have two introns"
        );
        for pair in g.genes.windows(2) {
            assert!(
                pair[0].end() < pair[1].start(),
                "{} {}",
                pair[0].name,
                pair[1].name
            );
        }
        let last = g.genes.last().unwrap();
        assert!(last.end() > 150_000, "rpl2 spans the origin");
        assert!(g.genes[0].start() > last.end() - 150_000);
        for gene in g.genes.iter().filter(|x| x.kind == Kind::Protein) {
            let total: u64 = gene.cds.iter().map(|(s, e, _)| e - s + 1).sum();
            assert_eq!(total % 3, 0, "{}", gene.name);
        }
    }

    #[test]
    fn origin_spanning_gene_splits_in_genbank() {
        let loc = genbank_location(&[(199_701, 200_600)], Strand::Plus, 200_000);
        assert_eq!(loc, "join(199701..200000,1..600)");
        let loc = genbank_location(
            &[(199_701, 199_900), (200_101, 200_600)],
            Strand::Plus,
            200_000,
        );
        assert_eq!(loc, "join(199701..199900,101..600)");
    }
}
