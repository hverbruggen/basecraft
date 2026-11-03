//! NCBI submission files for table2asn: nucleotide FASTA (`.fsa`) with
//! source modifiers, source tables (`.src`), five-column feature tables
//! (`.tbl`) and submission templates (`.sbt`), plus table2asn validation
//! reports (`.val`).
//!
//! These are independent of the GenBank flatfile writer: submission files
//! carry only what NCBI's tools read. Writers take values as given; callers
//! check that they contain no tabs, line breaks or non-ASCII characters.
use std::io::{self, Write};

/// One FASTA record with `[name=value]` modifiers on its definition line.
pub struct FsaRecord<'a> {
    pub id: &'a str,
    pub modifiers: &'a [(String, String)],
    pub residues: &'a [u8],
}

/// Write records, residues 60 to a line.
pub fn write_fsa(w: &mut dyn Write, records: &[FsaRecord<'_>]) -> io::Result<()> {
    for record in records {
        write!(w, ">{}", record.id)?;
        for (name, value) in record.modifiers {
            write!(w, " [{name}={value}]")?;
        }
        writeln!(w)?;
        for line in record.residues.chunks(60) {
            w.write_all(line)?;
            writeln!(w)?;
        }
    }
    Ok(())
}

/// Write a source table: `Sequence_ID` then one column per qualifier, in
/// the order qualifiers first appear. Absent values are empty cells.
pub fn write_src(w: &mut dyn Write, rows: &[(&str, &[(String, String)])]) -> io::Result<()> {
    let mut columns: Vec<&str> = Vec::new();
    for (_, qualifiers) in rows {
        for (name, _) in *qualifiers {
            if !columns.contains(&name.as_str()) {
                columns.push(name);
            }
        }
    }
    write!(w, "Sequence_ID")?;
    for column in &columns {
        write!(w, "\t{column}")?;
    }
    writeln!(w)?;
    for (id, qualifiers) in rows {
        write!(w, "{id}")?;
        for column in &columns {
            let value = qualifiers
                .iter()
                .find(|(n, _)| n == column)
                .map_or("", |(_, v)| v.as_str());
            write!(w, "\t{value}")?;
        }
        writeln!(w)?;
    }
    Ok(())
}

/// A feature in a five-column table, 1-based inclusive.
pub struct TableFeature<'a> {
    pub kind: &'a str,
    pub start: u64,
    pub end: u64,
    pub partial5: bool,
    pub partial3: bool,
    pub qualifiers: &'a [(String, String)],
}

/// Write a five-column feature table: `>Feature ID`, then for each
/// feature `start<TAB>end<TAB>key` (`<` and `>` mark partial ends) and
/// `<TAB><TAB><TAB>qualifier<TAB>value` lines.
pub fn write_tbl(w: &mut dyn Write, records: &[(&str, Vec<TableFeature<'_>>)]) -> io::Result<()> {
    for (id, features) in records {
        writeln!(w, ">Feature {id}")?;
        for f in features {
            let five = if f.partial5 { "<" } else { "" };
            let three = if f.partial3 { ">" } else { "" };
            writeln!(w, "{five}{}\t{three}{}\t{}", f.start, f.end, f.kind)?;
            for (name, value) in f.qualifiers {
                writeln!(w, "\t\t\t{name}\t{value}")?;
            }
        }
    }
    Ok(())
}

/// The submitter's contact details and affiliation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub first: String,
    pub middle: String,
    pub last: String,
    pub email: String,
    pub phone: String,
    pub institution: String,
    pub department: String,
    pub street: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

/// A publication author.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Person {
    pub first: String,
    /// Middle names or period-separated initials, such as `Marie` or `M.R.`.
    pub middle: String,
    pub last: String,
}

impl From<&basecraft_protocol::submission::Author> for Person {
    fn from(author: &basecraft_protocol::submission::Author) -> Self {
        Self {
            first: author.first.clone(),
            middle: author.middle.clone(),
            last: author.last.clone(),
        }
    }
}

/// `Author.middle` historically held initials. Retain bare uppercase runs such
/// as MR; uppercase full middle words are inherently ambiguous in that legacy field.
fn compact_initials(part: &str) -> bool {
    part.chars().count() > 1 && part.chars().all(|c| c.is_alphabetic() && c.is_uppercase())
}

fn dotted_initials(initials: impl Iterator<Item = char>) -> String {
    let mut out = String::new();
    for initial in initials {
        out.push(initial);
        out.push('.');
    }
    out
}

/// Each spelled-out word contributes one initial. Periods separate abbreviated
/// initials; hyphenated given-name components retain their hyphen.
fn name_initials(name: &str, preserve_compact: bool) -> String {
    name.split_whitespace()
        .map(|word| {
            word.split('-')
                .map(|part| {
                    part.split('.')
                        .map(|component| {
                            if preserve_compact && compact_initials(component) {
                                dotted_initials(component.chars())
                            } else {
                                component
                                    .chars()
                                    .find(|c| c.is_alphabetic())
                                    .map_or_else(String::new, |initial| format!("{initial}."))
                            }
                        })
                        .collect::<String>()
                })
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect()
}

impl Person {
    /// NCBI initials from full given/middle names or dotted initials (`A.M.`, `J.-P.M.-C.`).
    pub fn initials(&self) -> String {
        name_initials(&self.first, false) + &name_initials(&self.middle, true)
    }

    /// Preserve spelled-out middle names in `Name-std.middle`. Initials-only
    /// input keeps its historical template shape (no redundant middle field).
    pub fn middle_name(&self) -> Option<&str> {
        self.middle
            .split(|c: char| c.is_whitespace() || c == '-' || c == '.')
            .any(|part| {
                !compact_initials(part) && part.chars().filter(|c| c.is_alphabetic()).count() > 1
            })
            .then_some(self.middle.as_str())
    }
}

/// An ASN.1 text string: double quotes are doubled.
fn asn(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn write_name(w: &mut dyn Write, person: &Person, indent: &str, comma: &str) -> io::Result<()> {
    writeln!(w, "{indent}name name {{")?;
    writeln!(w, "{indent}  last {},", asn(&person.last))?;
    writeln!(w, "{indent}  first {},", asn(&person.first))?;
    if let Some(middle) = person.middle_name() {
        writeln!(w, "{indent}  middle {},", asn(middle))?;
    }
    writeln!(w, "{indent}  initials {}", asn(&person.initials()))?;
    writeln!(w, "{indent}}}{comma}")
}

fn write_affil(w: &mut dyn Write, c: &Contact, email: bool, indent: &str) -> io::Result<()> {
    let mut fields = vec![("affil", &c.institution)];
    if !c.department.is_empty() {
        fields.push(("div", &c.department));
    }
    fields.extend([("city", &c.city)]);
    if !c.state.is_empty() {
        fields.push(("sub", &c.state));
    }
    fields.push(("country", &c.country));
    if !c.street.is_empty() {
        fields.push(("street", &c.street));
    }
    if email {
        fields.push(("email", &c.email));
        if !c.phone.is_empty() {
            fields.push(("phone", &c.phone));
        }
    }
    if !c.postal_code.is_empty() {
        fields.push(("postal-code", &c.postal_code));
    }
    writeln!(w, "{indent}affil std {{")?;
    for (i, (name, value)) in fields.iter().enumerate() {
        let comma = if i + 1 < fields.len() { "," } else { "" };
        writeln!(w, "{indent}  {name} {}{comma}", asn(value))?;
    }
    writeln!(w, "{indent}}}")
}

fn write_authors(w: &mut dyn Write, authors: &[Person], indent: &str) -> io::Result<()> {
    writeln!(w, "{indent}names std {{")?;
    for (i, author) in authors.iter().enumerate() {
        writeln!(w, "{indent}  {{")?;
        write_name(w, author, &format!("{indent}    "), "")?;
        let comma = if i + 1 < authors.len() { "," } else { "" };
        writeln!(w, "{indent}  }}{comma}")?;
    }
    write!(w, "{indent}}}")
}

/// Write a submission template: the contact, the authors with the
/// contact's affiliation, and an unpublished-citation descriptor with the
/// publication title.
pub fn write_sbt(
    w: &mut dyn Write,
    contact: &Contact,
    authors: &[Person],
    title: &str,
) -> io::Result<()> {
    write_sbt_details(w, contact, authors, title, authors, title)
}

/// Distinct direct-submission and unpublished-publication citations.
/// NCBI-Biblio Cit-sub has `descr`, not `title`; the submission title is
/// written there explicitly. Cit-gen.title holds the publication title.
pub fn write_sbt_details(
    w: &mut dyn Write,
    contact: &Contact,
    submission_authors: &[Person],
    submission_title: &str,
    publication_authors: &[Person],
    publication_title: &str,
) -> io::Result<()> {
    let person = Person {
        first: contact.first.clone(),
        middle: dotted_initials(contact.middle.chars().filter(|c| c.is_alphabetic())),
        last: contact.last.clone(),
    };
    writeln!(w, "Submit-block ::= {{")?;
    writeln!(w, "  contact {{")?;
    writeln!(w, "    contact {{")?;
    write_name(w, &person, "      ", ",")?;
    write_affil(w, contact, true, "      ")?;
    writeln!(w, "    }}")?;
    writeln!(w, "  }},")?;
    writeln!(w, "  cit {{")?;
    writeln!(w, "    authors {{")?;
    write_authors(w, submission_authors, "      ")?;
    writeln!(w, ",")?;
    write_affil(w, contact, false, "      ")?;
    writeln!(w, "    }},")?;
    writeln!(w, "    descr {}", asn(submission_title))?;
    writeln!(w, "  }},")?;
    writeln!(w, "  subtype new")?;
    writeln!(w, "}}")?;
    writeln!(w, "Seqdesc ::= pub {{")?;
    writeln!(w, "  pub {{")?;
    writeln!(w, "    gen {{")?;
    writeln!(w, "      cit \"unpublished\",")?;
    writeln!(w, "      authors {{")?;
    write_authors(w, publication_authors, "        ")?;
    writeln!(w)?;
    writeln!(w, "      }},")?;
    writeln!(w, "      title {}", asn(publication_title))?;
    writeln!(w, "    }}")?;
    writeln!(w, "  }}")?;
    writeln!(w, "}}")
}

/// One message from a table2asn `.val` report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValMessage {
    /// `Info`, `Warning`, `Error`, `Reject` or `Fatal`.
    pub severity: String,
    /// Such as `SEQ_INST.TerminalNs`.
    pub code: String,
    pub message: String,
    /// The local sequence ID the message names (`lcl|ID`), if any.
    pub seq_id: Option<String>,
}

const SEVERITIES: [&str; 5] = ["Info", "Warning", "Error", "Reject", "Fatal"];

/// Read a `.val` report: one message per `Severity: valid [CODE] text`
/// line; other lines continue the previous message.
pub fn parse_val(text: &str) -> Vec<ValMessage> {
    let mut out: Vec<ValMessage> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let parsed = line.split_once(": ").and_then(|(severity, rest)| {
            if !SEVERITIES.contains(&severity) {
                return None;
            }
            let rest = rest.strip_prefix("valid ").unwrap_or(rest);
            let (code, message) = rest
                .strip_prefix('[')
                .and_then(|r| r.split_once("] "))
                .unwrap_or(("", rest));
            Some(ValMessage {
                severity: severity.to_string(),
                code: code.to_string(),
                message: message.to_string(),
                seq_id: None,
            })
        });
        match (parsed, out.last_mut()) {
            (Some(message), _) => out.push(message),
            (None, Some(previous)) => {
                previous.message.push(' ');
                previous.message.push_str(line.trim());
            }
            (None, None) => out.push(ValMessage {
                severity: "Info".into(),
                code: String::new(),
                message: line.trim().to_string(),
                seq_id: None,
            }),
        }
    }
    for message in &mut out {
        message.seq_id = local_id(&message.message);
    }
    out
}

/// The first `lcl|ID` in a message.
fn local_id(text: &str) -> Option<String> {
    let start = text.find("lcl|")? + 4;
    let id: String = text[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ':' | ']' | ',' | ')'))
        .collect();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_names_and_initials_are_exported_without_expanding_words() {
        for (first, middle, initials, structured_middle) in [
            ("Anne", "Marie", "A.M.", Some("Marie")),
            ("Anne", "M.", "A.M.", None),
            ("Anne", "M.R.", "A.M.R.", None),
            ("Anne", "M R", "A.M.R.", None),
            (
                "Jean-Pierre",
                "Marie-Claire",
                "J.-P.M.-C.",
                Some("Marie-Claire"),
            ),
            ("J.-P.", "M.-C.", "J.-P.M.-C.", None),
            ("Jean Pierre", "Marie R.", "J.P.M.R.", Some("Marie R.")),
            ("Anne", "MR", "A.M.R.", None),
            ("Anne", "MARIE", "A.M.A.R.I.E.", None),
        ] {
            let person = Person {
                first: first.into(),
                middle: middle.into(),
                last: "van der Meer-Smith".into(),
            };
            assert_eq!(person.initials(), initials);
            assert_eq!(person.middle_name(), structured_middle);
            let mut bytes = Vec::new();
            write_name(&mut bytes, &person, "", "").unwrap();
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.contains(&format!("initials \"{initials}\"")), "{text}");
            if let Some(middle) = structured_middle {
                assert!(text.contains(&format!("middle \"{middle}\"")), "{text}");
            } else {
                assert!(!text.contains("middle "), "{text}");
            }
            assert!(text.contains("last \"van der Meer-Smith\""));
        }
    }

    #[test]
    fn contact_middle_initials_keep_their_compact_initials_meaning() {
        let mut bytes = Vec::new();
        write_sbt(
            &mut bytes,
            &Contact {
                first: "Jane".into(),
                middle: "MR".into(),
                last: "Doe".into(),
                ..Contact::default()
            },
            &[],
            "Study",
        )
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("initials \"J.M.R.\""));
        assert!(!text.contains("middle "));
    }

    #[test]
    fn distinct_citations_use_the_actual_asn_destinations() {
        let contact = Contact {
            first: "Contact".into(),
            last: "Person".into(),
            ..Contact::default()
        };
        let submission = vec![Person {
            first: "Anne".into(),
            middle: "M.".into(),
            last: "van der Meer".into(),
        }];
        let publication = vec![Person {
            first: "Bob".into(),
            middle: String::new(),
            last: "O'Neill-Smith".into(),
        }];
        let mut out = Vec::new();
        write_sbt_details(
            &mut out,
            &contact,
            &submission,
            "Sequence study",
            &publication,
            "Paper title",
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        let (submit, publication) = out.split_once("Seqdesc ::=").unwrap();
        assert!(submit.contains("descr \"Sequence study\""));
        assert!(submit.contains("van der Meer"));
        assert!(!submit.contains("O'Neill-Smith"));
        assert!(publication.contains("title \"Paper title\""));
        assert!(publication.contains("O'Neill-Smith"));
        assert!(!publication.contains("van der Meer"));
        assert!(submit.contains("Contact"), "Contact is independent");
    }

    #[test]
    fn writes_fsa_src_and_tbl() {
        let mods = vec![("location".to_string(), "chloroplast".to_string())];
        let mut out = Vec::new();
        let seq = vec![b'A'; 65];
        write_fsa(
            &mut out,
            &[FsaRecord {
                id: "s1",
                modifiers: &mods,
                residues: &seq,
            }],
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with(">s1 [location=chloroplast]\n"));
        assert_eq!(text.lines().nth(1).unwrap().len(), 60);
        let a = vec![
            ("organism".to_string(), "Caulerpa taxifolia".to_string()),
            ("isolate".to_string(), "x".to_string()),
        ];
        let b = vec![
            ("organism".to_string(), "Caulerpa sp. HV2".to_string()),
            ("specimen_voucher".to_string(), "HV2".to_string()),
        ];
        let mut out = Vec::new();
        write_src(&mut out, &[("s1", &a), ("s2", &b)]).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Sequence_ID\torganism\tisolate\tspecimen_voucher\ns1\tCaulerpa taxifolia\tx\t\ns2\tCaulerpa sp. HV2\t\tHV2\n"
        );
        let q = vec![("product".to_string(), "18S ribosomal RNA".to_string())];
        let mut out = Vec::new();
        write_tbl(
            &mut out,
            &[(
                "s1",
                vec![TableFeature {
                    kind: "rRNA",
                    start: 1,
                    end: 89,
                    partial5: true,
                    partial3: false,
                    qualifiers: &q,
                }],
            )],
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            ">Feature s1\n<1\t89\trRNA\n\t\t\tproduct\t18S ribosomal RNA\n"
        );
    }

    #[test]
    fn writes_sbt_with_quoted_strings() {
        let contact = Contact {
            first: "Jane".into(),
            last: "Doe".into(),
            email: "jane@example.org".into(),
            institution: "University of \"Melbourne\"".into(),
            city: "Melbourne".into(),
            country: "Australia".into(),
            ..Contact::default()
        };
        let authors = [Person {
            first: "Jane".into(),
            middle: "M.".into(),
            last: "Doe".into(),
        }];
        let mut out = Vec::new();
        write_sbt(&mut out, &contact, &authors, "Markers").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("affil \"University of \"\"Melbourne\"\"\""),
            "{text}"
        );
        assert!(text.contains("initials \"J.M.\""));
        assert!(text.contains("email \"jane@example.org\""));
        assert!(text.contains("title \"Markers\""));
        assert_eq!(text.matches('{').count(), text.matches('}').count());
    }

    #[test]
    fn parses_val_reports() {
        let text = "Warning: valid [SEQ_INST.TerminalNs] N at beginning of sequence BIOSEQ: lcl|seq2: raw, dna len= 89\nError: valid [SEQ_FEAT.InternalStop] 11 internal stops. Genetic code [11] FEATURE: CDS: x <1>  [lcl|seq1:<1->300] [lcl|seq1: raw, dna len= 307] -> [lcl|seq1_1]\n  continued\nError: valid [SEQ_DESCR.BadGeoLocNameCode] Bad geo_loc_name [Atlantis] DESCRIPTOR: BioSrc: x\n";
        let messages = parse_val(text);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].code, "SEQ_INST.TerminalNs");
        assert_eq!(messages[0].seq_id.as_deref(), Some("seq2"));
        assert_eq!(messages[1].severity, "Error");
        assert_eq!(messages[1].seq_id.as_deref(), Some("seq1"));
        assert!(messages[1].message.ends_with("continued"));
        assert_eq!(messages[2].seq_id, None);
    }
}
