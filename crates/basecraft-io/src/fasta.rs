//! FASTA: a generalised `.fai` index, random-access reads, and a writer.
//!
//! Unlike samtools' `.fai`, the index handles records whose lines vary in
//! length. Uniform records (all lines the same width except the last) are
//! stored as two numbers, like `.fai`. Ragged records store every line start.

use std::io::{self, Write};

use memchr::memchr;
use serde::{Deserialize, Serialize};

use crate::error::parse_error;
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// Every line but the last holds `residues` residues and takes `bytes`
    /// bytes including the line terminator.
    Uniform { residues: u64, bytes: u64 },
    /// `(first residue, byte offset)` for every line, in order.
    Ragged(Vec<(u64, u64)>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordIndex {
    /// The header up to the first whitespace.
    pub name: String,
    /// Byte offset of the `>`.
    pub header_offset: u64,
    /// Byte offset of the first residue.
    pub seq_offset: u64,
    /// Number of residues.
    pub len: u64,
    pub layout: Layout,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastaIndex {
    pub records: Vec<RecordIndex>,
}

/// Lines of one record seen so far. Stays two numbers while the record is
/// uniform, and switches to a full line list the first time it isn't.
struct LayoutBuilder {
    seq_offset: Option<u64>,
    residues: u64,
    /// Width and byte length of the first line.
    width: Option<(u64, u64)>,
    /// Number of full-width lines so far, while uniform.
    full_lines: u64,
    /// A line shorter than `width`, or a blank line, was seen while
    /// uniform. Any further non-empty line makes the record ragged.
    short_seen: bool,
    /// Start of that short line; `None` if it was blank.
    short_start: Option<(u64, u64)>,
    ragged: Option<Vec<(u64, u64)>>,
}

impl LayoutBuilder {
    fn new() -> Self {
        Self {
            seq_offset: None,
            residues: 0,
            width: None,
            full_lines: 0,
            short_seen: false,
            short_start: None,
            ragged: None,
        }
    }

    fn line(&mut self, offset: u64, residues: u64, bytes: u64) {
        if residues == 0 {
            if self.width.is_some() && self.ragged.is_none() {
                self.short_seen = true;
            }
            return;
        }
        let start = (self.residues, offset);
        self.residues += residues;
        if let Some(r) = &mut self.ragged {
            r.push(start);
            return;
        }
        let Some((w, b)) = self.width else {
            self.seq_offset = Some(offset);
            self.width = Some((residues, bytes));
            self.full_lines = 1;
            return;
        };
        if self.short_seen || residues > w || (residues == w && bytes != b) {
            self.ragged = Some(self.uniform_starts(w, b, start));
        } else if residues < w {
            self.short_seen = true;
            self.short_start = Some(start);
        } else {
            self.full_lines += 1;
        }
    }

    /// Line starts of the uniform prefix, followed by `current`.
    fn uniform_starts(&self, w: u64, b: u64, current: (u64, u64)) -> Vec<(u64, u64)> {
        let base = self.seq_offset.unwrap_or(0);
        let mut starts: Vec<(u64, u64)> = (0..self.full_lines)
            .map(|k| (k * w, base + k * b))
            .collect();
        if let Some(short) = self.short_start {
            starts.push(short);
        }
        starts.push(current);
        starts
    }

    fn finish(self) -> (u64, Layout) {
        let offset = self.seq_offset.unwrap_or(0);
        let layout = match (self.ragged, self.width) {
            (Some(r), _) => Layout::Ragged(r),
            (None, Some((residues, bytes))) => Layout::Uniform { residues, bytes },
            (None, None) => Layout::Uniform {
                residues: 0,
                bytes: 0,
            },
        };
        (offset, layout)
    }
}

impl FastaIndex {
    /// Scan a FASTA file and index every record.
    pub fn build(data: &[u8]) -> Result<Self> {
        let mut records = Vec::new();
        let mut current: Option<(String, u64, LayoutBuilder)> = None;
        let mut pos = 0usize;
        let mut line_no = 0usize;
        while pos < data.len() {
            line_no += 1;
            let end = memchr(b'\n', &data[pos..]).map_or(data.len(), |i| pos + i);
            let next = (end + 1).min(data.len());
            let mut content_end = end;
            if content_end > pos && data[content_end - 1] == b'\r' {
                content_end -= 1;
            }
            let line = &data[pos..content_end];
            if line.first() == Some(&b'>') {
                if let Some((name, header, b)) = current.take() {
                    records.push(finish_record(name, header, b));
                }
                let name_end = line
                    .iter()
                    .position(u8::is_ascii_whitespace)
                    .unwrap_or(line.len());
                let name = String::from_utf8_lossy(&line[1..name_end]).into_owned();
                current = Some((name, pos as u64, LayoutBuilder::new()));
            } else if let Some((_, _, b)) = &mut current {
                let trimmed = line.iter().rposition(|c| !c.is_ascii_whitespace());
                let residues = trimmed.map_or(0, |i| i + 1);
                if line[..residues].iter().any(u8::is_ascii_whitespace) {
                    return Err(parse_error(
                        "FASTA",
                        line_no,
                        "whitespace inside a sequence line",
                    ));
                }
                b.line(pos as u64, residues as u64, (next - pos) as u64);
            } else if !line.iter().all(u8::is_ascii_whitespace) {
                return Err(parse_error(
                    "FASTA",
                    line_no,
                    "sequence before the first header",
                ));
            }
            pos = next;
        }
        if let Some((name, header, b)) = current {
            records.push(finish_record(name, header, b));
        }
        Ok(Self { records })
    }

    pub fn get(&self, name: &str) -> Option<&RecordIndex> {
        self.records.iter().find(|r| r.name == name)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = INDEX_MAGIC.to_vec();
        out.extend(postcard::to_stdvec(self).unwrap_or_default());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let body = bytes.strip_prefix(INDEX_MAGIC)?;
        postcard::from_bytes(body).ok()
    }
}

/// Magic and format version for serialised indexes. Bump the version when
/// the structure changes; old cache entries then get rebuilt.
const INDEX_MAGIC: &[u8] = b"BCFAI\x01";

fn finish_record(name: String, header_offset: u64, b: LayoutBuilder) -> RecordIndex {
    let len = b.residues;
    let (seq_offset, layout) = b.finish();
    RecordIndex {
        name,
        header_offset,
        seq_offset,
        len,
        layout,
    }
}

impl RecordIndex {
    /// Byte offset of residue `i`, which must be `< len`.
    fn offset_of(&self, i: u64) -> u64 {
        match &self.layout {
            Layout::Uniform { residues, bytes } => {
                self.seq_offset + (i / residues) * bytes + i % residues
            }
            Layout::Ragged(lines) => {
                let line = lines.partition_point(|&(first, _)| first <= i) - 1;
                let (first, offset) = lines[line];
                offset + (i - first)
            }
        }
    }

    /// Copy residues `start..end` into `out`.
    pub fn read_into(&self, data: &[u8], start: u64, end: u64, out: &mut Vec<u8>) -> Result<()> {
        if start > end || end > self.len {
            return Err(Error::OutOfRange {
                name: self.name.clone(),
                start,
                end,
                len: self.len,
            });
        }
        out.reserve(usize::try_from(end - start).unwrap_or(0));
        let mut i = start;
        while i < end {
            let line_left = match &self.layout {
                Layout::Uniform { residues, .. } => residues - i % residues,
                Layout::Ragged(lines) => {
                    let line = lines.partition_point(|&(first, _)| first <= i);
                    let line_end = lines.get(line).map_or(self.len, |&(first, _)| first);
                    line_end - i
                }
            };
            let take = line_left.min(end - i);
            #[allow(clippy::cast_possible_truncation)]
            let off = self.offset_of(i) as usize;
            #[allow(clippy::cast_possible_truncation)]
            out.extend_from_slice(&data[off..off + take as usize]);
            i += take;
        }
        Ok(())
    }

    pub fn read(&self, data: &[u8], start: u64, end: u64) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.read_into(data, start, end, &mut out)?;
        Ok(out)
    }

    /// The full header line without the `>`.
    pub fn header<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        #[allow(clippy::cast_possible_truncation)]
        let start = self.header_offset as usize + 1;
        let end = memchr(b'\n', &data[start..]).map_or(data.len(), |i| start + i);
        let line = &data[start..end];
        line.strip_suffix(b"\r").unwrap_or(line)
    }
}

/// Write one record, wrapped at `width` residues per line.
pub fn write_record(w: &mut dyn Write, header: &[u8], seq: &[u8], width: usize) -> io::Result<()> {
    w.write_all(b">")?;
    w.write_all(header)?;
    w.write_all(b"\n")?;
    for line in seq.chunks(width.max(1)) {
        w.write_all(line)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(idx: &FastaIndex, data: &[u8], name: &str) -> Vec<u8> {
        let r = idx.get(name).unwrap();
        r.read(data, 0, r.len).unwrap()
    }

    #[test]
    fn uniform_records() {
        let data = b">a desc\nACGT\nACGT\nAC\n>b\nGG\n";
        let idx = FastaIndex::build(data).unwrap();
        assert_eq!(idx.records.len(), 2);
        let a = idx.get("a").unwrap();
        assert_eq!(a.len, 10);
        assert_eq!(
            a.layout,
            Layout::Uniform {
                residues: 4,
                bytes: 5
            }
        );
        assert_eq!(a.header(data), b"a desc");
        assert_eq!(read_all(&idx, data, "a"), b"ACGTACGTAC");
        assert_eq!(a.read(data, 3, 9).unwrap(), b"TACGTA");
        assert_eq!(read_all(&idx, data, "b"), b"GG");
    }

    #[test]
    fn ragged_record() {
        let data = b">r\nACG\nTTTTT\nA\nCCCC\n";
        let idx = FastaIndex::build(data).unwrap();
        let r = idx.get("r").unwrap();
        assert!(matches!(r.layout, Layout::Ragged(_)));
        assert_eq!(read_all(&idx, data, "r"), b"ACGTTTTTACCCC");
        assert_eq!(r.read(data, 2, 10).unwrap(), b"GTTTTTAC");
    }

    #[test]
    fn crlf_and_no_trailing_newline() {
        let data = b">x\r\nACGT\r\nAC";
        let idx = FastaIndex::build(data).unwrap();
        let x = idx.get("x").unwrap();
        assert_eq!(
            x.layout,
            Layout::Uniform {
                residues: 4,
                bytes: 6
            }
        );
        assert_eq!(read_all(&idx, data, "x"), b"ACGTAC");
    }

    #[test]
    fn short_line_then_more_is_ragged() {
        let data = b">x\nACGT\nAC\nACGT\n";
        let idx = FastaIndex::build(data).unwrap();
        assert_eq!(read_all(&idx, data, "x"), b"ACGTACACGT");
        assert!(matches!(idx.records[0].layout, Layout::Ragged(_)));
    }

    #[test]
    fn long_uniform_record_turning_ragged_late() {
        // More than 1024 uniform lines, then a ragged tail: the builder must
        // still produce correct offsets after discarding early line starts.
        let mut data = b">x\n".to_vec();
        let mut expect = Vec::new();
        for i in 0..2000 {
            let line: Vec<u8> = (0..10).map(|j| b"ACGT"[(i + j) % 4]).collect();
            expect.extend(&line);
            data.extend(&line);
            data.push(b'\n');
        }
        data.extend(b"GGGGGGGGGGGG\nT\n");
        expect.extend(b"GGGGGGGGGGGGT");
        let idx = FastaIndex::build(&data).unwrap();
        assert_eq!(read_all(&idx, &data, "x"), expect);
        let r = &idx.records[0];
        assert_eq!(
            r.read(&data, 19_995, 20_013).unwrap(),
            &expect[19_995..20_013]
        );
    }

    #[test]
    fn blank_lines() {
        // A blank line between header and sequence, one at the end, and one
        // in the middle of a second record.
        let data = b">a\n\nACGT\nAC\n\n>b\nACGT\n\nACGT\n";
        let idx = FastaIndex::build(data).unwrap();
        assert_eq!(read_all(&idx, data, "a"), b"ACGTAC");
        assert!(matches!(idx.records[0].layout, Layout::Uniform { .. }));
        assert_eq!(read_all(&idx, data, "b"), b"ACGTACGT");
        assert!(matches!(idx.records[1].layout, Layout::Ragged(_)));
    }

    #[test]
    fn roundtrip_serialisation() {
        let data = b">a\nACGT\nAC\n>r\nA\nCCC\nGG\n";
        let idx = FastaIndex::build(data).unwrap();
        assert_eq!(FastaIndex::from_bytes(&idx.to_bytes()), Some(idx));
    }

    #[test]
    fn out_of_range() {
        let data = b">a\nACGT\n";
        let idx = FastaIndex::build(data).unwrap();
        assert!(idx.records[0].read(data, 2, 5).is_err());
    }
}
