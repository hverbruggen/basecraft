//! NCBI genetic codes (translation tables) and codon translation with IUPAC
//! ambiguity.
//!
//! Tables are NCBI's `ncbieaa` and `sncbieaa` strings, codons in TCAG
//! order. Codes whose stop codons depend on context (27, 28, 31) are not
//! offered. Every table here was checked against table2asn's translations
//! (SCI-0012 validation notes).

use basecraft_protocol::Feature;

/// One NCBI translation table.
#[derive(Clone, Copy, Debug)]
pub struct GeneticCode {
    pub id: u8,
    pub name: &'static str,
    amino_acids: &'static [u8; 64],
    starts: &'static [u8; 64],
}

macro_rules! code {
    ($id:expr, $name:expr, $aa:expr, $starts:expr) => {
        GeneticCode {
            id: $id,
            name: $name,
            amino_acids: $aa,
            starts: $starts,
        }
    };
}

/// The supported tables, by NCBI number.
pub const CODES: &[GeneticCode] = &[
    code!(
        1,
        "Standard",
        b"FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"---M------**--*----M---------------M----------------------------"
    ),
    code!(
        2,
        "Vertebrate Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG",
        b"----------**--------------------MMMM----------**---M------------"
    ),
    code!(
        3,
        "Yeast Mitochondrial",
        b"FFLLSSSSYY**CCWWTTTTPPPPHHQQRRRRIIMMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"----------**----------------------MM---------------M------------"
    ),
    code!(
        4,
        "Mold, Protozoan, and Coelenterate Mitochondrial; Mycoplasma/Spiroplasma",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"--MM------**-------M------------MMMM---------------M------------"
    ),
    code!(
        5,
        "Invertebrate Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSSSVVVVAAAADDEEGGGG",
        b"---M------**--------------------MMMM---------------M------------"
    ),
    code!(
        6,
        "Ciliate, Dasycladacean and Hexamita Nuclear",
        b"FFLLSSSSYYQQCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"--------------*--------------------M----------------------------"
    ),
    code!(
        9,
        "Echinoderm and Flatworm Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        b"-----------------------------------M---------------M------------"
    ),
    code!(
        10,
        "Euplotid Nuclear",
        b"FFLLSSSSYY**CCCWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"-----------------------------------M----------------------------"
    ),
    code!(
        11,
        "Bacterial, Archaeal and Plant Plastid",
        b"FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"---M------**--*----M------------MMMM---------------M------------"
    ),
    code!(
        12,
        "Alternative Yeast Nuclear",
        b"FFLLSSSSYY**CC*WLLLSPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"-------------------M---------------M----------------------------"
    ),
    code!(
        13,
        "Ascidian Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSGGVVVVAAAADDEEGGGG",
        b"---M------------------------------MM---------------M------------"
    ),
    code!(
        14,
        "Alternative Flatworm Mitochondrial",
        b"FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        b"-----------------------------------M----------------------------"
    ),
    code!(
        16,
        "Chlorophycean Mitochondrial",
        b"FFLLSSSSYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"-----------------------------------M----------------------------"
    ),
    code!(
        21,
        "Trematode Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        b"-----------------------------------M---------------M------------"
    ),
    code!(
        22,
        "Scenedesmus obliquus Mitochondrial",
        b"FFLLSS*SYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"-----------------------------------M----------------------------"
    ),
    code!(
        23,
        "Thraustochytrium Mitochondrial",
        b"FF*LSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"--------------------------------M--M---------------M------------"
    ),
    code!(
        24,
        "Rhabdopleuridae Mitochondrial",
        b"FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG",
        b"---M---------------M---------------M---------------M------------"
    ),
    code!(
        25,
        "Candidate Division SR1 and Gracilibacteria",
        b"FFLLSSSSYY**CCGWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"---M-------------------------------M---------------M------------"
    ),
    code!(
        26,
        "Pachysolen tannophilus Nuclear",
        b"FFLLSSSSYY**CC*WLLLAPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"-------------------M---------------M----------------------------"
    ),
    code!(
        29,
        "Mesodinium Nuclear",
        b"FFLLSSSSYYYYCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"--------------*--------------------M----------------------------"
    ),
    code!(
        30,
        "Peritrich Nuclear",
        b"FFLLSSSSYYEECC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        b"--------------*--------------------M----------------------------"
    ),
    code!(
        33,
        "Cephalodiscidae Mitochondrial",
        b"FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG",
        b"---M-------*-------M---------------M---------------M------------"
    ),
];

/// The table with NCBI number `id`.
pub fn code(id: u8) -> Option<&'static GeneticCode> {
    CODES.iter().find(|c| c.id == id)
}

/// The code GenBank assumes when a record says nothing: the standard code.
/// Shown only as a default, never used for a saved result (rule 4.2).
pub const DEFAULT: u8 = 1;

/// Where a genetic code came from, highest precedence first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeSource {
    /// Given by the person or the command.
    Explicit,
    /// The feature's `transl_table` qualifier.
    Feature,
    /// The `transl_table` every CDS of the record states.
    Record,
    /// Nothing states a code: [`DEFAULT`].
    Default,
}

impl CodeSource {
    /// How a display names the source: "default" when it is the default.
    pub fn label(self) -> &'static str {
        match self {
            CodeSource::Explicit => "chosen",
            CodeSource::Feature => "from the feature",
            CodeSource::Record => "from the record's CDS",
            CodeSource::Default => "default",
        }
    }
}

/// The genetic code to translate with and where it came from, in the one
/// order of precedence: the code given, the feature's `transl_table`, the
/// `transl_table` every CDS of the record states, else [`DEFAULT`]. A
/// stated code basecraft doesn't support is an error, not a fallback.
pub fn resolve(
    explicit: Option<u8>,
    feature: Option<&Feature>,
    record: &[&Feature],
) -> Result<(&'static GeneticCode, CodeSource), String> {
    let stated = |feature: &Feature| {
        feature.qualifier("transl_table").map(|value| {
            value
                .trim()
                .parse::<u8>()
                .map_err(|_| format!("transl_table {value:?} is not a genetic code number"))
        })
    };
    let (id, source) = if let Some(id) = explicit {
        (id, CodeSource::Explicit)
    } else if let Some(id) = feature.and_then(stated) {
        (id?, CodeSource::Feature)
    } else if let Some(id) = record_code(record)? {
        (id, CodeSource::Record)
    } else {
        (DEFAULT, CodeSource::Default)
    };
    code(id)
        .map(|code| (code, source))
        .ok_or_else(|| format!("genetic code {id} is not supported"))
}

/// The `transl_table` the record's CDS features state, when they all state
/// the same one. CDS features without it, as GenBank allows for the
/// standard code, make the record's code unknown.
fn record_code(features: &[&Feature]) -> Result<Option<u8>, String> {
    let mut found = None;
    for feature in features.iter().filter(|f| f.kind == "CDS") {
        let Some(value) = feature.qualifier("transl_table") else {
            return Ok(None);
        };
        let id = value
            .trim()
            .parse::<u8>()
            .map_err(|_| format!("transl_table {value:?} is not a genetic code number"))?;
        if found.is_some_and(|other| other != id) {
            return Ok(None);
        }
        found = Some(id);
    }
    Ok(found)
}

/// The unambiguous bases (as TCAG indices) an IUPAC nucleotide code stands
/// for. `None` for anything else.
fn bases(b: u8) -> Option<&'static [u8]> {
    Some(match b.to_ascii_uppercase() {
        b'T' | b'U' => &[0],
        b'C' => &[1],
        b'A' => &[2],
        b'G' => &[3],
        b'R' => &[2, 3],
        b'Y' => &[0, 1],
        b'S' => &[1, 3],
        b'W' => &[0, 2],
        b'K' => &[0, 3],
        b'M' => &[1, 2],
        b'B' => &[0, 1, 3],
        b'D' => &[0, 2, 3],
        b'H' => &[0, 1, 2],
        b'V' => &[1, 2, 3],
        b'N' | b'?' => &[0, 1, 2, 3],
        _ => return None,
    })
}

/// Every codon index an IUPAC codon stands for, or `None` if a character
/// is not a nucleotide code.
fn expand(codon: &[u8]) -> Option<Vec<usize>> {
    let [a, b, c] = codon else { return None };
    let mut out = Vec::new();
    for &x in bases(*a)? {
        for &y in bases(*b)? {
            for &z in bases(*c)? {
                out.push(usize::from(x) * 16 + usize::from(y) * 4 + usize::from(z));
            }
        }
    }
    Some(out)
}

impl GeneticCode {
    /// The amino acid a codon codes for: `*` for stop, `X` when its
    /// ambiguity codes allow different amino acids or it is not a codon.
    pub fn translate_codon(&self, codon: &[u8]) -> u8 {
        let Some(indices) = expand(codon) else {
            return b'X';
        };
        let first = self.amino_acids[indices[0]];
        if indices.iter().all(|&i| self.amino_acids[i] == first) {
            first
        } else {
            b'X'
        }
    }

    /// Whether every reading of the codon is a stop codon.
    pub fn is_stop(&self, codon: &[u8]) -> bool {
        self.translate_codon(codon) == b'*'
    }

    /// Whether every reading of the codon is a start codon in this table.
    pub fn is_start(&self, codon: &[u8]) -> bool {
        expand(codon).is_some_and(|indices| indices.iter().all(|&i| self.starts[i] == b'M'))
    }

    /// Translate `seq` from its first base, codon by codon; a trailing
    /// incomplete codon is left out.
    pub fn translate(&self, seq: &[u8]) -> Vec<u8> {
        seq.as_chunks::<3>()
            .0
            .iter()
            .map(|codon| self.translate_codon(codon))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_well_formed_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for code in CODES {
            assert!(seen.insert(code.id));
            assert!(
                code.amino_acids
                    .iter()
                    .all(|a| a.is_ascii_uppercase() || *a == b'*')
            );
            assert!(code.starts.iter().all(|s| b"-M*".contains(s)));
        }
        assert!(code(7).is_none() && code(27).is_none());
    }

    #[test]
    fn translates_with_ambiguity() {
        let standard = code(1).unwrap();
        assert_eq!(standard.translate(b"ATGGCNTAA"), b"MA*");
        // TAR is TAA or TAG: a stop either way. TRA is TAA or TGA.
        assert!(standard.is_stop(b"TAR"));
        assert!(standard.is_stop(b"TRA"));
        assert_eq!(standard.translate_codon(b"AAN"), b'X');
        assert_eq!(standard.translate_codon(b"AT-"), b'X');
        assert!(standard.is_start(b"ATG") && !standard.is_start(b"ATA"));
        // TGA is tryptophan in the invertebrate mitochondrial code, AGA serine.
        let invertebrate = code(5).unwrap();
        assert_eq!(invertebrate.translate(b"TGAAGA"), b"WS");
        assert!(invertebrate.is_start(b"ATA"));
        // Plastid code: GTG starts, TGA stops.
        let plastid = code(11).unwrap();
        assert!(plastid.is_start(b"GTG") && plastid.is_stop(b"TGA"));
    }

    fn cds(table: Option<&str>) -> Feature {
        Feature {
            kind: "CDS".into(),
            location: basecraft_protocol::Location::simple(
                0,
                3,
                basecraft_protocol::Strand::Forward,
            ),
            qualifiers: table
                .map(|t| basecraft_protocol::Qualifier::new("transl_table", t))
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn resolve_follows_one_order_of_precedence() {
        let four = cds(Some("4"));
        let eleven = cds(Some("11"));
        let plain = cds(None);
        let id = |r: Result<(&GeneticCode, CodeSource), String>| r.map(|(c, s)| (c.id, s));
        let both = [&eleven, &eleven];
        assert_eq!(
            id(resolve(Some(2), Some(&four), &both)),
            Ok((2, CodeSource::Explicit))
        );
        assert_eq!(
            id(resolve(None, Some(&four), &both)),
            Ok((4, CodeSource::Feature))
        );
        assert_eq!(
            id(resolve(None, Some(&plain), &both)),
            Ok((11, CodeSource::Record))
        );
        // CDS that disagree, or one that states nothing, leave the record's
        // code unknown: the default, labelled as such.
        for record in [vec![&four, &eleven], vec![&eleven, &plain], vec![]] {
            assert_eq!(
                id(resolve(None, None, &record)),
                Ok((DEFAULT, CodeSource::Default))
            );
        }
        assert_eq!(CodeSource::Default.label(), "default");
        assert!(resolve(Some(7), None, &[]).is_err());
        assert!(resolve(None, Some(&cds(Some("x"))), &[]).is_err());
        assert!(resolve(None, None, &[&cds(Some("99"))]).is_err());
    }
}
