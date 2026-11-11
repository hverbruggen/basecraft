"""Public SDK tests; no core implementation imports."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python/src"))
from basecraft.plugin import Plugin  # noqa: E402


def context(root, **fields):
    return {"schema": 2, "operation": "org.example.sdk/run@1", "run_id": "owned", "run_root": str(root),
            "inputs": {}, "outputs": {}, "cpu_threads": 3, "cancel_file": "cancel",
            "progress_file": "progress", **fields}


class PluginContextTests(unittest.TestCase):
    def test_handlers_receive_parameters_and_paths_resolved_against_the_run_folder(self):
        plugin = Plugin("org.example.sdk", "1.0.0")
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as elsewhere:
            root = Path(directory)
            (root / "pending").mkdir()
            external = Path(elsewhere) / "input.fa"

            @plugin.operation("run")
            def run(parameters, ctx):
                self.assertIsNone(ctx.snapshot)
                self.assertEqual(parameters, {"count": True})
                self.assertEqual(ctx.threads, 3)
                self.assertEqual(ctx.input("source"), str(external))
                self.assertEqual(ctx.cancel_file, str(root / "cancel"))
                self.assertEqual(ctx.source("source")["path"], str(external))
                Path(ctx.output("artifacts", "result.txt")).write_text("exact result")
                Path(ctx.output("artifacts", "logs/run.log")).write_text("diagnostic")
                return {"data": {"ok": True}}

            info = plugin._dispatch("initialize", {}, [])
            self.assertEqual(info, {"protocol": 2, "id": "org.example.sdk", "version": "1.0.0", "operations": ["run"]})
            raw = context(root, inputs={"source": [str(external)]}, outputs={"artifacts": "pending"},
                          input_evidence={"source": [{"original_path": str(external), "source_hash": "b3:x",
                                                      "submitted_hash": "b3:x", "transformation": "identity-v1",
                                                      "run_directory": "."}]})
            result = plugin._dispatch("invoke", {"operation": "run", "params": {"count": True}, "context": raw}, [])
            self.assertEqual(result["data"], {"ok": True})
            self.assertEqual(result["artifacts"], [{"role": "artifacts", "path": "logs/run.log"},
                                                   {"role": "artifacts", "path": "result.txt"}])

    def test_a_call_without_a_schema_two_context_is_refused(self):
        plugin = Plugin("org.example.sdk", "1.0.0")
        plugin.operation("run")(lambda _parameters, _context: None)
        for request in ({"operation": "run"}, {"operation": "run", "context": {"schema": 1}}):
            with self.subTest(request=request), self.assertRaises((KeyError, ValueError)):
                plugin._dispatch("invoke", request, [])
        with self.assertRaises(ValueError):
            plugin.operation("run")(lambda _parameters, _context: None)
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(TypeError):
            plugin._dispatch("invoke", {"operation": "run", "context": context(directory)}, [])

    def test_queries_take_no_inputs(self):
        plugin = Plugin("org.example.sdk", "1.0.0")
        plugin.query("engines")(lambda: {"engine": None})
        self.assertEqual(plugin._dispatch("query", {"name": "engines"}, []), {"engine": None})
        self.assertEqual(plugin._dispatch("initialize", {}, [])["operations"], [])


if __name__ == "__main__":
    unittest.main()
