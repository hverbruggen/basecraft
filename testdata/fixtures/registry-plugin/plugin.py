"""Independent installed-package fixture; never imported by discovery."""
import json
import os
import time
from pathlib import Path

from basecraft.plugin import Plugin

if sentinel := os.environ.get("BASECRAFT_FIXTURE_SENTINEL"):
    Path(sentinel).write_text("imported")

plugin = Plugin("org.example.registry-fixture", "1.0.0")


@plugin.context_operation("count")
def count(_snapshot, parameters, context):
    mode = parameters["mode"]
    if mode == "slow":
        for _ in range(300):
            if Path(context["cancel_file"]).exists():
                raise RuntimeError("cancelled")
            time.sleep(0.1)
    if mode == "fail":
        raise RuntimeError("requested fixture failure")
    size = 0
    for path in context["inputs"]["sources"]:
        content = Path(path).read_bytes()
        if not parameters["include_newlines"]:
            content = content.replace(b"\n", b"")
        size += len(content)
    data = {"bytes": size if mode != "invalid_data" else "invalid"}
    Path(context["outputs"]["report"]).write_text(json.dumps(data) + "\n")
    return {"data": data, "artifacts": [{"role": "report", "path": "."}]}


if __name__ == "__main__":
    plugin.run()
