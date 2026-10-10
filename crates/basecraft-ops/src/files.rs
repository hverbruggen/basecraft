//! Opening and saving documents by file format.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use basecraft_docs::SequenceDoc;
use basecraft_io::{cache, fasta, genbank, gff3};

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
