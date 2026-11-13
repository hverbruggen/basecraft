//! Parse additions before creating a typed alignment operation.

pub fn parse_sequences(text: &str, name: &str) -> Result<Vec<(String, String)>, String> {
    let text = text.trim();
    let mut rows: Vec<(String, String)> = Vec::new();
    if text.starts_with('>') {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if let Some(header) = line.strip_prefix('>') {
                let name = header.split_whitespace().next().ok_or("Empty FASTA name")?;
                rows.push((name.into(), String::new()));
            } else {
                rows.last_mut()
                    .ok_or("Sequence before FASTA header")?
                    .1
                    .extend(line.chars().filter(|c| !c.is_whitespace()));
            }
        }
    } else {
        rows.push((
            name.trim().into(),
            text.chars().filter(|c| !c.is_whitespace()).collect(),
        ));
    }
    let mut names = std::collections::HashSet::new();
    let mut duplicates = std::collections::BTreeSet::new();
    for (name, seq) in &mut rows {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err("Use unique sequence names without spaces".into());
        }
        if !names.insert(name.clone()) {
            duplicates.insert(name.clone());
        }
        *seq = seq.to_ascii_uppercase().replace('.', "-");
        if seq.is_empty()
            || !seq
                .bytes()
                .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'*')
        {
            return Err(format!("Invalid or empty sequence: {name}"));
        }
    }
    if !duplicates.is_empty() {
        return Err(format!(
            "Repeated sequence IDs in the input:\n\n{}",
            duplicates
                .into_iter()
                .map(|n| format!("• {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if rows.is_empty() {
        return Err("Enter at least one sequence".into());
    }
    Ok(rows)
}

/// Why sequences cannot be added to an alignment.
#[derive(Debug, thiserror::Error)]
pub enum AdditionError {
    /// The text is not sequences; the reason as `parse_sequences` gives it.
    #[error("{0}")]
    Unreadable(String),
    #[error(transparent)]
    OutsideAlphabet(#[from] basecraft_docs::alignment::EditError),
    #[error("These sequence IDs are already in the alignment:\n\n{}", bullets(.0))]
    NamesTaken(Vec<String>),
    #[error("Select one reviewed specimen at a time")]
    NotOneSpecimen,
    /// The accepted review could not be kept as the addition's evidence.
    #[error("{0}")]
    Evidence(String),
}

fn bullets(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("• {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The rows `text` (FASTA, or one sequence named `name`) adds to `alignment`,
/// refused if a symbol is outside its alphabet or a name is already a row.
pub fn addition_rows(
    alignment: &basecraft_docs::alignment::AlignmentDoc,
    text: &str,
    name: &str,
) -> Result<Vec<(String, String)>, AdditionError> {
    let rows = parse_sequences(text, name).map_err(AdditionError::Unreadable)?;
    alignment.validate_additions(&rows)?;
    let taken = alignment.duplicate_sequence_names(&rows);
    if taken.is_empty() {
        Ok(rows)
    } else {
        Err(AdditionError::NamesTaken(taken))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_fasta_identifiers_are_listed() {
        let error = parse_sequences(">one\nAC\n>two\nGT\n>one\nAA\n>two\nCC", "").unwrap_err();
        assert!(error.contains("• one") && error.contains("• two"));
    }
    #[test]
    fn pasted_fasta_and_raw_sequence() {
        assert_eq!(
            parse_sequences(">a description\nac g\n>b\ntt\n", "").unwrap(),
            vec![("a".into(), "ACG".into()), ("b".into(), "TT".into())]
        );
        assert_eq!(
            parse_sequences("ac gt", "sample").unwrap(),
            vec![("sample".into(), "ACGT".into())]
        );
        assert!(parse_sequences(">a\nAC\n>a\nTT", "").is_err());
        assert!(parse_sequences("123", "a").is_err());
        assert!(parse_sequences(">a\n", "").is_err());
    }
}
