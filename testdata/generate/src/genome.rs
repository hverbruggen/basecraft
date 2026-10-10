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
}

/// A gene with one transcript. Coordinates are 1-based and inclusive, in
/// genomic order. On a circular genome, a feature crossing the origin has
/// `end > length`, as in GFF3.
pub struct Gene {
    pub name: String,
    pub strand: Strand,
    pub kind: Kind,
    pub exons: Vec<(u64, u64)>,
    /// `(start, end, phase)`, empty for tRNAs.
    pub cds: Vec<(u64, u64, u8)>,
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
            Kind::Trna => gene_rng.range(70, 90),
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
        Kind::Trna => 1,
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
        Kind::Trna => Vec::new(),
        Kind::Protein => make_cds(&exons, strand, rng),
    };
    Gene {
        name,
        strand,
        kind,
        exons,
        cds,
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
        match gene.kind {
            Kind::Protein => {
                let segs: Vec<(u64, u64)> = gene.cds.iter().map(|&(s, e, _)| (s, e)).collect();
                let loc = genbank_location(&segs, gene.strand, len);
                writeln!(w, "     CDS             {loc}")?;
                writeln!(w, "                     /gene=\"{}\"", gene.name)?;
                writeln!(w, "                     /codon_start=1")?;
                writeln!(w, "                     /product=\"hypothetical protein\"")?;
            }
            Kind::Trna => {
                writeln!(w, "     tRNA            {span}")?;
                writeln!(w, "                     /gene=\"{}\"", gene.name)?;
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
