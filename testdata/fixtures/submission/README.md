# GenBank marker submission fixtures

Small alignments and a specimen sheet for submission tests (SCI-0012).
Sequences come from public GenBank records, changed as noted:

- `rbcL.afa`: plastid rbcL fragment (Caulerpa), genetic code 11, reading frame
  codon_start 2. HV102 has terminal `NN`, IUPAC `R` and `Y`, `?` and a ragged
  end; columns 201-203 are unknown in every new row (masked). HV103 lacks one
  base (a frameshift: no clean frame). REF_rbcL stands for a downloaded
  reference with an existing accession.
- `COI.afa`: Drosophila melanogaster COI (MW122888, MT807020), invertebrate
  mitochondrial code 5, codon_start 2 (TGA codes tryptophan). HV203 has one
  substitution.
- `18S.afa`: Caulerpa webbiana 18S rRNA (AY206426, bases 1-383).
- `ITS.afa`: Caulerpa ITS regions (QB163437, PV171516). Two rows are both
  named HV403_ITS; orphan_ITS has no specimen.
- `specimens.csv`: Darwin Core columns with `document` and `record` links.
  HV203 has the missing-value reason `not collected`; HV204 has no date;
  HV302 is identified only as `Caulerpa sp.`; HV404's country is `Atlantis`,
  which table2asn rejects.
