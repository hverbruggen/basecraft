"""Strict IQ-TREE output validation and recoverable multi-file publication."""
import json
import math
import os
import re
import shutil
import tempfile
from pathlib import Path

from basecraft.tools import quote_newick, write_atomic


def _tokens(text):
    """Token spans allow leaf-only restoration without rewriting lengths/support."""
    tokens = []
    i = 0
    while i < len(text):
        start = i
        char = text[i]
        if char.isspace():
            i += 1
        elif char == "[":
            depth = 1
            i += 1
            while i < len(text) and depth:
                depth += (text[i] == "[") - (text[i] == "]")
                i += 1
            if depth:
                raise ValueError("unterminated Newick comment")
        elif char in "(),:;":
            tokens.append((char, char, i, i + 1))
            i += 1
        elif char == "'":
            value = ""
            i += 1
            while i < len(text):
                if text[i] == "'":
                    i += 1
                    if i == len(text) or text[i] != "'":
                        break
                value += text[i]
                i += 1
            else:
                raise ValueError("unterminated Newick label")
            tokens.append(("label", value, start, i))
        else:
            while i < len(text) and not text[i].isspace() and text[i] not in "[](),:;'":
                i += 1
            if start == i:
                raise ValueError("unexpected Newick delimiter")
            tokens.append(("label", text[start:i], start, i))
    return tokens


def restore_tree(text, names):
    """Require exactly one complete tree and exactly the expected leaf multiset."""
    try:
        tokens = _tokens(text)
        leaves = []
        children = []
        i = 0
        need_node = True
        root_done = False
        while not root_done:
            if i >= len(tokens):
                raise ValueError("incomplete Newick tree")
            kind, value, start, end = tokens[i]
            if need_node:
                if kind == "(":
                    children.append(0)
                    i += 1
                    continue
                if kind != "label" or not value:
                    raise ValueError("missing Newick leaf label")
                leaves.append((value, start, end))
                i += 1
            else:
                if kind == "," and children:
                    need_node = True
                    i += 1
                    continue
                if kind != ")" or not children or children.pop() < 2:
                    raise ValueError("invalid Newick child list")
                i += 1
                # Optional internal support/name is not a specimen label.
                if i < len(tokens) and tokens[i][0] == "label":
                    i += 1
            if i < len(tokens) and tokens[i][0] == ":":
                i += 1
                if i >= len(tokens) or tokens[i][0] != "label" or text[tokens[i][2]] == "'":
                    raise ValueError("missing Newick branch length")
                length = tokens[i][1]
                if not re.fullmatch(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?", length):
                    raise ValueError("invalid Newick branch length")
                if not math.isfinite(float(length)) or float(length) < 0:
                    raise ValueError("nonfinite or negative Newick branch length")
                i += 1
            need_node = False
            if children:
                children[-1] += 1
            else:
                root_done = True
        if i != len(tokens) - 1 or tokens[i][0] != ";":
            raise ValueError("expected one complete Newick tree")
        observed = [name for name, _, _ in leaves]
        if len(observed) != len(names) or set(observed) != set(names):
            raise ValueError("missing, duplicate or unknown specimen labels")
        restored = []
        previous = 0
        for name, start, end in leaves:
            restored.extend((text[previous:start], quote_newick(names[name])))
            previous = end
        restored.append(text[previous:])
        return "".join(restored)
    except ValueError as error:
        raise RuntimeError(f"Invalid IQ-TREE output: {error}") from error


def publish_artifacts(artifacts):
    """Stage all writes/backups, then replace; roll back caught publication errors.

    Separate replacements are not a transaction. A process interruption or failed
    rollback can leave a mixed set; retain the journal and prior files for recovery.
    A None payload removes an obsolete optional companion on successful publication.
    """
    directory = Path(tempfile.mkdtemp(prefix=".basecraft-iqtree-publish-", dir=artifacts[0][0].parent))
    entries = []
    attempted = []
    try:
        for index, (path, payload) in enumerate(artifacts):
            if path.is_symlink() or (path.exists() and not path.is_file()):
                raise OSError(f"Artifact destination is not a regular file: {path}")
            staged = directory / f"new-{index}"
            backup = directory / f"previous-{index}"
            if payload is not None:
                write_atomic(staged, payload)
            if path.exists():
                shutil.copy2(path, backup)
            entries.append({"destination": str(path), "staged": str(staged) if payload is not None else None,
                            "previous": str(backup) if backup.exists() else None})
        write_atomic(directory / "recovery.json", json.dumps({"version": 1, "artifacts": entries}, indent=2) + "\n")
        for entry in entries:
            attempted.append(entry)
            if entry["staged"]:
                os.replace(entry["staged"], entry["destination"])
            else:
                Path(entry["destination"]).unlink(missing_ok=True)
    except OSError as error:
        failures = []
        for entry in reversed(attempted):
            try:
                if entry["previous"]:
                    # Copy so the original backup survives even a failed restore.
                    restore = directory / "restore"
                    shutil.copy2(entry["previous"], restore)
                    os.replace(restore, entry["destination"])
                else:
                    Path(entry["destination"]).unlink(missing_ok=True)
            except OSError as restore_error:
                failures.append(f"{entry['destination']}: {restore_error}")
        if failures:
            raise OSError(f"IQ-TREE partial publication; rollback incomplete. Prior artifacts and recovery.json retained in {directory}. "
                          f"Publication error: {error}. Restore errors: {'; '.join(failures)}") from error
        shutil.rmtree(directory, ignore_errors=True)
        raise OSError(f"IQ-TREE publication failed; previous result preserved: {error}") from error
    # Cleanup cannot turn a fully published result into a reported failure.
    shutil.rmtree(directory, ignore_errors=True)
