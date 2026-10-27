"""Single bounded NCBI POST. Query extraction/preflight belongs to shared Rust ops.

No retries, result polling, browser launch, or configurable upload endpoint.
"""
import hashlib
import http.client
import json
import os
import re
import signal
import ssl
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import urlencode

from basecraft.plugin import Plugin

plugin = Plugin("basecraft.blast", "0.1.0")
ENDPOINT = "https://blast.ncbi.nlm.nih.gov/Blast.cgi"
MAX_RESPONSE = 1024 * 1024
POST_SECONDS = 30
RID = re.compile(r"^[A-Z0-9-]{8,64}$")


class DeadlineError(TimeoutError):
    pass


def _deadline(_signum, _frame):
    raise DeadlineError("whole BLAST POST deadline exceeded")


def post(fields):
    """Alarm bounds DNS, TLS, upload and slow response together in SDK main thread."""
    previous = signal.signal(signal.SIGALRM, _deadline)
    signal.setitimer(signal.ITIMER_REAL, POST_SECONDS)
    connection = None
    try:
        connection = http.client.HTTPSConnection("blast.ncbi.nlm.nih.gov", timeout=POST_SECONDS,
                                                 context=ssl.create_default_context())
        body = urlencode(fields).encode("ascii")
        connection.request("POST", "/Blast.cgi", body,
                           {"Content-Type": "application/x-www-form-urlencoded",
                            "User-Agent": "Basecraft/0.1 (NCBI BLAST submission)"})
        response = connection.getresponse()
        data = response.read(MAX_RESPONSE + 1)
        if len(data) > MAX_RESPONSE:
            raise ValueError("NCBI response exceeds 1 MiB; submission status unknown")
        # A non-success status cannot prove that a POST was never accepted.
        if response.status != 200:
            raise ValueError(f"NCBI HTTP {response.status}; submission status unknown")
        return data.decode("utf-8", errors="replace")
    finally:
        if connection is not None:
            connection.close()
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def parse_response(text):
    block = re.search(r"QBlastInfoBegin(.*?)QBlastInfoEnd", text, re.DOTALL)
    if block:
        matches = re.findall(r"^\s*RID\s*=\s*([^\s]+)\s*$", block[1], re.MULTILINE)
        if len(matches) == 1 and RID.fullmatch(matches[0]):
            return "accepted", matches[0], "Submitted to NCBI; results open in your browser."
    # Only an explicit service error establishes known rejection. Other HTML,
    # missing/invalid IDs and truncated responses remain unknown, never retried.
    error = re.search(r"(?:Message ID#|Error:)([^<\r\n]{1,500})", text, re.IGNORECASE)
    if error:
        return "rejected", "", "NCBI rejected submission: " + error[1].strip()
    return "unknown", "", "Submission status unknown: NCBI did not return a valid RID. Do not automatically resubmit."


def fields_for(plan, parameters):
    fields = {"CMD": "Put", "QUERY": plan["fasta"], "PROGRAM": plan["program"],
              "DATABASE": plan["database"], "EXPECT": str(parameters["expect"]),
              "HITLIST_SIZE": str(parameters["max_hits"]), "FORMAT_TYPE": "HTML",
              "FILTER": "mL" if plan["program"] == "blastn" else "F",
              "tool": "Basecraft", "email": parameters["contact_email"]}
    if plan["database"] == "nt":
        fields["NO_DATABASE_OVERRIDE"] = "true"
    if plan["program"] == "blastn":
        fields["MEGABLAST"] = "on" if parameters["megablast"] else "off"
        fields["SHORT_QUERY_ADJUST"] = "false"
        fields["WORD_SIZE"] = "28" if parameters["megablast"] else "11"
        fields["NUCL_REWARD"] = "1" if parameters["megablast"] else "2"
        fields["NUCL_PENALTY"] = "-2" if parameters["megablast"] else "-3"
        fields["GAPCOSTS"] = "0 4" if parameters["megablast"] else "5 2"
    else:
        fields.update(MATRIX="BLOSUM62", COMPOSITION_BASED_STATISTICS="2",
                      WORD_SIZE="3" if plan["program"] == "blastp" else "5", GAPCOSTS="11 2")
    return fields


def write_receipt(path, receipt):
    """Allocated artifact; retain accepted identity before returning to host."""
    temporary = path.with_name(path.name + ".tmp")
    try:
        with temporary.open("w") as stream:
            json.dump(receipt, stream, ensure_ascii=False, allow_nan=False)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def submit_prepared(parameters, context, transport=post):
    # Runner replaces inputs with one shared, validated immutable preparation.
    paths = context["inputs"]["queries"]
    if len(paths) != 1:
        raise ValueError("BLAST requires shared host query preparation")
    plan = json.loads(Path(paths[0]).read_text())
    digest = "sha256:" + hashlib.sha256(plan["fasta"].encode()).hexdigest()
    if plan["schema"] != 1 or digest != plan["query_hash"]:
        raise ValueError("Prepared BLAST query identity mismatch")
    if plan["oversized"] and not (parameters["truncate"] and plan["truncation"]):
        raise ValueError("Oversized query has no explicit truncation approval")
    email = parameters["contact_email"]
    if not re.fullmatch(r"[^\s@]+@[^\s@]+\.[^\s@]+", email):
        raise ValueError("An authorized NCBI maintainer contact email is required")
    fields = fields_for(plan, parameters)
    cancel = Path(context["cancel_file"])
    receipt = {"schema": 1, "status": "cancelled", "rid": "", "url": "", "query_hash": plan["query_hash"],
                   "approval_hash": plan["approval_hash"], "program": plan["program"], "database": plan["database"],
                   "submitted_residues": plan["submitted_residues"], "plan": {k: v for k, v in plan.items() if k != "fasta"},
                   "request_fields": {k: v for k, v in fields.items() if k != "QUERY"},
                   "submitted_at": "", "estimated_seconds": None, "message": "Cancelled before upload; nothing sent."}
    if not cancel.exists():
        # Cancellation may race after this check. It cannot undo an accepted job.
        receipt["submitted_at"] = datetime.now(UTC).isoformat()
        try:
            text = transport(fields)
            status, rid, message = parse_response(text)
            estimate = re.search(r"^\s*RTOE\s*=\s*(\d{1,5})\s*$", text, re.MULTILINE)
            receipt["estimated_seconds"] = int(estimate[1]) if estimate else None
            receipt.update(status=status, rid=rid, message=message)
            if rid:
                receipt["url"] = ENDPOINT + "?" + urlencode({"CMD": "Get", "RID": rid, "FORMAT_TYPE": "HTML"})
        except (OSError, ValueError, http.client.HTTPException) as error:
            receipt.update(status="unknown", message=f"Submission status unknown: {error}. Do not automatically resubmit."[:2048])
    # Even cancellation after acceptance retains this RID. Receipt write failure
    # still returns accepted data; host attempts retention/publication, no retry.
    try:
        write_receipt(Path(context["outputs"]["receipt"]), receipt)
    except OSError as error:
        receipt["message"] = (receipt["message"] + f" Receipt write failed: {error}")[:2048]
    return {"ops": [], "data": receipt, "artifacts": [{"role": "receipt", "path": "."}]}


@plugin.context_operation("submit")
def submit(_snapshot, parameters, context):
    return submit_prepared(parameters, context)


def inspect_retained(receipt, plan):
    """Read an existing receipt after restart, with no service contact."""
    expected = "sha256:" + hashlib.sha256(plan["fasta"].encode()).hexdigest()
    if (plan["schema"] != 1 or plan["query_hash"] != expected
            or receipt["query_hash"] != expected
            or receipt["approval_hash"] != plan["approval_hash"]
            or receipt["plan"] != {k: v for k, v in plan.items() if k != "fasta"}):
        raise ValueError("Retained BLAST receipt does not match the frozen query")
    if receipt["status"] == "accepted":
        rid = receipt["rid"]
        if not RID.fullmatch(rid) or receipt["url"] != ENDPOINT + "?" + urlencode({"CMD": "Get", "RID": rid, "FORMAT_TYPE": "HTML"}):
            raise ValueError("Malformed retained RID/link")
    elif receipt["rid"] or receipt["url"]:
        raise ValueError("Non-accepted receipt contains an RID/link")
    return receipt


@plugin.context_operation("receipt")
def receipt(_snapshot, _parameters, context):
    data = inspect_retained(json.loads(Path(context["inputs"]["receipt"][0]).read_text()),
                            json.loads(Path(context["inputs"]["query"][0]).read_text()))
    return {"ops": [], "data": data}


if __name__ == "__main__":
    plugin.run()
