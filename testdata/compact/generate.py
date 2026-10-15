#!/usr/bin/env python3
"""Small deterministic synthetic fixtures; no downloaded or private biological data.

Use --check to compare the committed suite; default output is the external cache.
Existing exports are never overwritten. See README.md for construction/limitations.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

SEED = 1001
FIXTURES = Path(__file__).resolve().parents[1] / 'fixtures' / 'compact'
IDS = [f'specimen_{n:02}' for n in range(1, 9)]
# A designed coding template, not a claimed natural gene or functional protein.
CODONS = ('ATG GCT GAA CTG AAA GGT ATT GTT GAC ACC TTC CAG AAC CGT TCT '
          'GCT GGT CTG GAA GTT AAA ACC GAC ATT TTC AAC CAG TCT CGT GCT '
          'GAA CTG GGT GAC AAA GTT ACC ATT CAG TTC AAC GCT TCT CGT GGT '
          'CTG GAA AAA GAC GTT ATT ACC TTC CAG AAC TCT GCT CGT GGT TAA').split()
SYNONYMS = {'GCT': 'GCC', 'GAA': 'GAG', 'CTG': 'CTA', 'AAA': 'AAG',
            'GGT': 'GGC', 'ATT': 'ATC', 'GTT': 'GTC', 'GAC': 'GAT',
            'ACC': 'ACT', 'TTC': 'TTT', 'CAG': 'CAA', 'AAC': 'AAT',
            'CGT': 'CGC', 'TCT': 'TCC'}
# Explicit dictionary avoids relying on an application translation implementation.
AMINO = {'ATG': 'M', 'GCT': 'A', 'GAA': 'E', 'CTG': 'L', 'AAA': 'K',
         'GGT': 'G', 'ATT': 'I', 'GTT': 'V', 'GAC': 'D', 'ACC': 'T',
         'TTC': 'F', 'CAG': 'Q', 'AAC': 'N', 'CGT': 'R', 'TCT': 'S', 'TAA': '*'}


def reverse_complement(seq):
    return seq.translate(str.maketrans('ACGTN', 'TGCAN'))[::-1]


def fasta(rows, width=60, newline='\n'):
    return ''.join('>' + name + newline + newline.join(
        seq[i:i + width] for i in range(0, len(seq), width)) + newline
        for name, seq in rows)


def abif(calls, quality=True, bad_peak=False):
    """Minimal processed trace: triangular peaks, 12 samples/call, GATC wheel.

    ABIF July 2006 specification: 128-byte header, 28-byte big-endian entries,
    inline payloads <=4 bytes. This is a format fixture, not empirical evidence.
    """
    samples = 12 * (len(calls) + 1)
    positions = [12 * (i + 1) for i in range(len(calls))]
    entries = [(b'FWO_', 1, 2, 1, b'GATC')]
    for number, base in enumerate('GATC', 9):
        channel = [0] * samples
        for i, call in enumerate(calls):
            if call == base or call == 'N':
                amplitude = 240 if i in (0, len(calls) - 1) or call == 'N' else 900
                for delta in range(-4, 5):
                    channel[positions[i] + delta] = amplitude * (5 - abs(delta)) // 5
        entries.append((b'DATA', number, 4, 2, struct.pack(f'>{samples}h', *channel)))
    entries += [(b'PBAS', 2, 2, 1, calls.encode()),
                (b'PLOC', 2, 4, 2, struct.pack(f'>{len(calls)}h',
                 *(positions[:-1] + [samples] if bad_peak else positions)))]
    if quality:
        entries.append((b'PCON', 2, 2, 1, bytes(
            8 if i in (0, len(calls) - 1) or c == 'N' else 35
            for i, c in enumerate(calls))))
    offset = 128 + 28 * len(entries)
    directory, payload = b'', b''
    for tag, number, kind, width, data in entries:
        address = data.ljust(4, b'\0') if len(data) <= 4 else struct.pack('>I', offset)
        directory += struct.pack('>4sIHHII', tag, number, kind, width,
                                 len(data) // width, len(data)) + address + b'\0' * 4
        if len(data) > 4:
            payload += data
            offset += len(data)
    root = struct.pack('>4sIHHIIII', b'tdir', 1, 1023, 28, len(entries),
                       len(directory), 128, 0)
    return (b'ABIF' + struct.pack('>H', 101) + root).ljust(128, b'\0') + directory + payload


def suite():
    files, descriptions = {}, {}

    def add(path, content, expected, kind='synthetic-valid'):
        files[path] = content.encode() if isinstance(content, str) else content
        descriptions[path] = {'kind': kind, 'expected': expected}

    ancestor = ''.join(CODONS)
    # Rank positions by a specified digest; independent of Python PRNG versions.
    positions = sorted(range(1, 59), key=lambda p: hashlib.sha256(
        f'{SEED}:{p}'.encode()).digest())
    rows = []
    for i, name in enumerate(IDS):
        group = 0 if i < 2 else 1 if i < 5 else 2
        codons = CODONS.copy()
        for pos in positions[group * 8:group * 8 + 8]:
            codons[pos] = SYNONYMS[codons[pos]]
        # Two exact repeated haplotypes; other leaves have a private substitution.
        if i in (2, 3, 4, 5):
            pos = positions[24 + i]
            codons[pos] = SYNONYMS[codons[pos]]
        if group == 2:
            codons[40] = '---'  # In-frame inherited one-codon deletion.
        rows.append((name, ''.join(codons)))
    protein = ''.join(AMINO[c] for c in CODONS[:-1])
    intron = 'GTAAGT' + 'ATCTTAACCT' * 2 + 'CAG'
    spacer = 'TATAAAGCGCTATATCGGCTAACCTAGGTTAC'
    # Joined CDS split after 61 nt: second segment has GFF phase 2.
    genome = ancestor[:61] + intron + ancestor[61:] + spacer + reverse_complement(ancestor)
    end_join = 180 + len(intron)
    reverse_start = end_join + len(spacer)
    gff = (f'##gff-version 3\n##sequence-region locus 1 {len(genome)}\n'
           'locus\tsimulation\tCDS\t1\t61\t.\t+\t0\tID=joined;gene=toyA\n'
           f'locus\tsimulation\tCDS\t{62 + len(intron)}\t{end_join}\t.\t+\t2\tID=joined;gene=toyA\n'
           f'locus\tsimulation\tCDS\t{reverse_start + 1}\t{len(genome)}\t.\t-\t0\tID=reverse;gene=toyB\n')
    gb = (f'LOCUS       locus                    {len(genome)} bp    DNA     linear SYN 15-OCT-2026\n'
          'DEFINITION  Synthetic joined and reverse coding example; no natural accession.\n'
          'FEATURES             Location/Qualifiers\n'
          f'     CDS             join(1..61,{62 + len(intron)}..{end_join})\n'
          '                     /gene="toyA"\n                     /codon_start=1\n'
          f'     CDS             complement({reverse_start + 1}..{len(genome)})\n'
          '                     /gene="toyB"\n                     /codon_start=1\nORIGIN\n' +
          ''.join(f'{i + 1:9} {genome[i:i + 60].lower()}\n' for i in range(0, len(genome), 60)) + '//\n')
    add('sequences-annotations/coding.fasta', fasta([('ancestor', ancestor)]), '180 nt, start ATG, stop TAA, 59 amino acids')
    add('sequences-annotations/protein.fasta', fasta([('toy_peptide', protein)]), '59 aa; translation of coding.fasta excluding stop')
    add('sequences-annotations/noncoding.fasta', fasta([('designed_spacer', spacer)]), '32 nt designed intergenic sequence; no functional claim')
    add('sequences-annotations/locus.fasta', fasta([('locus', genome)]), '421 nt, same residues as locus.gb')
    add('sequences-annotations/locus.gff3', gff, 'Joined forward CDS, phase 0/2; reverse CDS reaches last base')
    add('sequences-annotations/locus.gb', gb, 'Two CDS encode the ancestor; GenBank and GFF3 locations agree')
    add('alignment-editing/homologues.afa', fasta(rows), '8 rows x 180 columns, shared branch changes, codon deletion, 6 haplotypes')
    add('alignment-editing/unaligned.fasta', fasta([(n, s.replace('-', '')) for n, s in rows]), 'MAFFT input; first five 180 nt, last three 177 nt')
    add('alignment-editing/ambiguous-gappy.afa', fasta([(n, s[:30] + '---' + ('N' if i == 0 else s[30]) + s[31:]) for i, (n, s) in enumerate(rows)]), '183 columns; all-gap columns 30..33; specimen_01 N at column 33; edge data', 'synthetic-edge')
    add('alignment-editing/identifiers.fasta', fasta([('sample_α', ancestor), ('sample_α', ancestor)], width=17, newline='\r\n'), 'Duplicate Unicode IDs preserved by IO; analysis must reject duplicates', 'synthetic-edge')
    add('tree-inference/known-distances.afa', '>a\nACGTACGTACGT\n>b\nACGTACGTACGA\n>c\nTCGTACGTACGA\n', 'Analytical toy: p distances ab=1/12, bc=1/12, ac=2/12; not realism', 'analytical')
    add('tree-inference/informative.afa', fasta(rows), 'Same eight specimen IDs; infer topology/branch lengths without treating groups as species truth')
    time_tree = '((specimen_01:0.1,specimen_02:0.1):0.9,((specimen_03:0.1,specimen_04:0.1,specimen_05:0.1):0.4,(specimen_06:0.1,specimen_07:0.1,specimen_08:0.1):0.4):0.5);\n'
    add('species-delimitation/specimens.afa', fasta(rows), 'Eight matching specimens; exact haplotype multiplicities 2,1,1,1,1,2')
    add('species-delimitation/time.nwk', time_tree, 'Rooted stipulated time units; all root-to-tip distances 1 (tolerance 1e-12), polytomies; GMYC may reject ties')
    # Strict binary, distinct branching times for the normal GMYC workflow.
    binary = '((specimen_01:0.03,specimen_02:0.03):0.97,((specimen_03:0.08,(specimen_04:0.02,specimen_05:0.02):0.06):0.42,(specimen_06:0.09,(specimen_07:0.01,specimen_08:0.01):0.08):0.41):0.5);\n'
    add('species-delimitation/time-binary.nwk', binary, 'Rooted ultrametric, height 1; seven distinct positive branching times; no promised species count')
    subst_tree = binary.replace('specimen_03:0.08', 'specimen_03:0.18')
    add('species-delimitation/substitutions.nwk', subst_tree, 'Rooted bifurcating tree, stipulated substitutions/site; intentionally non-ultrametric; passes rooted substitution-tree validation, not GMYC input')
    spart = ('begin spart;\nproject_name = synthetic_demo;\ndate = 2026-10-15T00:00:00;\n'
             'n_spartitions = 2 : construction / partial_review;\nn_individuals = 8 / 7;\n'
             'n_subsets = 3 / 3;\nindividual_assignment =\n' + ''.join(
                 f'{n} : {1 if i < 2 else 2 if i < 5 else 3} / {"?" if i == 7 else 1 if i < 2 else 2 if i < 5 else 3}\n'
                 for i, n in enumerate(IDS)) + ';\nend;\n')
    add('species-delimitation/hypotheses.spart', spart, 'Two constructed hypotheses, second unassigns specimen_08; not validated species')
    add('tree-inference/quoted.nex', "#NEXUS\nbegin trees;\ntranslate 1 'sample α', 2 'sample beta', 3 gamma;\ntree demo = [&R] ((1:0,2:0.2)95:0.1,3:0.3);\nend;\n", 'Three tips, quoted Unicode/space labels, support 95 and zero length', 'synthetic-edge')
    add('tree-inference/unrooted.nwk', '(a:0.1,b:0.1,c:0.2);\n', 'Unrooted interpretation; trifurcation', 'analytical')
    reference = ancestor[:36]
    add('sanger-review/reference.fasta', fasta([('reference', reference)]), '36 nt; forward calls differ at index 12, reverse at reference index 20 (N)')
    forward = reference[:12] + ('C' if reference[12] != 'C' else 'A') + reference[13:]
    reverse = reverse_complement(reference[:20] + 'N' + reference[21:])
    add('sanger-review/forward.ab1', abif(forward), '36 calls, 444 samples, disagreement at index 12, quality 8 at ends/35 inside; synthetic format test')
    add('sanger-review/reverse.ab1', abif(reverse), 'Reverse read orientation; N at call index 15; synthetic format test')
    add('sanger-review/no-quality.ab1', abif(reference, quality=False), 'PCON absent: quality unknown, never fabricated', 'synthetic-edge')
    add('recovery-errors/invalid/peak-outside.ab1', abif(reference, bad_peak=True), 'Reject PLOC sample index equal to channel length', 'invalid')
    add('recovery-errors/invalid/truncated.ab1', abif(reference)[:-80], 'Reject payload outside file', 'invalid')
    for name, content, expected in [
        ('empty.fasta', '', 'IO permits zero records; analyses reject empty inputs'),
        ('missing-header.fasta', 'ACGT\n', 'Reject sequence before header'),
        ('symbols.fasta', '>bad\nACG!7T\n', 'Robustness probe: IO is permissive; biological validation is a separate concern'),
        ('ragged.afa', '>a\nACGT\n>b\nACG\n', 'Reject as distance alignment; sequence IO may read it'),
        ('malformed.nwk', '((a:0.1,b:0.2);', 'Reject unmatched parenthesis'),
        ('malformed.gff3', 'locus\tsim\tCDS\t0\t12\t.\t+\t0\tID=bad\n', 'Reject start zero'),
        ('malformed.gb', 'this is not GenBank\n', 'Reject missing LOCUS'),
        ('truncated.gb', gb[:-20], 'Robustness probe: current GenBank reader accepts missing terminator; validation gap'),
        ('malformed.spart', 'begin spart;\nn_individuals = nope;\nend;\n', 'Reject malformed SPART'),
        ('missing-tip.nwk', '(specimen_01:0.1,unknown:0.1);\n', 'Tree is parseable; reject matching against eight-specimen alignment'),
        ('negative.nwk', '(a:-1,b:0);\n', 'IO may retain lengths; scientific tree validation must reject negatives'),
    ]:
        add('recovery-errors/invalid/' + name, content, expected, 'invalid')
    readmes = {
        'sequences-annotations': 'Open locus.gb, then locus.fasta (with locus.gff3 alongside). Inspect joined forward and reverse CDS, extract/translate, save a copy and reopen. Both CDS encode coding.fasta. Protein and noncoding FASTA are separate molecules.',
        'alignment-editing': 'Open homologues.afa. Select/copy a block, edit a residue, undo/redo, save a copy and reopen. Eight distinct specimen IDs must survive. Try MAFFT on unaligned.fasta. ambiguous-gappy.afa has a deliberately all-gap block and N; identifiers.fasta is an ID robustness case.',
        'tree-inference': 'Open informative.afa. Try distances/NJ/UPGMA/bootstrap (seed 1001), FastTree or IQ-TREE if installed. Check all eight taxa and finite nonnegative branches; no universal topology or species truth is promised. known-distances.afa has exact p distances 1/12, 1/12, 2/12. quoted.nex tests support and labels.',
        'species-delimitation': 'Open specimens.afa and hypotheses.spart. Try Manual/Combine and deduplication; all eight IDs must map back, including duplicates. time-binary.nwk is a normal GMYC time tree; time.nwk tests ties/polytomies. substitutions.nwk is a rooted bifurcating PTP input with stipulated substitutions/site, unsuitable for GMYC. Branch units are supplied here, not encoded by Newick. Small data can yield uninformative estimates; construction groups are not validated species.',
        'sanger-review': 'Open forward.ab1 and reverse.ab1 against reference.fasta. Review a disagreement and ambiguous call, inspect low-quality ends, and try no-quality.ab1 (quality must remain unknown). These minimal synthetic triangular traces test parsing and display only; they do not validate Tracy, basecalling accuracy, chemistry or empirical Sanger performance.',
        'recovery-errors': 'Files in invalid/ are deliberate robustness probes, not biological examples. See MANIFEST.json for per-file outcomes and known permissive readers. Work on copies in a disposable folder. Save an edited valid example, reopen and inspect History/Methods. Changed hashes, damaged journals, unwritable paths and partial writes belong in isolated automated tests, never in existing user data.',
    }
    for directory, instructions in readmes.items():
        add(directory + '/README.txt', instructions + '\n', 'Manual procedure', 'documentation')
    add('README.txt', 'Basecraft compact synthetic suite v2, seed 1001. GPL-3.0-or-later.\n'
        'Open the named folders and read their README.txt. MANIFEST.json records origin, hashes and expectations.\n'
        'No natural accession, external sequences or user data are included. No empirical species or Sanger validation is claimed.\n', 'Start here', 'documentation')
    manifest = {'version': 2, 'seed': SEED, 'origin': 'Original designed ancestor with inherited synonymous substitutions and a codon deletion; synthetic traces and analytical/invalid cases labeled separately',
                'license': 'GPL-3.0-or-later', 'specimens': IDS,
                'haplotypes': [[IDS[0], IDS[1]], [IDS[2]], [IDS[3]], [IDS[4]], [IDS[5]], [IDS[6], IDS[7]]],
                'files': {name: {**descriptions[name], 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)} for name, data in sorted(files.items())}}
    files['MANIFEST.json'] = (json.dumps(manifest, indent=2, ensure_ascii=False) + '\n').encode()
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=Path.home() / '.cache/basecraft-testdata/compact')
    parser.add_argument('--check', action='store_true', help='Compare committed fixtures without writing')
    args = parser.parse_args()
    files = suite()
    if args.check:
        actual = {p.relative_to(FIXTURES).as_posix(): p.read_bytes() for p in FIXTURES.rglob('*') if p.is_file()}
        if files != actual:
            parser.error('committed compact fixtures differ from the generator')
    else:
        # Refuse existing destinations, including edited demos. No delete/force option.
        args.out.mkdir(parents=True, exist_ok=False)
        for name, data in files.items():
            path = args.out / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    print(f'{len(files)} files, {sum(map(len, files.values()))} bytes; seed {SEED}')


if __name__ == '__main__':
    main()
