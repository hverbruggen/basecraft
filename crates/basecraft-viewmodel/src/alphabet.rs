//! How the window names a document's alphabet (D12), honestly: what was
//! stated or guessed, or that basecraft cannot tell, or that the sequences
//! are of more than one kind.
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

/// What to ask when basecraft cannot tell what `file` is, or None.
pub fn question(fact: &Fact<Alphabet>, file: &str) -> Option<String> {
    (fact.value == Alphabet::Undetermined).then(|| {
        format!(
            "Basecraft can't tell whether {file} is DNA, RNA or protein: it has a few letters only \
             proteins use. Say what it is; tools that need to know refuse until you do."
        )
    })
}

/// The alphabet a user states, from the dialog's answer.
pub fn stated(answer: &str) -> Option<StatedAlphabet> {
    Some(match answer {
        "dna" => StatedAlphabet::Dna,
        "rna" => StatedAlphabet::Rna,
        "protein" => StatedAlphabet::Protein,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(value: Alphabet, source: FactSource) -> Fact<Alphabet> {
        Fact { value, source }
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
    fn only_an_undetermined_document_asks() {
        let asked = question(
            &fact(Alphabet::Undetermined, FactSource::Guessed),
            "a.fasta",
        );
        assert!(asked.unwrap().contains("a.fasta"));
        for value in [Alphabet::Dna, Alphabet::Mixed, Alphabet::Unknown] {
            assert!(question(&fact(value, FactSource::Guessed), "a.fasta").is_none());
        }
        assert_eq!(stated("rna"), Some(StatedAlphabet::Rna));
        assert_eq!(stated("mixed"), None);
    }
}
