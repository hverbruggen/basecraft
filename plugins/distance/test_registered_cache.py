"""Producer and accepted-input identity checks used by registered cache bindings."""
import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path

import distance_plugin
from distance_plugin import invoke


@unittest.skipUnless(os.environ.get("BASECRAFT_DISTANCE"), "requires native engine")
class RegisteredCacheIdentity(unittest.TestCase):
    def test_changed_source_and_same_version_different_producer_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.afa"
            source.write_text(">a\nAAAAAC\n>b\nAAAACC\n>c\nAAACCC\n")
            original = source.read_bytes()
            target = root / "matrix.tsv"
            params = {"input": str(source), "output": str(target), "model": "p"}
            invoke("matrix", params)
            cache = Path(str(target) + ".distances.bin")
            saved = cache.read_bytes()
            params.update(output=str(root / "next.tsv"), cache_input=str(cache))
            invoke("matrix", params)
            self.assertTrue(json.loads((root / "next.tsv.run.json").read_text())["cache_reused"])
            source.write_text(">a\nAAAAAT\n>b\nAAAACC\n>c\nAAACCC\n")
            with self.assertRaisesRegex(ValueError, "producer do not match"):
                invoke("matrix", dict(params, output=str(root / "changed.tsv")))
            source.write_bytes(original)
            other = root / "other-engine"
            shutil.copyfile(os.environ["BASECRAFT_DISTANCE"], other)
            # An identical advertised version is insufficient for producer identity.
            with other.open("ab") as stream:
                stream.write(b"different producer bytes")
            with self.assertRaisesRegex(ValueError, "producer do not match"):
                invoke("matrix", dict(params, executable=str(other), output=str(root / "other.tsv")))
            self.assertEqual(cache.read_bytes(), saved)
            self.assertFalse((root / "changed.tsv").exists())
            self.assertFalse((root / "other.tsv").exists())


class MethodsTextTest(unittest.TestCase):
    def test_unreadable_cache_input_gets_a_clear_error(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "cache"
            for content in (b"[]", b"\xff\xfe not json", b"", distance_plugin.CACHE_MAGIC + b"{broken\n"):
                path.write_bytes(content)
                with self.assertRaisesRegex(ValueError, "calculate a fresh cache"):
                    distance_plugin.cache_identity(path)

    def test_recorded_settings_match_what_ran(self):
        params = {"min_overlap": 2, "seed": 7}
        matrix = distance_plugin.resolved_parameters("matrix", "p", "njstar", params, 0)
        self.assertNotIn("method", matrix)
        boot = distance_plugin.resolved_parameters("tree", "p", "sorted", params, 100)
        self.assertEqual(boot["seed"], 7)
        self.assertEqual(boot["method"], "sorted")
        plain = distance_plugin.resolved_parameters("tree", "p", "sorted", params, 0)
        self.assertNotIn("seed", plain)

    def test_each_method_cites_its_sources(self):
        tree = distance_plugin.methods("tree", "jc69", "sorted", 1, 0)
        cited = "\n".join(tree["citations"])
        for author in ("Saitou", "Studier", "Clausen", "Jukes", "Elias", "Simonsen"):
            self.assertIn(author, cited)
        self.assertIn("Clausen 2023", tree["description"])
        star = distance_plugin.methods("tree", "k2p", "njstar", 2, 0)
        self.assertIn("Criscuolo", "\n".join(star["citations"]))
        self.assertIn("Kimura", "\n".join(star["citations"]))
        boot = distance_plugin.methods("tree", "p", "sorted", 1, 100)
        self.assertIn("Felsenstein", "\n".join(boot["citations"]))
        self.assertIn("Steele", "\n".join(boot["citations"]))
        self.assertIn("with 100 ordinary site bootstrap replicates", boot["description"])

    def test_bootstrap_text_reports_what_ran(self):
        def text(**report):
            base = {"requested": 10, "attempted": 10, "completed": 10, "failed_replicates": 0,
                    "support_withheld": False, "cancelled": False}
            return distance_plugin.methods("tree", "p", "sorted", 1, 10, True, {**base, **report})
        self.assertIn("with 10 ordinary site bootstrap replicates", text()["description"])
        partial = text(completed=8, failed_replicates=2, support_withheld=True)
        self.assertIn("was incomplete (10 replicates requested, 10 attempted, 8 completed, 2 failed)", partial["description"])
        self.assertIn("support was withheld", partial["description"])
        self.assertNotIn("with 10 ordinary", partial["description"])
        cancelled = text(attempted=3, completed=3, support_withheld=True, cancelled=True)
        self.assertIn("was cancelled (10 replicates requested, 3 attempted, 3 completed, 0 failed)", cancelled["description"])
        for result in (partial, cancelled):
            self.assertIn("Felsenstein", "\n".join(result["citations"]))
            self.assertIn("Steele", "\n".join(result["citations"]))
        unsearched = distance_plugin.methods("tree", "p", "sorted", 1, 0, False)
        self.assertNotIn("Clausen", unsearched["description"])
        self.assertNotIn("Clausen", "\n".join(unsearched["citations"]))
        direct = distance_plugin.methods("tree", "k2p", "njstar", 2, 0, False)
        self.assertNotIn("Clausen", "\n".join(direct["citations"]))
        self.assertIn("Murtagh", "\n".join(distance_plugin.methods("tree", "p", "upgma", 1, 0)["citations"]))
        matrix = distance_plugin.methods("matrix", "p", "njstar", 1, 0)
        self.assertNotIn("Saitou", "\n".join(matrix["citations"]))
        self.assertIn("Elias", "\n".join(matrix["citations"]))

