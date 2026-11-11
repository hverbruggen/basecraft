//! GFF3 reader and writer.
//!
//! Lines that share an `ID` and type become one feature with several
//! segments, as GFF3 intends for CDS and similar features. GFF3 columns
//! with no place in [`Feature`] are kept as qualifiers with a `gff:` prefix:
//! `gff:source`, `gff:score` and `gff:phase` (the phase of the first
//! segment in biological order; later phases follow from segment lengths).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};

use basecraft_protocol::{Feature, Location, Qualifier, Segment, Strand};

use crate::Result;
use crate::error::parse_error;

pub const SOURCE: &str = "gff:source";
pub const SCORE: &str = "gff:score";
pub const PHASE: &str = "gff:phase";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub seqid: String,
    /// 0-based, half-open.
    pub start: u64,
    pub end: u64,
}

/// Features of one sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeqFeatures {
    pub seqid: String,
    pub circular: bool,
    pub features: Vec<Feature>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gff3 {
    /// `##sequence-region` directives.
    pub regions: Vec<Region>,
    /// Other directives and comments before the first feature, kept as-is.
    pub header: Vec<String>,
    /// Per sequence, in order of first appearance.
    pub sequences: Vec<SeqFeatures>,
    /// Byte offset of the sequence data after `##FASTA`, if present.
    pub fasta_offset: Option<usize>,
}

impl Gff3 {
    pub fn get(&self, seqid: &str) -> Option<&SeqFeatures> {
        self.sequences.iter().find(|s| s.seqid == seqid)
    }
}

pub fn parse(data: &[u8]) -> Result<Gff3> {
    let mut out = Gff3::default();
    let mut seq_index: HashMap<String, usize> = HashMap::new();
    // (sequence, ID, type) -> feature index, for merging multi-line features.
    // Keys borrow the raw columns, so merging costs no allocations.
    let mut by_id: HashMap<(usize, &str, &str), usize> = HashMap::new();
    let mut seen_feature = false;
    let mut pos = 0;
    let mut line_no = 0;
    while pos < data.len() {
        line_no += 1;
        let end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |i| pos + i);
        let raw = &data[pos..end];
        pos = end + 1;
        let line = std::str::from_utf8(raw.strip_suffix(b"\r").unwrap_or(raw))
            .map_err(|_| parse_error("GFF3", line_no, "invalid UTF-8"))?;
        if line.is_empty() {
            continue;
        }
        if line.starts_with("##FASTA") {
            out.fasta_offset = Some(pos.min(data.len()));
            break;
        }
        if let Some(rest) = line.strip_prefix("##sequence-region") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let [seqid, start, end] = parts[..] {
                let start: u64 = start
                    .parse()
                    .map_err(|_| parse_error("GFF3", line_no, "bad sequence-region start"))?;
                let end: u64 = end
                    .parse()
                    .map_err(|_| parse_error("GFF3", line_no, "bad sequence-region end"))?;
                out.regions.push(Region {
                    seqid: seqid.to_string(),
                    start: start.saturating_sub(1),
                    end,
                });
            }
            continue;
        }
        if line.starts_with('#') {
            if line == "###" {
                continue;
            }
            if !seen_feature && !line.starts_with("##gff-version") {
                out.header.push(line.to_string());
            }
            continue;
        }
        seen_feature = true;
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 9 {
            return Err(parse_error(
                "GFF3",
                line_no,
                format!("expected 9 columns, found {}", cols.len()),
            ));
        }
        let num = |s: &str, what: &str| -> Result<u64> {
            s.parse()
                .map_err(|_| parse_error("GFF3", line_no, format!("bad {what} {s:?}")))
        };
        let start = num(cols[3], "start")?;
        let end = num(cols[4], "end")?;
        if start == 0 || end < start {
            return Err(parse_error(
                "GFF3",
                line_no,
                "start must be >= 1 and <= end",
            ));
        }
        let strand = match cols[6] {
            "+" => Strand::Forward,
            "-" => Strand::Reverse,
            _ => Strand::Unstranded,
        };
        let seqid = decode(cols[0]);
        let si = *seq_index.entry(seqid.clone()).or_insert_with(|| {
            out.sequences.push(SeqFeatures {
                seqid,
                ..SeqFeatures::default()
            });
            out.sequences.len() - 1
        });
        let kind = decode(cols[2]);
        let attrs = parse_attributes(cols[8]);
        if kind == "region"
            && attrs
                .iter()
                .any(|q| q.key == "Is_circular" && q.value.as_deref() == Some("true"))
        {
            out.sequences[si].circular = true;
        }
        let segment = Segment::new(start - 1, end, strand);
        let raw_id = raw_attribute(cols[8], "ID");
        if let Some(id) = raw_id
            && let Some(&fi) = by_id.get(&(si, id, cols[2]))
        {
            let f = &mut out.sequences[si].features[fi];
            f.location.segments.push(segment);
            if cols[7] != "." {
                // Keep the phase of the segment that ends up first.
                set_phase_candidate(f, start - 1, cols[7]);
            }
            continue;
        }
        let mut qualifiers = Vec::new();
        if cols[1] != "." {
            qualifiers.push(Qualifier::new(SOURCE, decode(cols[1])));
        }
        if cols[5] != "." {
            qualifiers.push(Qualifier::new(SCORE, cols[5]));
        }
        if cols[7] != "." {
            qualifiers.push(Qualifier::new(PHASE, cols[7]));
        }
        qualifiers.extend(attrs);
        let features = &mut out.sequences[si].features;
        if let Some(id) = raw_id {
            by_id.insert((si, id, cols[2]), features.len());
        }
        features.push(Feature {
            kind,
            location: Location {
                segments: vec![segment],
                unordered: false,
            },
            qualifiers,
        });
    }

    for seq in &mut out.sequences {
        seq.features.shrink_to_fit();
        for f in &mut seq.features {
            f.qualifiers.shrink_to_fit();
            f.location.segments.shrink_to_fit();
        }
    }
    let lengths: HashMap<&str, u64> = out
        .regions
        .iter()
        .map(|r| (r.seqid.as_str(), r.end))
        .collect();
    for seq in &mut out.sequences {
        let len = lengths.get(seq.seqid.as_str()).copied();
        for f in &mut seq.features {
            finish_location(f, seq.circular.then_some(len).flatten());
        }
    }
    Ok(out)
}

/// Phase candidates are tracked per segment start until the location is
/// finished; the phase of the biologically first segment wins.
const PHASE_AT: &str = "gff:phase@";

fn set_phase_candidate(f: &mut Feature, start: u64, phase: &str) {
    f.qualifiers
        .push(Qualifier::new(format!("{PHASE_AT}{start}"), phase));
}

/// Sort segments into biological order, split segments that run past the
/// origin of a circular sequence, and settle the phase qualifier.
fn finish_location(f: &mut Feature, circular_len: Option<u64>) {
    // GFF3 marks incomplete ends with start_range=.,N and end_range=N,.
    let partial_low = f.qualifiers.iter().any(|q| q.key == "start_range");
    let partial_high = f.qualifiers.iter().any(|q| q.key == "end_range");
    f.qualifiers
        .retain(|q| q.key != "start_range" && q.key != "end_range");
    if let Some(s) = f.location.segments.iter_mut().min_by_key(|s| s.start) {
        s.partial_start |= partial_low;
    }
    if let Some(s) = f.location.segments.iter_mut().max_by_key(|s| s.end) {
        s.partial_end |= partial_high;
    }
    let first_start = f.location.segments[0].start;
    let reverse = f.location.strand() == Strand::Reverse;
    f.location.segments.sort_by_key(|s| s.start);
    if reverse {
        f.location.segments.reverse();
    }
    if let Some(len) = circular_len {
        let mut split = Vec::with_capacity(f.location.segments.len() + 1);
        for s in f.location.segments.drain(..) {
            if s.start >= len {
                split.push(Segment::new(s.start - len, s.end - len, s.strand));
            } else if s.end > len {
                let head = Segment::new(s.start, len, s.strand);
                let tail = Segment::new(0, s.end - len, s.strand);
                if reverse {
                    split.extend([tail, head]);
                } else {
                    split.extend([head, tail]);
                }
            } else {
                split.push(s);
            }
        }
        f.location.segments = split;
    }

    let candidates: Vec<(u64, String)> = f
        .qualifiers
        .iter()
        .filter_map(|q| {
            let start = q.key.strip_prefix(PHASE_AT)?.parse().ok()?;
            Some((start, q.value.clone().unwrap_or_default()))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    f.qualifiers.retain(|q| !q.key.starts_with(PHASE_AT));
    let mut all = candidates;
    if let Some(p) = f.qualifier(PHASE) {
        all.push((first_start, p.to_string()));
    }
    let pick = if reverse {
        all.iter().max_by_key(|(s, _)| *s)
    } else {
        all.iter().min_by_key(|(s, _)| *s)
    };
    if let Some((_, phase)) = pick.cloned() {
        f.qualifiers.retain(|q| q.key != PHASE);
        f.qualifiers.push(Qualifier::new(PHASE, phase));
    }
}

/// An attribute's value as written, still percent-encoded.
fn raw_attribute<'a>(col: &'a str, key: &str) -> Option<&'a str> {
    col.split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
}

fn parse_attributes(col: &str) -> Vec<Qualifier> {
    let mut out = Vec::new();
    if col == "." {
        return out;
    }
    for pair in col.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some((k, v)) => {
                let key = decode(k);
                for value in v.split(',') {
                    out.push(Qualifier::new(key.clone(), decode(value)));
                }
            }
            None => out.push(Qualifier {
                key: decode(pair),
                value: None,
            }),
        }
    }
    out
}

fn decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

fn encode(s: &str, extra: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let special =
            c.is_ascii_control() || c == '%' || (c.is_ascii() && extra.contains(&(c as u8)));
        if special {
            let _ = write!(out, "%{:02X}", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

/// Write one sequence's features. `len` is the sequence length, used for the
/// `##sequence-region` line and for re-joining origin-spanning segments.
pub fn write(w: &mut dyn Write, seqs: &[(&SeqFeatures, u64)]) -> io::Result<()> {
    writeln!(w, "##gff-version 3")?;
    for (seq, len) in seqs {
        write_region(w, seq, *len)?;
    }
    let mut auto_id = 0usize;
    for (seq, len) in seqs {
        write_features(w, seq, *len, &mut auto_id)?;
    }
    Ok(())
}

fn write_region(w: &mut dyn Write, seq: &SeqFeatures, len: u64) -> io::Result<()> {
    writeln!(
        w,
        "##sequence-region {} 1 {len}",
        encode(&seq.seqid, b"\t ")
    )
}

fn write_features(
    w: &mut dyn Write,
    seq: &SeqFeatures,
    len: u64,
    auto_id: &mut usize,
) -> io::Result<()> {
    let seqid = encode(&seq.seqid, b"\t");
    for f in &seq.features {
        write_feature(w, &seqid, f, seq.circular.then_some(len), auto_id)?;
    }
    Ok(())
}

/// A companion file without one sequence's region and feature lines. Every
/// other line, including other sequences, comments and an embedded `##FASTA`
/// section, is kept byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    bytes: Vec<u8>,
    /// Where the removed `##sequence-region` line goes back.
    region_at: usize,
    /// Where the removed feature lines go back.
    features_at: usize,
    /// Highest generated `basecraft_auto_N` ID among the kept lines.
    auto_id: usize,
}

/// Split `data` into the lines of `seqid` (dropped) and everything else (kept).
pub fn keep_others(data: &[u8], seqid: &str) -> Kept {
    let mut kept = Kept::default();
    let (mut region_at, mut features_at, mut first_feature) = (None, None, None);
    let mut pos = 0;
    while pos < data.len() {
        let end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |i| pos + i + 1);
        let raw = &data[pos..end];
        pos = end;
        let text = String::from_utf8_lossy(raw);
        let line = text.trim_end_matches(['\n', '\r']);
        if line.starts_with("##FASTA") {
            features_at.get_or_insert(kept.bytes.len());
            kept.bytes.extend_from_slice(raw);
            kept.bytes.extend_from_slice(&data[pos..]);
            break;
        }
        if let Some(rest) = line.strip_prefix("##sequence-region") {
            if rest.split_whitespace().next().map(decode).as_deref() == Some(seqid) {
                region_at.get_or_insert(kept.bytes.len());
                continue;
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            first_feature.get_or_insert(kept.bytes.len());
            if line.split('\t').next().map(decode).as_deref() == Some(seqid) {
                features_at.get_or_insert(kept.bytes.len());
                continue;
            }
            for (at, _) in line.match_indices("basecraft_auto_") {
                let digits: String = line[at + 15..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                kept.auto_id = kept.auto_id.max(digits.parse().unwrap_or(0));
            }
        }
        kept.bytes.extend_from_slice(raw);
    }
    if !kept.bytes.is_empty() && !kept.bytes.ends_with(b"\n") {
        kept.bytes.push(b'\n');
    }
    let end = kept.bytes.len();
    kept.features_at = features_at.unwrap_or(end);
    // A missing region line goes before the first feature line.
    kept.region_at = region_at.unwrap_or_else(|| first_feature.unwrap_or(kept.features_at));
    kept
}

/// Write the kept lines with `seq`'s region and features back in their place.
pub fn write_kept(w: &mut dyn Write, kept: &Kept, seq: &SeqFeatures, len: u64) -> io::Result<()> {
    if kept.bytes.is_empty() {
        return write(w, &[(seq, len)]);
    }
    let mut auto_id = kept.auto_id;
    let (first, second) = (
        kept.region_at.min(kept.features_at),
        kept.region_at.max(kept.features_at),
    );
    w.write_all(&kept.bytes[..first])?;
    if kept.region_at <= kept.features_at {
        write_region(w, seq, len)?;
    } else {
        write_features(w, seq, len, &mut auto_id)?;
    }
    w.write_all(&kept.bytes[first..second])?;
    if kept.region_at <= kept.features_at {
        write_features(w, seq, len, &mut auto_id)?;
    } else {
        write_region(w, seq, len)?;
    }
    w.write_all(&kept.bytes[second..])
}

fn write_feature(
    w: &mut dyn Write,
    seqid: &str,
    f: &Feature,
    circular_len: Option<u64>,
    auto_id: &mut usize,
) -> io::Result<()> {
    let source = f
        .qualifier(SOURCE)
        .map_or(".".to_string(), |s| encode(s, b"\t"));
    let score = f.qualifier(SCORE).unwrap_or(".");
    let first_phase: Option<u64> = f.qualifier(PHASE).and_then(|p| p.parse().ok());
    let segments = rejoin_origin(&f.location.segments, circular_len);

    let mut attrs: Vec<(String, Vec<String>)> = Vec::new();
    for q in f.qualifiers.iter().filter(|q| !q.key.starts_with("gff:")) {
        let value = q.value.clone().unwrap_or_default();
        match attrs.iter_mut().find(|(k, _)| *k == q.key) {
            Some((_, vals)) => vals.push(value),
            None => attrs.push((q.key.clone(), vec![value])),
        }
    }
    let segs = &f.location.segments;
    if let Some(low) = segs.iter().min_by_key(|s| s.start)
        && low.partial_start
    {
        attrs.push((
            "start_range".into(),
            vec![".".into(), (low.start + 1).to_string()],
        ));
    }
    if let Some(high) = segs.iter().max_by_key(|s| s.end)
        && high.partial_end
    {
        attrs.push(("end_range".into(), vec![high.end.to_string(), ".".into()]));
    }
    if segments.len() > 1 && !attrs.iter().any(|(k, _)| k == "ID") {
        *auto_id += 1;
        attrs.insert(
            0,
            ("ID".to_string(), vec![format!("basecraft_auto_{auto_id}")]),
        );
    }
    let attr_col = if attrs.is_empty() {
        ".".to_string()
    } else {
        attrs
            .iter()
            .map(|(k, vals)| {
                let vals: Vec<String> = vals.iter().map(|v| encode(v, b"\t;=&,")).collect();
                format!("{}={}", encode(k, b"\t;=&,"), vals.join(","))
            })
            .collect::<Vec<_>>()
            .join(";")
    };
    let kind = encode(&f.kind, b"\t");

    // Phases follow the biological order; lines are written in genomic order.
    let mut phases = Vec::with_capacity(segments.len());
    let mut done = 0u64;
    for (start, end, _) in &segments {
        phases.push(first_phase.map(|p0| (3 - (done + 3 - p0) % 3) % 3));
        done += end - start;
    }
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].0);
    for i in order {
        let (start, end, strand) = segments[i];
        let strand = match strand {
            Strand::Forward => '+',
            Strand::Reverse => '-',
            Strand::Unstranded => '.',
        };
        let phase = phases[i].map_or(".".to_string(), |p| p.to_string());
        writeln!(
            w,
            "{seqid}\t{source}\t{kind}\t{}\t{end}\t{score}\t{strand}\t{phase}\t{attr_col}",
            start + 1
        )?;
    }
    Ok(())
}

/// Merge a segment ending at the origin with the one continuing from 0,
/// written GFF3-style as a single segment with `end > len`.
fn rejoin_origin(segments: &[Segment], circular_len: Option<u64>) -> Vec<(u64, u64, Strand)> {
    let mut out: Vec<(u64, u64, Strand)> = Vec::with_capacity(segments.len());
    for s in segments {
        if let (Some(len), Some(last)) = (circular_len, out.last_mut()) {
            let forward_wrap = last.2 != Strand::Reverse && last.1 == len && s.start == 0;
            let reverse_wrap = last.2 == Strand::Reverse && last.0 == 0 && s.end == len;
            if s.strand == last.2 && forward_wrap {
                last.1 = len + s.end;
                continue;
            }
            if s.strand == last.2 && reverse_wrap {
                *last = (s.start, len + last.1, s.strand);
                continue;
            }
        }
        out.push((s.start, s.end, s.strand));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "##gff-version 3
##sequence-region chr1 1 1000
chr1\tsrc\tgene\t101\t400\t.\t-\t.\tID=g1;Name=abc%3Bdef
chr1\tsrc\tmRNA\t101\t400\t.\t-\t.\tID=g1.t1;Parent=g1
chr1\tsrc\tCDS\t101\t150\t.\t-\t1\tID=g1.cds;Parent=g1.t1
chr1\tsrc\tCDS\t301\t380\t.\t-\t0\tID=g1.cds;Parent=g1.t1
";

    #[test]
    fn merges_multiline_cds_in_biological_order() {
        let gff = parse(SAMPLE.as_bytes()).unwrap();
        let seq = gff.get("chr1").unwrap();
        assert_eq!(seq.features.len(), 3);
        assert_eq!(seq.features[0].qualifier("Name"), Some("abc;def"));
        let cds = &seq.features[2];
        let starts: Vec<u64> = cds.location.segments.iter().map(|s| s.start).collect();
        assert_eq!(starts, vec![300, 100]);
        // The 5' segment on the reverse strand is the upstream-most one, 301..380.
        assert_eq!(cds.qualifier(PHASE), Some("0"));
    }

    #[test]
    fn roundtrip() {
        let gff = parse(SAMPLE.as_bytes()).unwrap();
        let mut out = Vec::new();
        write(&mut out, &[(&gff.sequences[0], 1000)]).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, SAMPLE);
    }

    #[test]
    fn incomplete_ends_roundtrip() {
        let src = "##gff-version 3
##sequence-region c 1 1000
c\t.\tCDS\t101\t400\t.\t-\t0\tID=x;start_range=.,101;end_range=400,.
";
        let gff = parse(src.as_bytes()).unwrap();
        let loc = &gff.sequences[0].features[0].location;
        assert!(loc.partial_5() && loc.partial_3());
        let mut out = Vec::new();
        write(&mut out, &[(&gff.sequences[0], 1000)]).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), src);
    }

    #[test]
    fn circular_origin_split_and_rejoined() {
        let src = "##gff-version 3
##sequence-region p 1 1000
p\t.\tregion\t1\t1000\t.\t+\t.\tID=p;Is_circular=true
p\t.\tgene\t901\t1100\t.\t+\t.\tID=ori
";
        let gff = parse(src.as_bytes()).unwrap();
        let seq = gff.get("p").unwrap();
        assert!(seq.circular);
        let segs = &seq.features[1].location.segments;
        assert_eq!(
            (segs[0].start, segs[0].end, segs[1].start, segs[1].end),
            (900, 1000, 0, 100)
        );
        let mut out = Vec::new();
        write(&mut out, &[(seq, 1000)]).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), src);
    }
}
