//! Immutable BLAST query preparation shared by GUI and public operation routes.
//! Coordinates are zero based, half open, in biological segment order.
use std::path::Path;

use basecraft_docs::SequenceDoc;
use basecraft_protocol::{Feature, Strand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Result, error::invalid};

pub const NUCLEOTIDE_LIMIT: usize = 1_000_000;
pub const PROTEIN_LIMIT: usize = 100_000;
const NUCLEOTIDE_RESIDUES: &[u8] = b"ACGTURYSWKMBDHVN";

/// A presentation default, never permission to submit or bypass validation.
/// Document molecule metadata wins over ambiguous letters. Unlabelled input
/// defaults to nucleotide unless it contains letters outside the IUPAC set.
pub fn suggested_alphabet(queries: &[Query]) -> &'static str {
    if queries.iter().any(|query| {
        let molecule = query.source["molecule"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase();
        if molecule.contains("protein") || molecule == "aa" {
            true
        } else if molecule.contains("dna") || molecule.contains("rna") {
            false
        } else {
            query.sequence.bytes().any(|b| {
                b.is_ascii_alphabetic() && !NUCLEOTIDE_RESIDUES.contains(&b.to_ascii_uppercase())
            })
        }
    }) {
        "protein"
    } else {
        "nucleotide"
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub name: String,
    pub sequence: String,
    #[serde(default)]
    pub source: Value,
}

pub use basecraft_protocol::query_records::QueryPlan as Plan;

pub fn from_fasta(bytes: &[u8], source: &str) -> Result<Vec<Query>> {
    let index = basecraft_io::fasta::FastaIndex::build(bytes)?;
    index
        .records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            Ok(Query {
                name: record.name.clone(),
                sequence: String::from_utf8(record.read(bytes, 0, record.len)?)
                    .map_err(|_| invalid("BLAST queries must contain ASCII residues"))?,
                source: json!({"path":source,"record":i,"ranges":[[0,record.len]],"strand":"+"}),
            })
        })
        .collect()
}

/// Read every FASTA/GenBank record, or a GUI-generated frozen query collection.
pub fn from_file(path: &Path) -> Result<Vec<Query>> {
    from_file_named(path, &path.display().to_string())
}

pub fn from_file_named(path: &Path, source: &str) -> Result<Vec<Query>> {
    let bytes = std::fs::read(path)?;
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'>') {
        return from_fasta(&bytes, source);
    }
    basecraft_io::genbank::parse(&bytes)?
        .into_iter()
        .enumerate()
        .map(|(i, rec)| {
            let molecule = rec.molecule.clone();
            let mut query = from_doc(&SequenceDoc::from_genbank(rec), None, None)?;
            query.source["path"] = json!(source);
            query.source["record"] = json!(i);
            query.source["molecule"] = json!(molecule);
            Ok(query)
        })
        .collect()
}

/// Current alignment bytes, including unsaved changes; row and column choices
/// remain explicit and use the same preparation as sequence documents/files.
pub fn from_alignment(
    doc: &basecraft_docs::AlignmentDoc,
    rows: (usize, usize),
    columns: (u64, u64),
) -> Result<Vec<Query>> {
    if rows.0 >= rows.1
        || rows.1 > doc.row_count()
        || columns.0 >= columns.1
        || columns.1 > doc.width()
    {
        return Err(invalid("BLAST selection is empty or outside the alignment"));
    }
    (rows.0..rows.1).map(|row| Ok(Query {
        name:doc.row(row).name.clone(),
        sequence:String::from_utf8(doc.read(row,columns.0,columns.1)).map_err(|_|invalid("Non-ASCII alignment query"))?,
        source:json!({"document":doc.name,"revision":doc.version(),"record":row,"ranges":[columns],"strand":"+"}),
    })).collect()
}

/// Reuse Extract's spliced/reverse behavior. Mixed-strand features are rejected
/// explicitly rather than reinterpreted as a uniformly stranded feature.
pub fn from_doc(
    doc: &SequenceDoc,
    range: Option<(u64, u64)>,
    feature: Option<&Feature>,
) -> Result<Query> {
    let (start, end, segments, reverse) = if let Some(feature) = feature {
        let first = feature
            .location
            .segments
            .first()
            .ok_or_else(|| invalid("Annotation has no segments"))?;
        if feature
            .location
            .segments
            .iter()
            .any(|s| s.strand != first.strand)
        {
            return Err(invalid("BLAST does not support mixed-strand annotations"));
        }
        let segments: Vec<_> = feature
            .location
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        (
            first.start,
            first.end,
            segments,
            first.strand == Strand::Reverse,
        )
    } else {
        let (start, end) = range.unwrap_or((0, doc.len()));
        (start, end, vec![(start, end)], false)
    };
    if segments.iter().any(|&(a, b)| a >= b || b > doc.len()) {
        return Err(invalid(
            "BLAST query range is empty or outside the sequence",
        ));
    }
    let sequence = if let Some(feature) = feature {
        crate::files::feature_sequence(doc, feature)?
    } else {
        crate::files::extract_record(doc, start, end, &segments, reverse).sequence
    };
    Ok(Query {
        name: feature.map_or_else(
            || doc.name.clone(),
            |f| f.label().unwrap_or(&f.kind).to_owned(),
        ),
        sequence: String::from_utf8(sequence).map_err(|_| invalid("Non-ASCII query"))?,
        source: json!({"document":doc.name,"revision":doc.version(),"ranges":segments,"strand":if reverse {"-"} else {"+"},"molecule":doc.genbank.as_ref().map(|meta| &meta.molecule)}),
    })
}

/// Build the proposed prefix policy without granting permission to upload it.
/// Validation includes omitted records, before any transport or truncation.
pub fn prepare(queries: &[Query], settings: &Value) -> Result<Plan> {
    if queries.is_empty() {
        return Err(invalid("BLAST query contains no records"));
    }
    if queries.len() > 4096 {
        return Err(crate::Error::Limit {
            what: "BLAST query records per submission".into(),
            limit: 4096,
        });
    }
    let alphabet = settings["alphabet"].as_str().unwrap_or("");
    let (allowed, limit) = match alphabet {
        "nucleotide" => (NUCLEOTIDE_RESIDUES, NUCLEOTIDE_LIMIT),
        "protein" => (b"ABCDEFGHIKLMNPQRSTVWXYZUOJ".as_slice(), PROTEIN_LIMIT),
        _ => {
            return Err(invalid(
                "Choose nucleotide or protein explicitly; alphabet is not inferred from ambiguous letters",
            ));
        }
    };
    let database = settings["database"].as_str().unwrap_or("");
    let program = match (alphabet, database) {
        ("nucleotide", "nt") => "blastn",
        ("nucleotide", "nr") => "blastx",
        ("protein", "nr") => "blastp",
        ("protein", "nt") => "tblastn",
        _ => return Err(invalid("BLAST database must be nt or nr")),
    };
    if settings["megablast"] == true && program != "blastn" {
        return Err(invalid(
            "Megablast requires a nucleotide query and nt database",
        ));
    }
    let mut normalized = Vec::new();
    let mut total = 0usize;
    for query in queries {
        if let Some(molecule) = query.source["molecule"].as_str() {
            let molecule = molecule.to_ascii_lowercase();
            let protein = molecule.contains("protein") || molecule == "aa";
            let nucleotide = molecule.contains("dna") || molecule.contains("rna");
            if (protein && alphabet != "protein") || (nucleotide && alphabet != "nucleotide") {
                return Err(invalid(format!(
                    "{}: chosen alphabet disagrees with document molecule {}",
                    query.name, molecule
                )));
            }
        }
        let seq: Vec<_> = query
            .sequence
            .bytes()
            .filter(|b| !b.is_ascii_whitespace() && *b != b'-' && *b != b'.')
            .map(|b| b.to_ascii_uppercase())
            .collect();
        if seq.is_empty() {
            return Err(invalid(format!(
                "{}: query is empty or all gaps",
                query.name
            )));
        }
        if let Some(b) = seq.iter().find(|b| !allowed.contains(b)) {
            return Err(invalid(format!(
                "{}: invalid {} residue {:?}",
                query.name,
                alphabet,
                char::from(*b)
            )));
        }
        total = total
            .checked_add(seq.len())
            .ok_or_else(|| invalid("Query length overflow"))?;
        normalized.push(seq);
    }
    let mut budget = limit;
    let mut fasta = Vec::new();
    let mut records = Vec::new();
    for (i, (query, seq)) in queries.iter().zip(&normalized).enumerate() {
        let retained = seq.len().min(budget);
        budget -= retained;
        let id = format!("query_{}", i + 1);
        let input_ranges = retained_offsets(&query.sequence, retained);
        let source_ranges = project_ranges(&query.source, &input_ranges)?;
        records.push(basecraft_protocol::query_records::QueryRecord {
            id: id.clone(),
            name: query.name.clone(),
            source: query.source.clone(),
            original_length: seq.len(),
            submitted_length: retained,
            submitted_query_range: [0, retained],
            omitted: retained == 0,
            shortened: retained > 0 && retained < seq.len(),
            removed_gaps_or_whitespace: query.sequence.len() - seq.len(),
            submitted_input_ranges: input_ranges,
            submitted_source_ranges: source_ranges,
        });
        if retained > 0 {
            basecraft_io::fasta::write_record(&mut fasta, id.as_bytes(), &seq[..retained], 80)?;
        }
    }
    // Bind approval to ALL original bytes, names, mapping and effective search
    // settings. Truncation consent itself is excluded so approval can grant it.
    let mut identity = settings.clone();
    if let Some(object) = identity.as_object_mut() {
        object.remove("truncate");
        object.remove("approval_hash");
    }
    let approval_hash = basecraft_io::hash_bytes(&serde_json::to_vec(&(queries, identity))?);
    let plan = Plan {
        schema: 1,
        alphabet: alphabet.into(),
        program: program.into(),
        database: database.into(),
        original_residues: total,
        limit,
        submitted_residues: limit - budget,
        oversized: total > limit,
        records,
        query_hash: format!("sha256:{}", basecraft_io::sha256_bytes(&fasta)),
        fasta: String::from_utf8(fasta).map_err(|_| invalid("Invalid FASTA"))?,
        approval_hash,
        truncation: total > limit && settings["truncate"] == true,
    };
    if serde_json::to_vec(&plan)?.len() > 3 * 1024 * 1024 {
        return Err(invalid(
            "BLAST query mapping exceeds the evidence limit; select fewer records or a less fragmented range",
        ));
    }
    Ok(plan)
}

fn retained_offsets(sequence: &str, count: usize) -> Vec<(u64, u64)> {
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for (offset, _) in sequence
        .bytes()
        .enumerate()
        .filter(|(_, b)| !b.is_ascii_whitespace() && *b != b'-' && *b != b'.')
        .take(count)
    {
        let offset = offset as u64;
        if let Some(last) = ranges.last_mut()
            && last.1 == offset
        {
            last.1 += 1;
        } else {
            ranges.push((offset, offset + 1));
        }
    }
    ranges
}

fn project_ranges(source: &Value, retained: &[(u64, u64)]) -> Result<Vec<Value>> {
    let Some(ranges) = source["ranges"].as_array() else {
        return Ok(Vec::new());
    };
    let mut offset = 0u64;
    let reverse = source["strand"] == "-";
    let mut projected = Vec::new();
    for range in ranges {
        let a = range[0]
            .as_u64()
            .ok_or_else(|| invalid("Invalid source range"))?;
        let b = range[1]
            .as_u64()
            .filter(|b| *b >= a)
            .ok_or_else(|| invalid("Invalid source range"))?;
        let length = b - a;
        let next = offset
            .checked_add(length)
            .ok_or_else(|| invalid("Source range overflow"))?;
        for &(lo, hi) in retained {
            let start = lo.max(offset);
            let end = hi.min(next);
            if start < end {
                let (start, end) = if reverse {
                    (b - (end - offset), b - (start - offset))
                } else {
                    (a + start - offset, a + end - offset)
                };
                projected
                    .push(json!({"start":start,"end":end,"strand":if reverse {"-"} else {"+"}}));
            }
        }
        offset = next;
    }
    Ok(projected)
}

pub fn authorize(plan: &Plan, settings: &Value) -> Result<()> {
    if let Some(hash) = settings["approval_hash"].as_str()
        && hash != plan.approval_hash
    {
        return Err(invalid(
            "BLAST source or settings changed; review a new preview",
        ));
    }
    if plan.oversized && !plan.truncation {
        return Err(invalid(format!(
            "BLAST query has {} residues; {} permits {}. Explicit truncate=true is required after reviewing the prefix/omission proposal",
            plan.original_residues, plan.alphabet, plan.limit
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(alphabet: &str) -> Value {
        json!({"alphabet":alphabet,"database":"nr"})
    }
    fn query(sequence: String) -> Query {
        Query {
            name: "duplicate".into(),
            sequence,
            source: Value::Null,
        }
    }
    #[test]
    fn presentation_default_respects_metadata_and_keeps_validation() {
        let mut q = query("ACGTNNRY---".into());
        assert_eq!(suggested_alphabet(&[q.clone()]), "nucleotide");
        q.source = json!({"molecule":"protein"});
        assert_eq!(suggested_alphabet(&[q.clone()]), "protein");
        q.sequence = "MELKQ".into();
        q.source = Value::Null;
        assert_eq!(suggested_alphabet(&[q.clone()]), "protein");
        q.source = json!({"molecule":"DNA"});
        assert_eq!(suggested_alphabet(&[q.clone()]), "nucleotide");
        assert!(prepare(&[q], &settings("nucleotide")).is_err());
        let mixed = vec![
            query("ACGT".into()),
            Query {
                source: json!({"molecule":"protein"}),
                ..query("ACGT".into())
            },
        ];
        // An inferred default never makes conflicting explicit metadata valid.
        let mut mixed = mixed;
        mixed[0].source = json!({"molecule":"DNA"});
        assert!(prepare(&mixed, &settings(suggested_alphabet(&mixed))).is_err());
    }

    #[test]
    fn limits_consent_and_frozen_identity() {
        for (alphabet, limit) in [("nucleotide", NUCLEOTIDE_LIMIT), ("protein", PROTEIN_LIMIT)] {
            let mut s = settings(alphabet);
            let q = vec![query("A".repeat(limit))];
            let p = prepare(&q, &s).unwrap();
            assert!(!p.oversized);
            authorize(&p, &s).unwrap();
            let q = vec![
                query("A".repeat(limit - 2)),
                query("AAAA".into()),
                query("A".into()),
            ];
            let p = prepare(&q, &s).unwrap();
            assert!(authorize(&p, &s).is_err());
            assert_eq!(p.records[1].submitted_length, 2);
            assert_eq!(p.records[2].omitted, true);
            s["truncate"] = json!(true);
            s["approval_hash"] = json!(p.approval_hash);
            authorize(&prepare(&q, &s).unwrap(), &s).unwrap();
            s["database"] = json!("nt");
            assert!(authorize(&prepare(&q, &s).unwrap(), &s).is_err());
            assert_eq!(q[1].sequence, "AAAA");
        }
    }
    #[test]
    fn fasta_all_records_unique_ids_and_invalid_omitted_record() {
        let q = from_fasta(b">same\nAa-U\n>same\nNN\n", "source").unwrap();
        let p = prepare(&q, &settings("nucleotide")).unwrap();
        assert_eq!(p.fasta, ">query_1\nAAU\n>query_2\nNN\n");
        assert!(prepare(&[query("- .".into())], &settings("protein")).is_err());
        assert!(prepare(&[query("E".into())], &settings("nucleotide")).is_err());
        assert!(
            prepare(
                &[query("A".repeat(PROTEIN_LIMIT)), query("*".into())],
                &settings("protein")
            )
            .is_err()
        );
    }
    #[test]
    fn reverse_join_uses_extract_and_preserves_source() {
        let bytes = b"LOCUS       test 12 bp DNA circular\nFEATURES             Location/Qualifiers\n     CDS             complement(join(1..3,10..12))\nORIGIN\n        1 aaacccgggttt\n//\n";
        let rec = basecraft_io::genbank::parse(bytes).unwrap().remove(0);
        let doc = SequenceDoc::from_genbank(rec);
        let feature = doc.features().next().unwrap().1;
        let query = from_doc(&doc, None, Some(feature)).unwrap();
        assert_eq!(query.sequence.to_ascii_uppercase(), "AAATTT");
        assert_eq!(doc.read(0, 12), b"AAACCCGGGTTT");
        assert!(from_doc(&doc, Some((4, 99)), None).is_err());
    }
    #[test]
    fn unsaved_selection_join_ranges_and_changed_bytes() {
        let mut doc = SequenceDoc::new(
            "unsaved",
            std::sync::Arc::new(b"AAACCCGGGTTT".to_vec()),
            Vec::new(),
            true,
        );
        doc.insert(3, b"AT");
        let q = from_doc(&doc, Some((3, 8)), None).unwrap();
        assert_eq!(q.sequence, "ATCCC");
        let s = settings("nucleotide");
        let p = prepare(std::slice::from_ref(&q), &s).unwrap();
        assert_eq!(
            serde_json::to_value(&p.records[0].submitted_source_ranges).unwrap(),
            json!([{"start":3,"end":8,"strand":"+"}])
        );
        let mut approved = s;
        approved["approval_hash"] = json!(p.approval_hash);
        let mut changed = q;
        changed.sequence.replace_range(0..1, "G");
        assert!(authorize(&prepare(&[changed], &approved).unwrap(), &approved).is_err());
        let reverse = json!({"ranges":[[9,12],[0,3]],"strand":"-"});
        assert_eq!(
            project_ranges(&reverse, &[(0, 4)]).unwrap(),
            vec![
                json!({"start":9,"end":12,"strand":"-"}),
                json!({"start":2,"end":3,"strand":"-"})
            ]
        );
    }
    #[test]
    fn current_alignment_all_rows_and_selected_columns() {
        use basecraft_docs::alignment::AlnRow;
        let doc = basecraft_docs::AlignmentDoc::new(
            "batch",
            vec![
                AlnRow::new("same", std::sync::Arc::new(b"A-CGT".to_vec())),
                AlnRow::new("same", std::sync::Arc::new(b"NNCGT".to_vec())),
            ],
        );
        let q = from_alignment(&doc, (0, 2), (0, 5)).unwrap();
        let plan = prepare(&q, &settings("nucleotide")).unwrap();
        assert_eq!(plan.records.len(), 2);
        assert_eq!(plan.submitted_residues, 9);
        assert_eq!(
            serde_json::to_value(&plan.records[0].submitted_input_ranges).unwrap(),
            json!([[0, 1], [2, 5]])
        );
        let q = from_alignment(&doc, (1, 2), (2, 4)).unwrap();
        assert_eq!(q[0].sequence, "CG");
        assert!(from_alignment(&doc, (2, 3), (0, 1)).is_err());
    }
}
