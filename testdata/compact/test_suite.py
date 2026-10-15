"""Independent construction invariants and byte-exact fixture availability."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from generate import FIXTURES, suite


def rows(data):
    return [(r.splitlines()[0], ''.join(r.splitlines()[1:]))
            for r in data.decode().split('>')[1:]]


class CompactSuite(unittest.TestCase):
    def test_committed_bytes_hashes_and_budget(self):
        expected = suite()
        actual = {p.relative_to(FIXTURES).as_posix(): p.read_bytes()
                  for p in FIXTURES.rglob('*') if p.is_file()}
        self.assertEqual(expected, actual)
        self.assertLess(sum(map(len, actual.values())), 100_000)
        manifest = json.loads(actual['MANIFEST.json'])
        self.assertEqual(set(manifest['files']), set(actual) - {'MANIFEST.json'})
        for name, info in manifest['files'].items():
            self.assertEqual(info['sha256'], hashlib.sha256(actual[name]).hexdigest())
            self.assertEqual(info['bytes'], len(actual[name]))

    def test_coding_ancestry_and_deletion(self):
        data = suite()
        ancestor = rows(data['sequences-annotations/coding.fasta'])[0][1]
        self.assertEqual(len(ancestor), 180)
        self.assertEqual((ancestor[:3], ancestor[-3:]), ('ATG', 'TAA'))
        homologues = rows(data['alignment-editing/homologues.afa'])
        self.assertEqual(len(homologues), 8)
        self.assertEqual(len({s for _, s in homologues}), 6)
        self.assertEqual(homologues[0][1], homologues[1][1])
        self.assertEqual(homologues[6][1], homologues[7][1])
        # Independent standard genetic-code table, not the generator's synonyms.
        bases = 'TCAG'
        aa = 'FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG'
        code = dict(zip((a+b+c for a in bases for b in bases for c in bases), aa))
        translate = lambda s: ''.join(code[s[i:i+3]] for i in range(0, len(s), 3))
        protein = rows(data['sequences-annotations/protein.fasta'])[0][1]
        self.assertEqual(translate(ancestor), protein + '*')
        for i, (_, sequence) in enumerate(homologues):
            self.assertEqual(len(sequence), 180)
            self.assertEqual(translate(sequence.replace('-', '')),
                             protein + '*' if i < 5 else protein[:40] + protein[41:] + '*')
            self.assertGreater(sum(a == b for a, b in zip(ancestor, sequence)), 160)
        self.assertEqual(rows(data['alignment-editing/unaligned.fasta']),
                         [(n, s.replace('-', '')) for n, s in homologues])

    def test_annotation_sequence_truth(self):
        data = suite()
        genome = rows(data['sequences-annotations/locus.fasta'])[0][1]
        ancestor = rows(data['sequences-annotations/coding.fasta'])[0][1]
        self.assertEqual(len(genome), 421)
        self.assertEqual(genome[:61] + genome[90:209], ancestor)
        self.assertEqual(genome[241:421].translate(str.maketrans('ACGT', 'TGCA'))[::-1], ancestor)
        self.assertEqual(genome[61:63], 'GT')
        self.assertEqual(genome[88:90], 'AG')

    def test_export_refuses_to_overwrite_edits(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / 'demo'
            script = Path(__file__).with_name('generate.py')
            first = subprocess.run([sys.executable, script, '--out', dest], capture_output=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            sentinel = dest / 'README.txt'
            sentinel.write_text('user edits')
            second = subprocess.run([sys.executable, script, '--out', dest], capture_output=True)
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(sentinel.read_text(), 'user edits')


if __name__ == '__main__':
    unittest.main()
