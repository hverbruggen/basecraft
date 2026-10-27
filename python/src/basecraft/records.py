"""Read revision-scoped history and methods through the shared Basecraft core."""

import json
import subprocess
from pathlib import Path

from .tools import executable


def document_record(path):
    """Return structured history, methods and evidence for a file's current bytes.

    This reads saved evidence and never attaches to an unsaved GUI document.
    Use ``Session.document_record`` for a document open in a headless session.
    """
    result = subprocess.run(
        [executable(["basecraft"], "BASECRAFT"), "history", str(Path(path)), "--json"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    return json.loads(result.stdout)


def methods(path):
    """Return the generated methods text supported by the file's recorded evidence."""
    return document_record(path)["methods"]
