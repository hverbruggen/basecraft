import unittest

from consensus import signal_bounds
from primers import detect, reverse_complement


class PrimerTests(unittest.TestCase):
    def test_partial_mismatched_and_reverse_orientation(self):
        forward = "GACTAGCTACGATCGTACGA"
        reverse = "CGATGCTAGTCAGATCGCTA"
        left = forward[3:]
        left = left[:6] + ("A" if left[6] != "A" else "C") + left[7:]
        core = "TGCCTGAACCATGGCCTAAC" * 15
        sequence = left + core + reverse_complement(reverse)
        result = detect(sequence, forward, reverse)
        self.assertEqual(len(result["hits"]), 2)
        self.assertEqual(result["keep_start"], len(left))
        self.assertEqual(result["keep_end"], len(left) + len(core))
        opposite = detect(reverse_complement(sequence), forward, reverse)
        self.assertEqual(len(opposite["hits"]), 2)
        self.assertEqual(opposite["orientation"], "reverse")

    def test_indels_iupac_and_noisy_read_bases(self):
        primer = "GACTAGCTACGATCGTACGA"
        inserted = primer[:8] + "A" + primer[8:]
        result = detect(inserted + "C" * 200, primer, "")
        self.assertEqual(result["keep_start"], len(inserted))
        self.assertTrue(detect(primer + "C" * 200, "GACTAGYTACGATCGTACGA", "")["hits"])
        self.assertFalse(detect("N" * 200, primer, "")["hits"])
        self.assertFalse(detect("ACGT" * 80, "N" * 20, "")["hits"])

    def test_internal_occurrence_and_unrelated_sequences_do_not_mask(self):
        primer = "GACTAGCTACGATCGTACGA"
        result = detect("C" * 150 + primer + "C" * 150, primer, "")
        self.assertEqual(result["hits"], [])
        self.assertEqual(result["keep_start"], 0)
        self.assertFalse(detect("C" * 300, primer, "")["hits"])

    def test_signal_drop_is_sustained_and_does_not_recover_from_noise_spikes(self):
        heights = [10] * 16 + [100] * 64 + [10] * 16 + [300] * 2
        raw = {
            "basecallPos": list(range(1, len(heights) + 1)),
            **{"peak" + b: heights for b in "ACGT"},
        }
        left, right = signal_bounds(raw, 20)
        self.assertLess(left, 20)
        self.assertGreater(left, 8)
        self.assertLess(right, 85)
        self.assertGreater(right, 70)

    def test_chosen_signal_cutoff_changes_retained_ends(self):
        heights = [5] * 16 + [15] * 16 + [100] * 64 + [15] * 16 + [5] * 16
        raw = {
            "basecallPos": list(range(1, len(heights) + 1)),
            **{"peak" + b: heights for b in "ACGT"},
        }
        low_start, low_end = signal_bounds(raw, 10)
        high_start, high_end = signal_bounds(raw, 20)
        self.assertLess(low_start, high_start)
        self.assertGreater(low_end, high_end)
        for invalid in (0, 91):
            with self.assertRaisesRegex(ValueError, "between 1 and 90"):
                signal_bounds(raw, invalid)


if __name__ == "__main__":
    unittest.main()
