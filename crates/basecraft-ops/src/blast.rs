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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub name: String,
    pub sequence: String,
    #[serde(default)]
    pub source: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub schema: u32,
    pub alphabet: String,
    pub program: String,
    pub database: String,
    pub original_residues: usize,
    pub limit: usize,
    pub submitted_residues: usize,
    pub oversized: bool,
    pub records: Vec<Value>,
    pub fasta: String,
    pub query_hash: String,
    pub approval_hash: String,
    pub truncation: bool,
}

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
    let bytes = std::fs::read(path)?;
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'>') {
        return from_fasta(&bytes, &path.display().to_string());
    }
    basecraft_io::genbank::parse(&bytes)?
        .into_iter()
        .enumerate()
        .map(|(i, rec)| {
            let molecule = rec.molecule.clone();
            let mut query = from_doc(&SequenceDoc::from_genbank(rec), None, None)?;
            query.source["path"] = json!(path.display().to_string());
            query.source["record"] = json!(i);
            query.source["molecule"] = json!(molecule);
            Ok(query)
        })
        .collect()
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
        source: json!({"document":doc.name,"revision":doc.version(),"ranges":segments,"strand":if reverse {"-"} else {"+"}}),
    })
}

/// Build the proposed prefix policy without granting permission to upload it.
/// Validation includes omitted records, before any transport or truncation.
pub fn prepare(queries: &[Query], settings: &Value) -> Result<Plan> {
    if queries.is_empty() {
        return Err(invalid("BLAST query contains no records"));
    }
    let alphabet = settings["alphabet"].as_str().unwrap_or("");
    let (allowed, limit) = match alphabet {
        "nucleotide" => (b"ACGTURYSWKMBDHVN".as_slice(), NUCLEOTIDE_LIMIT),
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
        records.push(json!({"id":id,"name":query.name,"source":query.source,"original_length":seq.len(),"submitted_length":retained,"submitted_query_range":[0,retained],"omitted":retained==0,"shortened":retained>0 && retained<seq.len(),"removed_gaps_or_whitespace":query.sequence.len()-seq.len()}));
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
    Ok(Plan {
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
    })
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
            assert_eq!(p.records[1]["submitted_length"], 2);
            assert_eq!(p.records[2]["omitted"], true);
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
        assert_eq!(doc.read(0, 12), b"aaacccgggttt");
        assert!(from_doc(&doc, Some((4, 99)), None).is_err());
    }
}
