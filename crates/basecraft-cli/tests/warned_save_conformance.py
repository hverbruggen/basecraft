"""Cross-interface saves with actual core reference ledgers supplied by Rust."""
import json
import os
from pathlib import Path
import subprocess
import sys

from basecraft.session import Session, OperationError

root, fixture = Path(sys.argv[1]), json.loads(Path(sys.argv[2]).read_text())
original = b">gene\nACGT\n"
edited = b">gene\nATGT\n"
operation = {"op": "seq.replace", "params": {"start": 1, "end": 2, "residues": "T"}}


def source(name):
    path = root / (name + ".fasta")
    path.write_bytes(original)
    return path


def open_edit(session, path):
    doc = session.open(path.name)["document"]
    session.apply(doc, 0, operation)
    return doc


def decision(check, policy="cancel", ack=True):
    return {"token": check["token"], "known_policy": policy, "acknowledge_incomplete": ack}


def refuses(action, code):
    try:
        action()
    except OperationError as error:
        assert error.code == code, error.outcome
        return error.outcome
    raise AssertionError("unexpected successful save: " + code)


def cli(*args, success):
    out = subprocess.run([os.environ["BASECRAFT"], *map(str, args)], capture_output=True, text=True)
    assert (out.returncode == 0) == success, (args, out.stdout, out.stderr)
    return out


# Python convenience routes and direct MCP call() share exactly one Session.
with Session(root, writes=True) as session:
    definitions = {t["name"]: t for t in session.tools()}
    schema = definitions["save"]["inputSchema"]["properties"]["decision"]
    assert set(schema["required"]) == {"token", "known_policy", "acknowledge_incomplete"}
    assert "save_check" in definitions
    path = source("unknown")
    doc = open_edit(session, path)
    before = path.read_bytes()
    failure = refuses(lambda: session.save(doc), "evidence_check_incomplete")
    assert failure["schema"] == 1 and failure["operation"] == "save"
    assert failure["errors"][0]["operation"] == "save"
    check = failure["check"]
    assert check["evidence"]["issues"] and not check["evidence"]["conflict"]["analyses"]
    assert path.read_bytes() == before
    assert all({"code", "location", "reason", "relevance"} <= set(i) for i in check["evidence"]["issues"])
    refuses(lambda: session.save(doc, decision=decision(check, ack=False)), "evidence_check_incomplete")
    check = session.save_check(doc)
    outcome = session.call("save", {"document": doc, "decision": decision(check)})
    assert outcome["warnings"][0]["code"] == "evidence_not_checked", outcome
    assert outcome["warnings"][0]["issues"] == check["evidence"]["issues"]
    assert path.read_bytes() == edited
    refuses(lambda: session.save(doc, decision=decision(check)), "save_decision_stale")

    # Known and unknown are independent; failed known-policy attempts spend tokens.
    path = root / "known.fasta"
    doc = open_edit(session, path)
    check = session.save_check(doc)
    assert check["evidence"]["conflict"]["analyses"] and check["evidence"]["issues"]
    refuses(lambda: session.save(doc, decision=decision(check)), "analysis_input_conflict")
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy")), "save_decision_stale")
    check = session.save_check(doc)
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy", False)), "evidence_check_incomplete")
    check = session.save_check(doc)
    saved = session.save(doc, decision=decision(check, "preserve_copy"))
    assert saved["warnings"] and path.read_bytes() == edited
    assert (Path(fixture["known"]["known"]) / "preserved-input").read_bytes() == original

    path = source("scope")
    doc = open_edit(session, path)
    old = session.save_check(doc)
    newer = session.call("save_check", {"document": doc})
    refuses(lambda: session.save(doc, decision=decision(old)), "save_decision_stale")
    # Refusal publishes another new check, so query again before the next mutation.
    check = session.save_check(doc)
    session.undo(doc)
    refuses(lambda: session.save(doc, decision=decision(check)), "save_decision_stale")
    assert path.read_bytes() == original
    check = session.save_check(doc)
    path.write_bytes(b">gene\nGGGG\n")
    refuses(lambda: session.save(doc, decision=decision(check)), "save_decision_stale")
    assert path.read_bytes() == b">gene\nGGGG\n"
    check = session.save_check(doc)
    refuses(lambda: session.save(doc, decision=decision(check)), "changed_on_disk")
    assert path.read_bytes() == b">gene\nGGGG\n"

    # Same bytes in a replacement file still have a different target identity.
    path = source("replacement")
    doc = open_edit(session, path)
    check = session.save_check(doc)
    replacement = root / "replacement.tmp"
    replacement.write_bytes(original)
    replacement.replace(path)
    refuses(lambda: session.save(doc, decision=decision(check)), "save_decision_stale")
    assert path.read_bytes() == original

    # Changed raw damaged evidence also invalidates the displayed issue scope.
    path = source("inventory")
    doc = open_edit(session, path)
    check = session.save_check(doc)
    Path(fixture["ledger"]).write_bytes(b"different damaged ledger")
    refuses(lambda: session.save(doc, decision=decision(check)), "save_decision_stale")
    assert path.read_bytes() == original

    # Save As binding, no overwrite, root confinement, strict choices.
    path = source("saveas")
    doc = open_edit(session, path)
    check = session.save_check(doc, "fresh.fasta")
    refuses(lambda: session.save(doc, "other.fasta", decision=decision(check)), "save_decision_stale")
    occupied = source("occupied")
    check = session.save_check(doc, occupied.name)
    refuses(lambda: session.save(doc, occupied.name, decision=decision(check)), "invalid_arguments")
    assert occupied.read_bytes() == original and path.read_bytes() == original
    check = session.save_check(doc, "new.fasta")
    (root / "new.fasta").write_bytes(original)
    refuses(lambda: session.save(doc, "new.fasta", decision=decision(check)), "invalid_arguments")
    refuses(lambda: session.save_check(doc, "../outside.fasta"), "invalid_arguments")
    check = session.save_check(doc)
    for choice in [None, {}, {**decision(check), "acknowledge_incomplete": "true"},
                   {**decision(check), "known_policy": "anything"}, {**decision(check), "extra": True}]:
        refuses(lambda: session.call("save", {"document": doc, "decision": choice}), "invalid_arguments")
    # Foreign session cannot use or consume this live Session's token.
    check = session.save_check(doc)
    with Session(root, writes=True) as other:
        other_doc = other.open(path.name)["document"]
        refuses(lambda: other.save(other_doc, decision=decision(check)), "save_decision_stale")
    assert session.save(doc, decision=decision(check))["warnings"]

    path = root / "preservation.fasta"
    doc = open_edit(session, path)
    blocked = Path(fixture["known"]["preservation"]) / "preserved-input"
    blocked.mkdir()
    check = session.save_check(doc)
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy")), "invalid_arguments")
    assert path.read_bytes() == original
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy")), "save_decision_stale")
    blocked.rmdir()

    # Actual pending-intent write failure after preserving the accepted source.
    path = root / "writefail.fasta"
    doc = open_edit(session, path)
    check = session.save_check(doc)
    state = Path(os.environ["BASECRAFT_STATE_DIR"])
    blocker = state / "recovery" / check["session"] / "pending-save.json"
    blocker.mkdir()
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy")), "io_error")
    assert path.read_bytes() == original and session.state(doc)["can_undo"]
    assert (Path(fixture["known"]["writefail"]) / "preserved-input").read_bytes() == original
    blocker.rmdir()
    refuses(lambda: session.save(doc, decision=decision(check, "preserve_copy")), "save_decision_stale")
    check = session.save_check(doc)
    assert session.save(doc, decision=decision(check, "preserve_copy"))["warnings"]
    assert path.read_bytes() == edited

    # Permission cannot be waived, including an explicit valid receipt.
    path = source("permission")
    doc = open_edit(session, path)
    check = session.save_check(doc)
    root.chmod(0o555)
    try:
        if os.geteuid() != 0:
            refuses(lambda: session.save(doc, decision=decision(check)), "permission_denied")
            assert path.read_bytes() == original
        else:
            print("permission filesystem test unavailable as root")
    finally:
        root.chmod(0o755)

with Session(root) as readonly:
    doc = readonly.open("unknown.fasta")["document"]
    check = readonly.save_check(doc)
    refuses(lambda: readonly.save(doc, decision=decision(check)), "permission_denied")

# One-shot CLI uses a check and choice in the same invocation, never an old token.
path = source("cli-unknown")
args = ["edit", path, json.dumps(operation)]
out = cli(*args, success=False)
assert json.loads(out.stderr)["errors"][0]["code"] == "evidence_check_incomplete"
assert path.read_bytes() == original
snapshot = json.loads(cli("save-check", path, success=True).stdout)
assert snapshot["evidence"]["issues"]
out = cli(*args, "--acknowledge-incomplete-evidence", success=True)
assert json.loads(out.stderr)["warnings"][0]["code"] == "evidence_not_checked"
assert path.read_bytes() == edited
path = root / "cli-known.fasta"
args = ["edit", path, json.dumps(operation)]
out = cli(*args, "--acknowledge-incomplete-evidence", success=False)
assert json.loads(out.stderr)["errors"][0]["code"] == "analysis_input_conflict"
assert path.read_bytes() == original
out = cli(*args, "--acknowledge-incomplete-evidence", "--save-known-policy", "preserve_copy", success=True)
assert json.loads(out.stderr)["warnings"] and path.read_bytes() == edited
assert (Path(fixture["known"]["cli-known"]) / "preserved-input").read_bytes() == original
path = root / "cli-preservation.fasta"
blocked = Path(fixture["known"]["cli-preservation"]) / "preserved-input"
blocked.mkdir()
out = cli("edit", path, json.dumps(operation), "--acknowledge-incomplete-evidence",
          "--save-known-policy", "preserve_copy", success=False)
assert json.loads(out.stderr)["errors"][0]["code"] == "invalid_arguments" and path.read_bytes() == original
blocked.rmdir()

# CLI creation retains its original no-overwrite route under explicit consent.
occupied = root / "occupied.gb"
occupied.write_bytes(original)
out = cli("new-sequence", "--name", "new", "--sequence", "ACGT", "--output", occupied,
          "--acknowledge-incomplete-evidence", success=False)
assert json.loads(out.stderr)["errors"][0]["code"] == "invalid_arguments"
assert occupied.read_bytes() == original
out = cli("new-sequence", "--name", "new", "--sequence", "ACGT", "--output", root / "new.gb",
          "--acknowledge-incomplete-evidence", success=True)
assert json.loads(out.stderr)["warnings"][0]["code"] == "evidence_not_checked"
if os.geteuid() != 0:
    path = source("cli-permission")
    root.chmod(0o555)
    try:
        out = cli("edit", path, json.dumps(operation), "--acknowledge-incomplete-evidence", success=False)
        assert json.loads(out.stderr)["errors"][0]["code"] == "permission_denied"
        assert path.read_bytes() == original
    finally:
        root.chmod(0o755)

# Repair only the deliberately damaged ledger; complete checks have no warning.
Path(fixture["ledger"]).write_text(fixture["good"])
with Session(root, writes=True) as session:
    path = source("complete")
    doc = open_edit(session, path)
    check = session.save_check(doc)
    assert not check["evidence"]["issues"]
    assert session.save(doc)["warnings"] == []
    assert path.read_bytes() == edited
    path = root / "preservation.fasta"
    doc = open_edit(session, path)
    check = session.save_check(doc)
    assert check["evidence"]["conflict"]["analyses"] and not check["evidence"]["issues"]
    refuses(lambda: session.save(doc), "analysis_input_conflict")
    check = session.save_check(doc)
    assert session.save(doc, decision=decision(check, "preserve_copy", False))["warnings"] == []
    path = source("complete-explicit")
    doc = open_edit(session, path)
    check = session.save_check(doc)
    assert session.save(doc, decision=decision(check, ack=False))["warnings"] == []
path = root / "cli-preservation.fasta"
out = cli("edit", path, json.dumps(operation), "--save-known-policy", "preserve_copy", success=True)
assert not out.stderr and path.read_bytes() == edited
path = source("cli-complete")
out = cli("edit", path, json.dumps(operation), success=True)
assert not out.stderr and path.read_bytes() == edited
print("CLI, Python and MCP warned-save scenarios passed")
