"""NCBI genetic codes (translation tables) for plugins that read codons.

The same tables as basecraft's own (`basecraft-docs` genetic_code.rs): NCBI's
`ncbieaa` and `sncbieaa` strings, codons in TCAG order. A plugin that finds
or translates codons takes the code as an explicit parameter and looks it up
here; there is no default code.
"""

BASES = "TCAG"

#: NCBI number -> (name, amino acids, start codons), codons in TCAG order.
CODES = {
    1: ("Standard", "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------**--*----M---------------M----------------------------"),
    2: ("Vertebrate Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG", "----------**--------------------MMMM----------**---M------------"),
    3: ("Yeast Mitochondrial", "FFLLSSSSYY**CCWWTTTTPPPPHHQQRRRRIIMMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**----------------------MM---------------M------------"),
    4: ("Mold, Protozoan, and Coelenterate Mitochondrial; Mycoplasma/Spiroplasma", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--MM------**-------M------------MMMM---------------M------------"),
    5: ("Invertebrate Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSSSVVVVAAAADDEEGGGG", "---M------**--------------------MMMM---------------M------------"),
    6: ("Ciliate, Dasycladacean and Hexamita Nuclear", "FFLLSSSSYYQQCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------"),
    9: ("Echinoderm and Flatworm Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG", "-----------------------------------M---------------M------------"),
    10: ("Euplotid Nuclear", "FFLLSSSSYY**CCCWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "-----------------------------------M----------------------------"),
    11: ("Bacterial, Archaeal and Plant Plastid", "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------**--*----M------------MMMM---------------M------------"),
    12: ("Alternative Yeast Nuclear", "FFLLSSSSYY**CC*WLLLSPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "-------------------M---------------M----------------------------"),
    13: ("Ascidian Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSGGVVVVAAAADDEEGGGG", "---M------------------------------MM---------------M------------"),
    14: ("Alternative Flatworm Mitochondrial", "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG", "-----------------------------------M----------------------------"),
    16: ("Chlorophycean Mitochondrial", "FFLLSSSSYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "-----------------------------------M----------------------------"),
    21: ("Trematode Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNNKSSSSVVVVAAAADDEEGGGG", "-----------------------------------M---------------M------------"),
    22: ("Scenedesmus obliquus Mitochondrial", "FFLLSS*SYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "-----------------------------------M----------------------------"),
    23: ("Thraustochytrium Mitochondrial", "FF*LSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------------------------M--M---------------M------------"),
    24: ("Rhabdopleuridae Mitochondrial", "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG", "---M---------------M---------------M---------------M------------"),
    25: ("Candidate Division SR1 and Gracilibacteria", "FFLLSSSSYY**CCGWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M-------------------------------M---------------M------------"),
    26: ("Pachysolen tannophilus Nuclear", "FFLLSSSSYY**CC*WLLLAPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "-------------------M---------------M----------------------------"),
    29: ("Mesodinium Nuclear", "FFLLSSSSYYYYCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------"),
    30: ("Peritrich Nuclear", "FFLLSSSSYYEECC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------"),
    33: ("Cephalodiscidae Mitochondrial", "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG", "---M-------*-------M---------------M---------------M------------"),
}


#: The 64 codons in TCAG order, as the tables list them.
CODONS = [a + b + c for a in BASES for b in BASES for c in BASES]


def table(code: int) -> tuple[str, str, str]:
    """The table for NCBI number `code`; a code basecraft doesn't have is an error."""
    try:
        return CODES[code]
    except KeyError:
        raise ValueError(f"genetic code {code} is not supported") from None


def stop_codons(code: int) -> list[str]:
    """The stop codons of genetic code `code`."""
    amino_acids = table(code)[1]
    return [codon for codon, amino_acid in zip(CODONS, amino_acids, strict=True) if amino_acid == "*"]
