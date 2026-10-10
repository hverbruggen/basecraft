//! Opening and saving documents by file format.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use basecraft_docs::SequenceDoc;
use basecraft_io::{cache, fasta, genbank, gff3};
use basecraft_protocol::{Feature, Location, Qualifier, Segment};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    GenBank,
    /// FASTA, with annotations in a GFF3 sidecar of the same stem.
    Fasta,
}

impl Format {
    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "gb" | "gbk" | "genbank" | "gbff" => Ok(Format::GenBank),
            "fa" | "fasta" | "fna" | "fas" | "ffn" => Ok(Format::Fasta),
            _ => Err(Error::UnknownFormat(path.to_path_buf())),
        }
    }
}

pub fn gff_sidecar(path: &Path) -> PathBuf {
    path.with_extension("gff3")
}

/// An open document plus what's needed to write its file back.
pub struct Opened {
    pub doc: SequenceDoc,
    pub format: Format,
    /// For FASTA: the original file, so records other than the open one are
    /// copied through on save.
    pub fasta: Option<Arc<cache::IndexedFasta>>,
}

/// Open the first record of a sequence file.
pub fn open(path: &Path) -> Result<Opened> {
    let format = Format::from_path(path)?;
    match format {
        Format::GenBank => {
            let bytes = std::fs::read(path)?;
            let rec = genbank::parse(&bytes)?
                .into_iter()
                .next()
                .ok_or_else(|| Error::Empty(path.to_path_buf()))?;
            Ok(Opened {
                doc: SequenceDoc::from_genbank(rec),
                format,
                fasta: None,
            })
        }
        Format::Fasta => {
            let file = Arc::new(cache::open_fasta(path)?);
            if file.index.records.is_empty() {
                return Err(Error::Empty(path.to_path_buf()));
            }
            let sidecar = gff_sidecar(path);
            let gff = if sidecar.exists() {
                Some(gff3::parse(&std::fs::read(&sidecar)?)?)
            } else {
                None
            };
            let name = file.index.records[0].name.clone();
            let features = gff.and_then(|g| g.sequences.into_iter().find(|s| s.seqid == name));
            let doc = SequenceDoc::from_fasta(Arc::clone(&file), 0, features);
            Ok(Opened {
                doc,
                format,
                fasta: Some(file),
            })
        }
    }
}

/// Serialise the main file of a document.
pub fn write_main(opened: &Opened, w: &mut dyn Write) -> io::Result<()> {
    let doc = &opened.doc;
    match opened.format {
        Format::GenBank => genbank::write(w, &doc.to_genbank()),
        Format::Fasta => {
            let Some(file) = &opened.fasta else {
                return write_fasta_record(doc, None, w);
            };
            for (i, rec) in file.index.records.iter().enumerate() {
                if i == 0 {
                    write_fasta_record(doc, Some(rec.header(file.bytes())), w)?;
                } else {
                    let seq = rec
                        .read(file.bytes(), 0, rec.len)
                        .map_err(io::Error::other)?;
                    fasta::write_record(w, rec.header(file.bytes()), &seq, 60)?;
                }
            }
            Ok(())
        }
    }
}

fn write_fasta_record(
    doc: &SequenceDoc,
    header: Option<&[u8]>,
    w: &mut dyn Write,
) -> io::Result<()> {
    let header = header.map_or_else(|| doc.name.as_bytes().to_vec(), <[u8]>::to_vec);
    // Stream in chunks so a huge record never sits in memory twice.
    w.write_all(b">")?;
    w.write_all(&header)?;
    w.write_all(b"\n")?;
    let mut buf = Vec::new();
    let chunk = 60 * 16_384;
    let mut pos = 0;
    while pos < doc.len() {
        let end = (pos + chunk).min(doc.len());
        buf.clear();
        doc.read_into(pos, end, &mut buf);
        for line in buf.chunks(60) {
            w.write_all(line)?;
            w.write_all(b"\n")?;
        }
        pos = end;
    }
    Ok(())
}

/// Serialise the GFF3 sidecar, if the format has one.
pub fn write_sidecar(opened: &Opened, w: &mut dyn Write) -> Option<io::Result<()>> {
    (opened.format == Format::Fasta).then(|| {
        let feats = opened.doc.to_gff();
        gff3::write(w, &[(&feats, opened.doc.len())])
    })
}

/// A writer that hashes what passes through it.
pub struct HashingWriter<W> {
    pub inner: W,
    pub hasher: basecraft_io::Hasher,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: basecraft_io::Hasher::new(),
        }
    }

    pub fn hash(&self) -> String {
        self.hasher.finish()
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A new GenBank record for an extract of `doc`:
/// - `segments` empty: bases `start..end`, with every annotation that
///   overlaps them, clipped and shifted; an annotation reaching past the
///   range is marked incomplete at that end (GenBank `<`/`>`).
/// - `segments` given (5' to 3'): those parts spliced together, carrying
///   the annotations whose parts are exactly these, as one continuous
///   annotation. This is how a spliced CDS is extracted.
///
/// With `reverse`, the record is the reverse complement, so a reverse-
/// strand annotation reads 5' to 3' and lies on the forward strand.
pub fn extract_record(
    doc: &SequenceDoc,
    start: u64,
    end: u64,
    segments: &[(u64, u64)],
    reverse: bool,
) -> genbank::Record {
    let mut rec = if segments.len() > 1 {
        extract_spliced(doc, segments, reverse)
    } else {
        let mut r = extract_range(doc, start, end);
        if reverse {
            reverse_complement_record(&mut r);
        }
        r
    };
    rec.name = if reverse && segments.len() <= 1 {
        format!("{}_rc", rec.name)
    } else {
        rec.name
    };
    rec
}

fn complement(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'T' | b'U' => b'A',
        b'C' => b'G',
        b'G' => b'C',
        b'a' => b't',
        b't' | b'u' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'R' => b'Y',
        b'Y' => b'R',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'V' => b'B',
        b'D' => b'H',
        b'H' => b'D',
        other => other,
    }
}

/// Reverse-complement a record's sequence and mirror its annotations onto
/// the other strand. Segment order (5' to 3') carries over unchanged.
fn reverse_complement_record(rec: &mut genbank::Record) {
    let n = rec.sequence.len() as u64;
    rec.sequence.reverse();
    for b in &mut rec.sequence {
        *b = complement(*b);
    }
    for f in &mut rec.features {
        if f.kind == "source" {
            f.location = Location::simple(0, n, basecraft_protocol::Strand::Forward);
            continue;
        }
        for s in &mut f.location.segments {
            let (a, b) = (n - s.end, n - s.start);
            s.start = a;
            s.end = b;
            s.strand = match s.strand {
                basecraft_protocol::Strand::Forward => basecraft_protocol::Strand::Reverse,
                basecraft_protocol::Strand::Reverse => basecraft_protocol::Strand::Forward,
                basecraft_protocol::Strand::Unstranded => basecraft_protocol::Strand::Unstranded,
            };
            std::mem::swap(&mut s.partial_start, &mut s.partial_end);
        }
    }
    rec.header = vec![format!(
        "{} Reverse complement.",
        rec.header.join(" ").trim_end()
    )];
}

fn extract_spliced(doc: &SequenceDoc, segments: &[(u64, u64)], reverse: bool) -> genbank::Record {
    let mut sequence = Vec::new();
    for &(a, b) in segments {
        let mut part = doc.read(a, b);
        if reverse {
            part.reverse();
            for x in &mut part {
                *x = complement(*x);
            }
        }
        sequence.extend(part);
    }
    let n = sequence.len() as u64;
    let lo = segments.iter().map(|s| s.0).min().unwrap_or(0);
    let hi = segments.iter().map(|s| s.1).max().unwrap_or(0);
    // The annotations made of exactly these parts (a CDS, and perhaps its
    // mRNA): one continuous annotation over the spliced sequence.
    let mut features = vec![Feature {
        kind: "source".into(),
        location: Location::simple(0, n, basecraft_protocol::Strand::Forward),
        qualifiers: vec![Qualifier::new(
            "note",
            format!(
                "{} spliced parts of {} between bases {}..{}",
                segments.len(),
                doc.name,
                lo + 1,
                hi
            ),
        )],
    }];
    let mut label = None;
    for id in doc.features_in(lo, hi) {
        let Some(f) = doc.feature(id) else { continue };
        let same: Vec<(u64, u64)> = f
            .location
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        if same != segments {
            continue;
        }
        label = label.or_else(|| f.label().map(str::to_string));
        let mut seg = Segment::new(0, n, basecraft_protocol::Strand::Forward);
        seg.partial_start = f.location.partial_5();
        seg.partial_end = f.location.partial_3();
        features.push(Feature {
            kind: f.kind.clone(),
            location: Location {
                segments: vec![seg],
                unordered: false,
            },
            qualifiers: f.qualifiers.clone(),
        });
    }
    let meta = doc.genbank.clone().unwrap_or_default();
    let name = match &label {
        Some(l) => format!("{}_{l}", doc.name),
        None => format!("{}_{}-{}_spliced", doc.name, lo + 1, hi),
    };
    genbank::Record {
        name,
        molecule: if meta.molecule.is_empty() {
            "DNA".into()
        } else {
            meta.molecule
        },
        circular: false,
        division: if meta.division.is_empty() {
            "UNK".into()
        } else {
            meta.division
        },
        date: if meta.date.is_empty() {
            "01-JAN-1970".into()
        } else {
            meta.date
        },
        header: vec![format!(
            "DEFINITION  {}{} parts of {} spliced together, extracted with Basecraft.",
            label.map(|l| format!("{l}: ")).unwrap_or_default(),
            segments.len(),
            doc.name
        )],
        features,
        sequence,
    }
}

/// Bases `start..end` with their annotations, clipped and shifted.
fn extract_range(doc: &SequenceDoc, start: u64, end: u64) -> genbank::Record {
    let name = format!("{}_{}-{}", doc.name, start + 1, end);
    let mut features = Vec::new();
    for id in doc.features_in(start, end) {
        let Some(f) = doc.feature(id) else { continue };
        let mut segments = Vec::new();
        for s in &f.location.segments {
            let (a, b) = (s.start.max(start), s.end.min(end));
            let inside = if s.is_empty() {
                s.start >= start && s.start <= end
            } else {
                b > a
            };
            if !inside {
                continue;
            }
            segments.push(Segment {
                start: a - start,
                end: b.max(a) - start,
                strand: s.strand,
                partial_start: s.partial_start || s.start < start,
                partial_end: s.partial_end || s.end > end,
            });
        }
        if segments.is_empty() {
            continue;
        }
        // Parts dropped entirely count too: an annotation reaching past the
        // range is incomplete at that end.
        if let Some((lo, hi)) = f.location.bounds() {
            if lo < start
                && let Some(s) = segments.iter_mut().min_by_key(|s| s.start)
            {
                s.partial_start = true;
            }
            if hi > end
                && let Some(s) = segments.iter_mut().max_by_key(|s| s.end)
            {
                s.partial_end = true;
            }
        }
        features.push(Feature {
            kind: f.kind.clone(),
            location: Location {
                segments,
                unordered: f.location.unordered,
            },
            qualifiers: f.qualifiers.clone(),
        });
    }
    features.insert(
        0,
        Feature {
            kind: "source".into(),
            location: Location::simple(0, end - start, basecraft_protocol::Strand::Forward),
            qualifiers: vec![Qualifier::new(
                "note",
                format!("bases {}..{} of {}", start + 1, end, doc.name),
            )],
        },
    );
    // A source feature from the original no longer describes the extract.
    features.retain(|f| {
        !(f.kind == "source" && f.qualifier("note").is_none_or(|n| !n.starts_with("bases ")))
    });
    let meta = doc.genbank.clone().unwrap_or_default();
    genbank::Record {
        name,
        molecule: if meta.molecule.is_empty() {
            "DNA".into()
        } else {
            meta.molecule
        },
        circular: false,
        division: if meta.division.is_empty() {
            "UNK".into()
        } else {
            meta.division
        },
        date: if meta.date.is_empty() {
            "01-JAN-1970".into()
        } else {
            meta.date
        },
        header: vec![format!(
            "DEFINITION  Bases {}..{} of {}, extracted with Basecraft.",
            start + 1,
            end,
            doc.name
        )],
        features,
        sequence: doc.read(start, end),
    }
}
