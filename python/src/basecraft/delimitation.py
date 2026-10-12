"""Species delimitation through Basecraft's public CLI and typed operations.

This Apache-2.0 module contains no scientific implementation or GPL dependency.
Numerical work is performed by the installed application's public plugins.
"""

import json
import subprocess
from pathlib import Path

from basecraft.tools import executable


def operation(action, **parameters):
    """Create a versioned ``delimitation.edit`` operation for CLI/MCP/plugins."""
    return {"op": "delimitation.edit", "params": {"edit": {"action": action, **parameters}}}


def run(source, output, method="gmyc_single", **parameters):
    """Run the same public plugins as the GUI and return their artifact manifest.

    For an alignment, use ``gmyc_alignment_single`` or
    ``gmyc_alignment_multiple`` and ``unique_haplotypes=True`` (the default).
    These infer a UPGMA equal-rate tree after the chosen sampling policy.
    Tree methods require an explicit ``rooted=True`` assertion. Failed/cancelled
    jobs raise ``CalledProcessError`` and retain the application's reports.
    """
    command = [
        executable(["basecraft"], "BASECRAFT"),
        "tool",
        "delimitation",
        str(Path(source).resolve()),
        "--output",
        str(Path(output).resolve()),
        "--params",
        json.dumps(dict(parameters, method=method)),
    ]
    process = subprocess.run(command, capture_output=True, text=True, check=True)
    return json.loads(process.stdout)


def apply(workspace, action, **parameters):
    """Apply one typed edit and save the workspace with its replayable history."""
    command = [
        executable(["basecraft"], "BASECRAFT"),
        "edit",
        str(Path(workspace).resolve()),
        json.dumps(operation(action, **parameters)),
    ]
    subprocess.run(command, capture_output=True, text=True, check=True)
