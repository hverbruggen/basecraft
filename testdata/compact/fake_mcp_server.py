"""Minimal stand-in for `basecraft mcp` used by the Python session tests.

It mirrors the real server's framing, pagination and structured error shapes
for a handful of tools. FAKE_MCP_MODE selects failure behaviour and
FAKE_MCP_LOG records the tool calls it received.
"""
import json
import os
import sys
import time

OPERATION = "org.example.fixture/count@1"
TOOL = "org.example.fixture.count.v1"
MODE = os.environ.get("FAKE_MCP_MODE", "normal")
LOG = os.environ.get("FAKE_MCP_LOG")
WRITABLE = "--allow-writes" in sys.argv


def log(entry):
    if LOG:
        with open(LOG, "a") as out:
            out.write(json.dumps(entry) + "\n")


def failure(operation, code, message):
    return {"schema": 1, "operation": operation, "state": "failed",
            "errors": [{"code": code, "message": message, "operation": operation}]}


def structured(value, failed):
    return {"content": [{"type": "text", "text": json.dumps(value)}], "isError": failed,
            "structuredContent": value}


TOOLS = [
    {"name": "open", "inputSchema": {"type": "object"}},
    {"name": "read", "inputSchema": {"type": "object"}},
    {"name": "apply", "inputSchema": {"type": "object"}},
    {"name": "save", "inputSchema": {"type": "object"}},
    {"name": TOOL, "description": "Count bytes.",
     "inputSchema": {"type": "object", "properties": {
         "operation": {"const": OPERATION},
         "parameters": {"type": "object", "additionalProperties": False,
                        "properties": {"mode": {"enum": ["normal", "slow"]}}}}},
     "outputSchema": {"type": "object"}, "annotations": {"readOnlyHint": True},
     "_meta": {"basecraft": {"pins": {"generation": "g"}}}},
]
document = {"document": 0, "path": "gene.fasta", "kind": "sequence", "version": 0}
jobs = {}


def call(name, args):
    if name in ("apply", "save") and not WRITABLE:
        return structured(failure(name, "invalid_arguments", "This server is read-only; "
                                  "start it with --allow-writes to enable edits"), True)
    if name == "open":
        return structured(document, False)
    if name == "read":
        return structured({"residues": "ACGT"[args.get("start", 0):args["end"]]}, False)
    if name == "apply":
        if args["version"] != document["version"]:
            return structured(failure(name, "invalid_arguments", "Document version changed"), True)
        document["version"] += 1
        return structured(document, False)
    if name == "save":
        return structured(dict(document, saved=args.get("path", document["path"])), False)
    if name == TOOL:
        if set(args.get("parameters", {})) - {"mode"}:
            return structured(failure(OPERATION, "invalid_arguments", "unknown parameter"), True)
        run_id = f"run-{len(jobs)}"
        mode = args.get("parameters", {}).get("mode", "normal")
        jobs[run_id] = {"mode": mode, "state": "running", "polls": 0}
        return structured({"schema": 1, "operation": OPERATION, "run_id": run_id,
                           "state": "running"}, False)
    if name.startswith("basecraft.host.jobs_"):
        job = jobs.get(args.get("job"))
        if job is None:
            return structured(failure("basecraft.host", "invalid_arguments", "unknown job"), True)
        kind = name.split("_")[1].split(".")[0]
        if kind == "cancel" and job["state"] == "running":
            job["state"] = "cancelled"
        elif kind == "get" and job["mode"] == "normal":
            job["polls"] += 1
            if job["polls"] > 1:
                job["state"] = "completed"
        elif kind == "result" and job["state"] == "running":
            return structured(failure(OPERATION, "invalid_arguments", "job is not terminal"), True)
        outcome = {"schema": 1, "operation": OPERATION, "run_id": args["job"],
                   "state": job["state"]}
        if job["state"] == "completed":
            outcome["data"] = {"bytes": 7}
        return structured(outcome, job["state"] in ("failed", "cancelled", "partial"))
    return None


def main():
    if MODE == "crash":
        print("basecraft: project folder does not exist", file=sys.stderr)
        sys.exit(1)
    initialized = False
    for line in sys.stdin.buffer:
        request = json.loads(line)
        if "id" not in request:
            continue
        method, params, reply = request["method"], request.get("params", {}), None
        if method == "initialize":
            initialized = True
            reply = {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
                     "serverInfo": {"name": "fake", "version": "0"}}
        elif not initialized:
            reply = {"error": {"code": -32002, "message": "Initialize first"}}
        elif method == "tools/list":
            offset = int(params.get("cursor", "g:0").split(":")[1])
            reply = {"tools": TOOLS[offset:offset + 2]}
            if offset + 2 < len(TOOLS):
                reply["nextCursor"] = f"g:{offset + 2}"
        elif method == "tools/call":
            log({"tool": params["name"], "arguments": params.get("arguments", {})})
            if MODE == "oversize" and params["name"] == "read":
                sys.stdout.write("x" * (8 * 1024 * 1024 + 2) + "\n")
                sys.stdout.flush()
                continue
            if MODE in ("garbage", "array") and params["name"] == "read":
                sys.stdout.write("not json\n" if MODE == "garbage" else "[]\n")
                sys.stdout.flush()
                continue
            if MODE == "late" and params["name"] == "read":
                time.sleep(0.5)
            result = call(params["name"], params.get("arguments", {}))
            reply = ({"error": {"code": -32602, "message": "Unknown tool"}}
                     if result is None else {"result": result})
        else:
            reply = {"error": {"code": -32601, "message": "Method not found"}}
        if "result" not in reply and "error" not in reply:
            reply = {"result": reply}
        sys.stdout.write(json.dumps(dict(reply, jsonrpc="2.0", id=request["id"])) + "\n")
        sys.stdout.flush()
    log({"eof": sorted(k for k, job in jobs.items() if job["state"] == "running"),
         "argv": sys.argv[1:]})


if __name__ == "__main__":
    main()
