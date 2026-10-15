"""Independent construction invariants and byte-exact fixture availability."""
import hashlib
import json
from pathlib import Path
import struct
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

    def test_abif_manufacturer_directory_schema(self):
        # Decode committed bytes independently of generate.abif and Basecraft IO.
        # Applied Biosystems July 2006, pp. 14 and 52-53: PLOC short, PCON char.
        for name in ('forward.ab1', 'reverse.ab1', 'no-quality.ab1'):
            with self.subTest(trace=name):
                data = (FIXTURES / 'sanger-review' / name).read_bytes()
                self.assertEqual(data[:4], b'ABIF')
                root = struct.unpack_from('>4sIHHIIII', data, 6)
                self.assertEqual(root[:4], (b'tdir', 1, 1023, 28))
                count, size, offset = root[4:7]
                self.assertEqual(size, 28 * count)
                self.assertGreaterEqual(offset, 128)
                self.assertLessEqual(offset + size, len(data))
                entries = {}
                for i in range(count):
                    at = offset + i * 28
                    tag, number, kind, width, n, size, address, _ = struct.unpack_from(
                        '>4sIHHIIII', data, at)
                    self.assertNotIn((tag, number), entries)
                    self.assertEqual(size, width * n)
                    start = at + 20 if size <= 4 else address
                    self.assertLessEqual(start + size, len(data))
                    entries[tag, number] = ((kind, width, n), data[start:start + size])
                self.assertEqual(entries[b'FWO_', 1], ((2, 1, 4), b'GATC'))
                for number in range(9, 13):
                    self.assertEqual(entries[b'DATA', number][0], (4, 2, 444))
                self.assertEqual(entries[b'PBAS', 2][0], (2, 1, 36))
                self.assertEqual(entries[b'PLOC', 2][0], (4, 2, 36))
                self.assertEqual(struct.unpack('>36h', entries[b'PLOC', 2][1]),
                                 tuple(range(12, 433, 12)))
                if name == 'no-quality.ab1':
                    self.assertNotIn((b'PCON', 2), entries)
                else:
                    self.assertEqual(entries[b'PCON', 2][0], (2, 1, 36))
                    calls = entries[b'PBAS', 2][1]
                    expected = tuple(8 if i in (0, 35) or c == ord('N') else 35
                                     for i, c in enumerate(calls))
                    self.assertEqual(struct.unpack('>36b', entries[b'PCON', 2][1]), expected)

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
