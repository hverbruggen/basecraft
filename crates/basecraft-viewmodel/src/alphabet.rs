//! How the window names a document's alphabet (D12), honestly: what was
//! stated or guessed, or that basecraft cannot tell, or that the sequences
//! are of more than one kind.
use basecraft_docs::{AlignmentDoc, SequenceDoc};
use basecraft_protocol::facts::{Alphabet, Fact, FactSource, StatedAlphabet};

/// The alphabet as the status line names it.
pub fn text(fact: &Fact<Alphabet>) -> String {
    let name = match fact.value {
        Alphabet::Dna => "DNA",
        Alphabet::Rna => "RNA",
        Alphabet::Protein => "protein",
        Alphabet::Unknown => "sequence type unknown",
        Alphabet::Undetermined => "sequence type undetermined",
        Alphabet::Mixed => "mixed sequence types",
    };
    let guessed = fact.source == FactSource::Guessed
        && matches!(
            fact.value,
            Alphabet::Dna | Alphabet::Rna | Alphabet::Protein
        );
    if guessed {
        format!("{name} (guessed)")
    } else {
        name.to_owned()
    }
}

/// What to ask when basecraft can't tell what some of an alignment's rows
/// are: the document's own refusal, which names them. Rows of clear but
/// different kinds are reported, not asked about; an answer, including
/// "don't ask again" (D22), asks nothing.
pub fn alignment_question(doc: &AlignmentDoc) -> Option<String> {
    let unclear = doc.row_alphabets().contains(&Alphabet::Undetermined);
    (doc.stated_alphabet().is_none() && unclear).then(|| doc.alphabet_refusal())?
}

/// [`alignment_question`] for a single sequence.
pub fn sequence_question(doc: &SequenceDoc) -> Option<String> {
    let unclear = doc.alphabet().value == Alphabet::Undetermined;
    (doc.stated_alphabet().is_none() && unclear).then(|| doc.alphabet_refusal())?
}

/// The edit that states the alphabet the user chose in the dialog.
pub fn stated(answer: &str) -> Option<basecraft_protocol::Op> {
    let alphabet = match answer {
        "dna" => StatedAlphabet::Dna,
        "rna" => StatedAlphabet::Rna,
        "protein" => StatedAlphabet::Protein,
        "undetermined" => StatedAlphabet::Undetermined,
        _ => return None,
    };
    Some(basecraft_protocol::Op::SetAlphabet { alphabet })
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_docs::alignment::AlnRow;
    use std::sync::Arc;

    fn fact(value: Alphabet, source: FactSource) -> Fact<Alphabet> {
        Fact { value, source }
    }
    fn alignment(rows: &[&str]) -> AlignmentDoc {
        AlignmentDoc::new(
            "t",
            rows.iter()
                .enumerate()
                .map(|(i, seq)| AlnRow::new(format!("r{i}"), Arc::new(seq.as_bytes().to_vec())))
                .collect(),
        )
    }

    #[test]
    fn undetermined_and_mixed_are_named_as_such_and_guesses_say_so() {
        assert_eq!(
            text(&fact(Alphabet::Dna, FactSource::Guessed)),
            "DNA (guessed)"
        );
        assert_eq!(text(&fact(Alphabet::Protein, FactSource::User)), "protein");
        assert_eq!(
            text(&fact(Alphabet::Undetermined, FactSource::Guessed)),
            "sequence type undetermined"
        );
        assert_eq!(
            text(&fact(Alphabet::Mixed, FactSource::Guessed)),
            "mixed sequence types"
        );
    }

    #[test]
    fn one_undetermined_row_among_dna_rows_asks_and_names_it() {
        let dna = "ACGT".repeat(50);
        let unclear = format!("{}E{}", "ACGT".repeat(25), "ACGT".repeat(25));
        let doc = alignment(&[&dna, &dna, &unclear]);
        assert_eq!(doc.alphabet().value, Alphabet::Mixed);
        let asked = alignment_question(&doc).unwrap();
        assert!(asked.contains("r2") && !asked.contains("r0"), "{asked}");
        // Clear rows of different kinds are reported, not asked about.
        let protein = "MKLVEEFPQ".repeat(20);
        assert!(alignment_question(&alignment(&[&dna, &protein])).is_none());
        assert!(alignment_question(&alignment(&[&dna, &dna])).is_none());
        assert_eq!(
            stated("rna"),
            Some(basecraft_protocol::Op::SetAlphabet {
                alphabet: StatedAlphabet::Rna
            })
        );
        assert_eq!(stated("mixed"), None);
    }

    #[test]
    fn a_declined_question_is_not_asked_again_but_still_refuses() {
        let unclear = format!("{}E{}", "ACGT".repeat(25), "ACGT".repeat(25));
        let mut doc = alignment(&[&unclear]);
        assert!(alignment_question(&doc).is_some());
        let Some(basecraft_protocol::Op::SetAlphabet { alphabet }) = stated("undetermined") else {
            panic!("no declined answer");
        };
        doc.set_alphabet(Some(fact(alphabet.into(), FactSource::User)));
        assert_eq!(alignment_question(&doc), None);
        assert!(doc.alphabet_refusal().is_some());
    }
}
