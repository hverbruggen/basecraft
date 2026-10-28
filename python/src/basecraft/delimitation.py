"""Species delimitation through Basecraft's public CLI and typed operations.

This Apache-2.0 module contains no scientific implementation or GPL dependency.
Numerical work is performed by the installed application's public plugins.
"""

import json
import os
import subprocess
from pathlib import Path

from basecraft.tools import executable


def operation(action, **parameters):
    """Create a versioned ``delimitation.edit`` operation for CLI/MCP/plugins."""
    return {"op": "delimitation.edit", "params": {"edit": {"action": action, **parameters}}}


_WORKFLOWS = {"gmyc_alignment_single": "gmyc_alignment_single",
              "gmyc_alignment_multiple": "gmyc_alignment_multiple",
              "asap_alignment": "asap_alignment"}


def run(source, method="gmyc_single", workspace=None, output=None, **parameters):
    """Run a registered delimitation operation and return its outcome.

    ``source`` is the tree or alignment to analyse. For an alignment, use
    ``gmyc_alignment_single``, ``gmyc_alignment_multiple`` or
    ``asap_alignment``; these infer a UPGMA equal-rate tree after the chosen
    sampling policy. Tree methods require an explicit ``rooted=True``.
    With ``workspace`` (a ``.spart`` file), the run is recorded in that
    workspace and its hypotheses are added and saved. ``output`` copies the
    result report to a new file. Raises ``OperationError`` unless completed.
    """
    from .operations import run as run_operation

    source = Path(source).resolve()
    paths = [source] + [Path(p).resolve() for p in (workspace, output) if p is not None]
    project = Path(os.path.commonpath(paths))
    if project == source:
        project = source.parent
    package = "basecraft.host" if method in _WORKFLOWS else "basecraft.delimitation"
    operation = f"{package}/{method}@1"
    tree = source.suffix.lower() in (".nwk", ".newick", ".tre", ".tree", ".nex", ".nexus")
    role = "tree" if tree and package == "basecraft.delimitation" else "alignment"
    destinations = {}
    if output is not None:
        result = "fit.json" if package == "basecraft.host" else "result.json"
        destinations[f"artifacts/{result}"] = str(Path(output).resolve().relative_to(project))
    request = {"inputs": {role: [{"kind": "file", "path": str(source.relative_to(project))}]},
               "parameters": parameters, "destinations": destinations}
    if workspace is not None:
        request["target"] = {"kind": "file",
                             "path": str(Path(workspace).resolve().relative_to(project))}
    return run_operation(operation, project=project, **request)


def apply(workspace, action, **parameters):
    """Apply one typed edit and save the workspace with its replayable history."""
    command = [
        executable(["basecraft"], "BASECRAFT"),
        "edit",
        str(Path(workspace).resolve()),
        json.dumps(operation(action, **parameters)),
    ]
    subprocess.run(command, capture_output=True, text=True, check=True)
