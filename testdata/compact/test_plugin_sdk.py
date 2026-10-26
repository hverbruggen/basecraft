"""Public SDK negotiation tests; no core implementation imports."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python/src"))
from basecraft.plugin import Plugin  # noqa: E402


class PluginContextTests(unittest.TestCase):
    def test_legacy_handlers_remain_two_argument_and_context_is_opt_in(self):
        plugin = Plugin("org.example.legacy", "1.0.0")

        @plugin.operation("old")
        def old(snapshot, parameters):
            self.assertIsNone(snapshot)
            return {"message": str(parameters["n"])}

        self.assertEqual(plugin._dispatch("initialize", {}, [])["capabilities"], [])
        self.assertEqual(
            plugin._dispatch("invoke", {"operation": "old", "params": {"n": 4}}, []),
            {"message": "4"},
        )
        with self.assertRaises(ValueError):
            plugin._dispatch("invoke", {"operation": "old", "context": {"schema": 1}}, [])

    def test_context_handlers_require_version_and_receive_unchanged_parameters(self):
        plugin = Plugin("org.example.context", "1.0.0")

        @plugin.context_operation("new")
        def new(snapshot, parameters, context):
            self.assertIsNone(snapshot)
            self.assertEqual(parameters, {"count": True})
            self.assertEqual(context, {"schema": 1, "cpu_threads": 3})
            return {"data": {"ok": True}}

        info = plugin._dispatch("initialize", {}, [])
        self.assertEqual(info["operations"], ["new"])
        self.assertEqual(info["capabilities"], ["operation-contract-v1"])
        request = {"operation": "new", "params": {"count": True}}
        with self.assertRaises(ValueError):
            plugin._dispatch("invoke", request, [])
        request["context"] = {"schema": 1, "cpu_threads": 3}
        self.assertEqual(plugin._dispatch("invoke", request, []), {"data": {"ok": True}})
        with self.assertRaises(ValueError):
            plugin.operation("new")(lambda _snapshot, _parameters: None)

    def test_declared_legacy_adapter_preserves_controls_and_allocated_collections(self):
        import json
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "outputs"
            output.mkdir()
            contract = root / "operations.json"
            contract.write_text(json.dumps({"schema": 1, "operations": [{
                "implementation": "old", "legacy_adapter": {
                    "primary_input": "source", "inputs": {"source": "input"},
                    "outputs": {"output": {"role": "artifacts", "path": "result.txt"}},
                    "allocation": "artifacts",
                },
            }]}))
            plugin = Plugin("org.example.adapter", "1.0.0")

            @plugin.operation("old")
            def old(_snapshot, parameters):
                self.assertEqual(parameters["count"], 3)
                self.assertEqual(parameters["input"], str(root / "source"))
                target = Path(parameters["output"])
                target.write_text("exact result")
                target.with_suffix(".log").write_text("diagnostic")
                return {"outputs": [str(target)], "ops": []}

            plugin.contract(contract)
            self.assertEqual(plugin._dispatch("initialize", {}, [])["capabilities"], ["operation-contract-v1"])
            context = {"schema": 1, "inputs": {"source": [str(root / "source")]},
                       "outputs": {"artifacts": str(output)}, "run_id": "owned",
                       "cancel_file": str(root / "cancel"), "progress_file": str(root / "progress")}
            result = plugin._dispatch("invoke", {"operation": "old", "params": {"count": 3}, "context": context}, [])
            self.assertEqual(result["artifacts"], [{"role": "artifacts", "path": "result.log"}, {"role": "artifacts", "path": "result.txt"}])
            self.assertIsNone(result["data"])
