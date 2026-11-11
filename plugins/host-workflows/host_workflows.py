"""Metadata entry for application-owned public workflows.

Execution is dispatched by the shared host to its existing workflow orchestrator.
This entry provides a clear refusal if launched as an ordinary Python operation.
"""
from basecraft.plugin import Plugin

plugin = Plugin("basecraft.host", "1.0.0")


def refuse(_snapshot, _parameters, _context):
    raise ValueError("Application workflow requires the Basecraft registry host")


for operation in ("gmyc_alignment_single", "gmyc_alignment_multiple", "asap_alignment"):
    plugin.operation(operation)(refuse)

if __name__ == "__main__":
    plugin.run()
