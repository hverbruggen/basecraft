//! GenBank flat file reader and writer.
//!
//! The sequence is parsed into memory: GenBank interleaves numbers and
//! spaces with residues, so it can't be read straight from a mapped file.
//! Header lines between `LOCUS` and `FEATURES` are kept verbatim.

use std::io::{self, BufRead, Read, Write};

use basecraft_protocol::{Feature, Location, Qualifier, Segment, Strand};

use crate::Result;
use crate::error::parse_error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub molecule: String,
    pub circular: bool,
    pub division: String,
    pub date: String,
    /// Lines after `LOCUS` and before `FEATURES`, verbatim.
    pub header: Vec<String>,
    pub features: Vec<Feature>,
    pub sequence: Vec<u8>,
}

pub fn parse(data: &[u8]) -> Result<Vec<Record>> {
    read_records(io::Cursor::new(data), data.len(), false).map(|(records, _)| records)
}

/// Parse only the editable first record; count later records without retaining them.
pub fn parse_first(data: &[u8]) -> Result<(Record, usize)> {
    first(read_records(io::Cursor::new(data), data.len(), true)?)
}

fn first((records, count): (Vec<Record>, usize)) -> Result<(Record, usize)> {
    records
        .into_iter()
        .next()
        .map(|record| (record, count))
        .ok_or_else(|| parse_error("GenBank", 1, "expected LOCUS"))
}

/// Read, hash and parse together. No raw whole-file buffer is constructed.
pub fn open_hashed(path: &std::path::Path) -> Result<(Record, usize, String)> {
    let file = std::fs::File::open(path)?;
    let size = usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX);
    let mut reader = HashingReader {
        file,
        hash: crate::Hasher::new(),
        bytes: 0,
    };
    let result = first(read_records(
        io::BufReader::with_capacity(size.clamp(1, 64 * 1024), &mut reader),
        size,
        true,
    )?);
    #[cfg(feature = "test-counters")]
    crate::counters::record_read(path, reader.bytes);
    let (record, count) = result?;
    Ok((record, count, reader.hash.finish()))
}

struct HashingReader {
    file: std::fs::File,
    hash: crate::Hasher,
    bytes: u64,
}
impl Read for HashingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(buffer)?;
        self.hash.update(&buffer[..count]);
        self.bytes += count as u64;
        Ok(count)
    }
}

type Lines<R> = std::iter::Peekable<std::iter::Enumerate<io::Lines<R>>>;
fn peek<R: BufRead>(lines: &mut Lines<R>) -> Result<Option<(usize, &str)>> {
    match lines.peek() {
        Some((no, Ok(line))) => Ok(Some((*no, line.as_str()))),
        Some((no, Err(error))) => Err(parse_error("GenBank", no + 1, error.to_string())),
        None => Ok(None),
    }
}

fn read_records(
    reader: impl BufRead,
    size: usize,
    first_only: bool,
) -> Result<(Vec<Record>, usize)> {
    let mut records = Vec::new();
    let mut count = 0;
    let mut lines = reader.lines().enumerate().peekable();
    while let Some((no, line)) = lines.next() {
        let line = line.map_err(|error| parse_error("GenBank", no + 1, error.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with("LOCUS") {
            return Err(parse_error("GenBank", no + 1, "expected LOCUS"));
        }
        count += 1;
        if first_only && count > 1 {
            for (no, line) in lines.by_ref() {
                let line =
                    line.map_err(|error| parse_error("GenBank", no + 1, error.to_string()))?;
                if line.starts_with("//") {
                    break;
                }
            }
            continue;
        }
        let mut rec = parse_locus(&line);
        let declared = line
            .split_whitespace()
            .nth(2)
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        rec.sequence
            .try_reserve_exact(declared.min(size))
            .map_err(io::Error::other)?;
        while let Some((_, line)) = peek(&mut lines)? {
            if line.starts_with("FEATURES") || line.starts_with("ORIGIN") || line.starts_with("//")
            {
                break;
            }
            rec.header.push(line.to_owned());
            lines.next();
        }
        if peek(&mut lines)?.is_some_and(|(_, line)| line.starts_with("FEATURES")) {
            lines.next();
            parse_features(&mut lines, &mut rec)?;
        }
        if peek(&mut lines)?.is_some_and(|(_, line)| line.starts_with("ORIGIN")) {
            lines.next();
            for (no, line) in lines.by_ref() {
                let line =
                    line.map_err(|error| parse_error("GenBank", no + 1, error.to_string()))?;
                if line.starts_with("//") {
                    break;
                }
                rec.sequence.extend(
                    line.bytes()
                        .filter(u8::is_ascii_alphabetic)
                        .map(|b| b.to_ascii_uppercase()),
                );
            }
        } else if peek(&mut lines)?.is_some_and(|(_, line)| line.starts_with("//")) {
            lines.next();
        }
        records.push(rec);
    }
    Ok((records, count))
}

fn parse_locus(line: &str) -> Record {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let name = parts.get(1).copied().unwrap_or("").to_string();
    let circular = parts.contains(&"circular");
    // After "bp": molecule type, optional topology, division, date.
    let after_bp: Vec<&str> = parts
        .iter()
        .position(|p| *p == "bp" || *p == "aa")
        .map(|i| {
            parts[i + 1..]
                .iter()
                .copied()
                .filter(|p| *p != "linear" && *p != "circular")
                .collect()
        })
        .unwrap_or_default();
    Record {
        name,
        molecule: after_bp.first().copied().unwrap_or("DNA").to_string(),
        circular,
        division: after_bp.get(1).copied().unwrap_or("UNK").to_string(),
        date: after_bp
            .get(2)
            .copied()
            .unwrap_or("01-JAN-1970")
            .to_string(),
        header: Vec::new(),
        features: Vec::new(),
        sequence: Vec::new(),
    }
}

fn parse_features<R: BufRead>(lines: &mut Lines<R>, rec: &mut Record) -> Result<()> {
    // Collect each feature's raw text first: key, location text, qualifier lines.
    struct Raw {
        line: usize,
        key: String,
        location: String,
        qualifiers: Vec<String>,
    }
    let mut raws: Vec<Raw> = Vec::new();
    while let Some((no, l)) = peek(lines)? {
        if !l.starts_with(' ') {
            break;
        }
        let key_field = l.get(5..21).unwrap_or("").trim();
        let rest = l.get(21..).unwrap_or("").trim_end();
        if !key_field.is_empty() {
            raws.push(Raw {
                line: no + 1,
                key: key_field.to_string(),
                location: rest.to_string(),
                qualifiers: Vec::new(),
            });
            lines.next();
            continue;
        }
        let Some(raw) = raws.last_mut() else {
            return Err(parse_error(
                "GenBank",
                no + 1,
                "continuation line before any feature",
            ));
        };
        if rest.starts_with('/') {
            raw.qualifiers.push(rest.to_string());
        } else if let Some(q) = raw.qualifiers.last_mut() {
            // Continuation of a qualifier value. Translations and other
            // unspaced values join without a space.
            let unspaced = q.starts_with("/translation") || !q.contains(' ') && !rest.contains(' ');
            if !unspaced {
                q.push(' ');
            }
            q.push_str(rest);
        } else {
            raw.location.push_str(rest);
        }
        lines.next();
    }
    for raw in raws {
        let location = parse_location(&raw.location).ok_or_else(|| {
            parse_error(
                "GenBank",
                raw.line,
                format!("unsupported location {:?}", raw.location),
            )
        })?;
        let qualifiers = raw.qualifiers.iter().map(|q| parse_qualifier(q)).collect();
        rec.features.push(Feature {
            kind: raw.key,
            location,
            qualifiers,
        });
    }
    Ok(())
}

fn parse_qualifier(q: &str) -> Qualifier {
    let body = &q[1..];
    match body.split_once('=') {
        Some((k, v)) => {
            let v = v.trim();
            let value = if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                v[1..v.len() - 1].replace("\"\"", "\"")
            } else {
                v.to_string()
            };
            Qualifier::new(k, value)
        }
        None => Qualifier {
            key: body.to_string(),
            value: None,
        },
    }
}

/// Parse a GenBank location into segments in biological order. Returns
/// `None` for references to other entries (`AB000001.1:1..10`).
pub fn parse_location(text: &str) -> Option<Location> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut unordered = false;
    let segments = parse_loc(&compact, Strand::Forward, &mut unordered)?;
    Some(Location {
        segments,
        unordered,
    })
}

fn parse_loc(s: &str, strand: Strand, unordered: &mut bool) -> Option<Vec<Segment>> {
    if let Some(inner) = strip_call(s, "complement") {
        let flipped = match strand {
            Strand::Reverse => Strand::Forward,
            _ => Strand::Reverse,
        };
        let mut segs = parse_loc(inner, flipped, unordered)?;
        segs.reverse();
        return Some(segs);
    }
    for (name, is_order) in [("join", false), ("order", true)] {
        if let Some(inner) = strip_call(s, name) {
            *unordered |= is_order;
            let mut out = Vec::new();
            for part in split_top_level(inner) {
                out.extend(parse_loc(part, strand, unordered)?);
            }
            return Some(out);
        }
    }
    parse_range(s, strand).map(|seg| vec![seg])
}

fn strip_call<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    s.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')
}

fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn parse_range(s: &str, strand: Strand) -> Option<Segment> {
    if s.contains(':') {
        return None;
    }
    let num = |t: &str| -> Option<(u64, bool)> {
        let partial = t.starts_with('<') || t.starts_with('>');
        t.trim_start_matches(['<', '>'])
            .parse()
            .ok()
            .map(|n| (n, partial))
    };
    if let Some((a, b)) = s.split_once("..") {
        let (start, ps) = num(a)?;
        let (end, pe) = num(b)?;
        if start == 0 || end < start {
            return None;
        }
        let mut seg = Segment::new(start - 1, end, strand);
        seg.partial_start = ps;
        seg.partial_end = pe;
        return Some(seg);
    }
    if let Some((a, _)) = s.split_once('^') {
        // A site between two bases: an empty segment after base `a`.
        let (pos, _) = num(a)?;
        return Some(Segment::new(pos, pos, strand));
    }
    let (pos, partial) = num(s)?;
    if pos == 0 {
        return None;
    }
    let mut seg = Segment::new(pos - 1, pos, strand);
    seg.partial_start = partial;
    Some(seg)
}

/// Format a location in GenBank syntax.
pub fn format_location(loc: &Location) -> String {
    let fmt_seg = |s: &Segment| -> String {
        if s.is_empty() {
            return format!("{}^{}", s.start, s.start + 1);
        }
        let lt = if s.partial_start { "<" } else { "" };
        let gt = if s.partial_end { ">" } else { "" };
        if s.len() == 1 && !s.partial_end {
            format!("{lt}{}", s.end)
        } else {
            format!("{lt}{}..{gt}{}", s.start + 1, s.end)
        }
    };
    let op = if loc.unordered { "order" } else { "join" };
    let all_reverse =
        !loc.segments.is_empty() && loc.segments.iter().all(|s| s.strand == Strand::Reverse);
    if all_reverse {
        // complement(join(...)) lists segments in genomic order, the reverse
        // of biological order.
        let inner: Vec<String> = loc.segments.iter().rev().map(fmt_seg).collect();
        return if inner.len() == 1 {
            format!("complement({})", inner[0])
        } else {
            format!("complement({op}({}))", inner.join(","))
        };
    }
    let parts: Vec<String> = loc
        .segments
        .iter()
        .map(|s| match s.strand {
            Strand::Reverse => format!("complement({})", fmt_seg(s)),
            _ => fmt_seg(s),
        })
        .collect();
    if parts.len() == 1 {
        parts.into_iter().next().unwrap_or_default()
    } else {
        format!("{op}({})", parts.join(","))
    }
}

/// Qualifiers whose values GenBank writes without quotes.
const UNQUOTED: &[&str] = &[
    "codon_start",
    "transl_table",
    "number",
    "citation",
    "rpt_type",
    "direction",
    "transl_except",
    "anticodon",
    "rpt_unit_range",
    "estimated_length",
    "compare",
    "tag_peptide",
];

const WIDTH: usize = 79;
const INDENT: &str = "                     ";

pub fn write(w: &mut dyn Write, rec: &Record) -> io::Result<()> {
    let topology = if rec.circular { "circular" } else { "linear" };
    writeln!(
        w,
        "LOCUS       {:<16} {:>11} bp    {:<7} {topology:<8} {} {}",
        rec.name,
        rec.sequence.len(),
        rec.molecule,
        rec.division,
        rec.date
    )?;
    for l in &rec.header {
        writeln!(w, "{l}")?;
    }
    writeln!(w, "FEATURES             Location/Qualifiers")?;
    for f in &rec.features {
        write_wrapped(
            w,
            &format!("     {:<16}", f.kind),
            &format_location(&f.location),
            &[','],
        )?;
        for q in f.qualifiers.iter().filter(|q| !q.key.contains(':')) {
            let text = match &q.value {
                None => format!("/{}", q.key),
                Some(v) if UNQUOTED.contains(&q.key.as_str()) => format!("/{}={v}", q.key),
                Some(v) => format!("/{}=\"{}\"", q.key, v.replace('"', "\"\"")),
            };
            write_wrapped(w, INDENT, &text, &[' '])?;
        }
    }
    writeln!(w, "ORIGIN")?;
    for (i, line) in rec.sequence.chunks(60).enumerate() {
        write!(w, "{:>9}", i * 60 + 1)?;
        for block in line.chunks(10) {
            w.write_all(b" ")?;
            w.write_all(&block.to_ascii_lowercase())?;
        }
        w.write_all(b"\n")?;
    }
    writeln!(w, "//")
}

/// Write `text` after `first_prefix`, wrapping at column 79 with a 21-space
/// indent. Breaks after one of `break_after` where possible, otherwise hard.
fn write_wrapped(
    w: &mut dyn Write,
    first_prefix: &str,
    text: &str,
    break_after: &[char],
) -> io::Result<()> {
    let room = WIDTH - INDENT.len();
    let mut rest = text;
    let mut prefix = first_prefix;
    loop {
        if rest.len() <= room {
            return writeln!(w, "{prefix}{rest}");
        }
        let window = &rest[..floor_char_boundary(rest, room)];
        let cut = window
            .rfind(|c| break_after.contains(&c))
            .map_or(window.len(), |i| i + 1);
        let (line, tail) = rest.split_at(cut);
        writeln!(w, "{prefix}{}", line.trim_end())?;
        rest = tail.trim_start_matches(' ');
        prefix = INDENT;
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        let cases = [
            ("100..200", vec![(99, 200, Strand::Forward)]),
            ("complement(100..200)", vec![(99, 200, Strand::Reverse)]),
            (
                "complement(join(10..20,30..40))",
                vec![(29, 40, Strand::Reverse), (9, 20, Strand::Reverse)],
            ),
            (
                "join(complement(30..40),complement(10..20))",
                vec![(29, 40, Strand::Reverse), (9, 20, Strand::Reverse)],
            ),
            (
                "join(990..1000,1..5)",
                vec![(989, 1000, Strand::Forward), (0, 5, Strand::Forward)],
            ),
            ("5", vec![(4, 5, Strand::Forward)]),
        ];
        for (text, expect) in cases {
            let loc = parse_location(text).unwrap();
            let got: Vec<(u64, u64, Strand)> = loc
                .segments
                .iter()
                .map(|s| (s.start, s.end, s.strand))
                .collect();
            assert_eq!(got, expect, "{text}");
        }
        let partial = parse_location("<1..>50").unwrap();
        assert!(partial.segments[0].partial_start && partial.segments[0].partial_end);
        assert_eq!(format_location(&partial), "<1..>50");
        assert!(parse_location("AB000001.1:1..10").is_none());
    }

    #[test]
    fn location_format_roundtrip() {
        for text in [
            "100..200",
            "complement(100..200)",
            "complement(join(10..20,30..40))",
            "join(990..1000,1..5)",
            "order(1..5,10..20)",
            "join(1..5,complement(10..20))",
        ] {
            assert_eq!(format_location(&parse_location(text).unwrap()), text);
        }
    }

    const SAMPLE: &str = r#"LOCUS       TEST                     120 bp    DNA     circular SYN 10-OCT-2026
DEFINITION  A test record.
ACCESSION   TEST
FEATURES             Location/Qualifiers
     source          1..120
                     /organism="synthetic construct"
     gene            complement(join(100..120,1..10))
                     /gene="ori"
                     /pseudo
     CDS             join(20..40,50..70)
                     /gene="abc"
                     /codon_start=1
                     /note="a long note that has to wrap across more than one
                     line because it is long, with ""quotes"""
                     /translation="MKKLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL
                     LLLLLLLLLL"
ORIGIN
        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
       61 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
//
"#;

    #[test]
    fn parse_record() {
        let recs = parse(SAMPLE.as_bytes()).unwrap();
        let r = &recs[0];
        assert_eq!(r.name, "TEST");
        assert!(r.circular);
        assert_eq!(r.sequence.len(), 120);
        assert_eq!(r.features.len(), 3);
        let ori = &r.features[1];
        let segs: Vec<(u64, u64)> = ori
            .location
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        assert_eq!(segs, vec![(0, 10), (99, 120)]);
        assert_eq!(ori.qualifiers[1].value, None);
        let cds = &r.features[2];
        assert_eq!(
            cds.qualifier("note"),
            Some(
                "a long note that has to wrap across more than one line because it is long, with \"quotes\""
            )
        );
        assert_eq!(cds.qualifier("translation").map(str::len), Some(66));
    }

    #[test]
    fn roundtrip() {
        let recs = parse(SAMPLE.as_bytes()).unwrap();
        let mut out = Vec::new();
        write(&mut out, &recs[0]).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(parse(text.as_bytes()).unwrap(), recs);
        assert!(text.lines().all(|l| l.len() <= 80), "{text}");
    }
}
