#!/usr/bin/env python3
"""Independent Basecraft v1 reader; hash verification optionally needs blake3.

python verify_provenance.py RESULT --metadata-only
python verify_provenance.py RESULT --bundle PATH_TO_BUNDLE

Only reads declared JSON and artifact bytes. Never executes bundle contents.
"""
import argparse
import json
import re
from pathlib import Path


def relative(root, name):
    path = Path(name)
    if not name or path.is_absolute() or any(part in ("..", ".") for part in path.parts):
        raise ValueError(f"unsafe bundle path: {name}")
    resolved = (root / path).resolve(strict=True)
    if not resolved.is_relative_to(root.resolve(strict=True)):
        raise ValueError(f"bundle path escapes through symlink: {name}")
    return resolved


def record(path):
    if path.is_symlink() or path.stat().st_size > 64 * 1024 * 1024:
        raise ValueError(f"unsafe or oversized record: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def discover(result):
    store = result.parent / ".basecraft" / "analyses-v1"
    if store.exists():
        marker = record(store / "store.json")
        if marker.get("format") != "basecraft.analysis-store" or marker.get("schema_version") != 1:
            raise ValueError("foreign or unsupported analysis namespace")
        bindings = store / "bindings"
        if bindings.is_symlink() or store.is_symlink():
            raise ValueError("unsafe analysis namespace")
        found = []
        if bindings.is_dir():
            for index, path in enumerate(bindings.iterdir()):
                if index >= 4096:
                    raise ValueError("too many result bindings")
                binding = record(path)
                if binding.get("result") == result.name:
                    if binding.get("schema_version") != 1 or not re.fullmatch(r"[A-Za-z0-9-]+", binding["run"]):
                        raise ValueError("unsupported or unsafe binding")
                    run = relative(store, "runs/" + binding["run"])
                    if (run / "manifest.json").is_file():
                        found.append(run)
        if len(found) > 1:
            raise ValueError("ambiguous result binding")
        if found:
            return found[0]
    return Path(str(result) + ".provenance")


def source(bundle, reference):
    seen = set()
    for _ in range(32):
        bundle = bundle.resolve(strict=True)
        if bundle in seen:
            raise ValueError("input evidence cycle")
        seen.add(bundle)
        evidence = record(bundle / "input.json")
        if evidence.get("schema_version") != 1 or evidence.get("state") == "broken":
            raise ValueError("input evidence unavailable or explicitly broken")
        if "origin_run" not in evidence:
            break
        bundle = bundle / evidence["origin_run"]
    else:
        raise ValueError("input evidence exceeds 32 references")
    if evidence.get("source", {}).get("hash") != reference["hash"]:
        raise ValueError("input reference identity differs")
    return bundle / ("preserved-input" if evidence["state"] == "preserved" else evidence["source"]["path"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("result", type=Path)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--metadata-only", action="store_true")
    args = parser.parse_args()
    bundle = args.bundle or discover(args.result)
    if not args.metadata_only:
        try:
            from blake3 import blake3
        except ImportError as error:
            raise ValueError("hash verification needs blake3; --metadata-only checks records and sizes without verifying hashes") from error
    # manifest.json names the latest record; each record holds one save.
    latest = record(bundle / "manifest.json")
    if latest.get("schema_version") != 1 or not re.fullmatch(r"[A-Za-z0-9-]+", latest["id"]):
        raise ValueError("unsupported or unsafe history manifest")
    publication = record(relative(bundle, f"records/{latest['id']}.json"))
    if publication["schema_version"] != 1 or not publication["outputs"]:
        raise ValueError("unsupported or empty publication")

    total_bytes = 0
    if not args.metadata_only:
        hashing = blake3()
        hashing.update(relative(bundle, f"records/{latest['id']}.json").read_bytes())
        if "b3:" + hashing.hexdigest() != latest["hash"]:
            raise ValueError("latest record checksum differs")

    def digest(path):
        if args.metadata_only:
            return None
        hashing = blake3()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                hashing.update(block)
        return "b3:" + hashing.hexdigest()

    def check(path, artifact):
        nonlocal total_bytes
        total_bytes += path.stat().st_size
        if total_bytes > 8 * 1024**3:
            raise ValueError("artifact verification exceeds 8 GiB")
        if path.stat().st_size != artifact["size"]:
            raise ValueError(f"size differs: {path}")
        if not args.metadata_only and digest(path) != artifact["hash"]:
            raise ValueError(f"hash differs: {path}")

    pending = [(bundle, publication, False)]
    seen = set()
    active = set()
    while pending:
        location, current, leaving = pending.pop()
        identity = (location.resolve(strict=True), current["id"])
        if leaving:
            active.remove(identity)
            continue
        if identity in active:
            raise ValueError("evidence ancestry cycle")
        if identity in seen:
            continue
        seen.add(identity)
        active.add(identity)
        pending.append((location, current, True))
        if len(seen) > 10000:
            raise ValueError("too many ancestor records")
        if current["schema_version"] != 1:
            raise ValueError("unsupported ancestor version")
        for artifact in current["inputs"] + current["outputs"]:
            if artifact.get("retained") is not None:
                check(relative(location, artifact["retained"]), artifact)
            elif artifact.get("reference") is not None:
                check(source(location, artifact["reference"]), artifact)
            elif artifact in current["inputs"]:
                raise ValueError(f"input bytes unavailable: {artifact['id']}")
        ancestors = [(location, parent, current["parent_hashes"][parent]) for parent in current["parents"]]
        if (location / "input.json").is_file():
            evidence = record(location / "input.json")
            if evidence.get("state") == "broken":
                raise ValueError("input evidence explicitly broken")
            ledgers = [(location, evidence)]
            ledger_seen = set()
            while ledgers:
                ledger_root, ledger = ledgers.pop()
                canonical = ledger_root.resolve(strict=True)
                if canonical in ledger_seen:
                    raise ValueError("input evidence cycle")
                ledger_seen.add(canonical)
                for ancestor in ledger.get("upstream", []):
                    ancestors.append((ledger_root / ancestor["bundle"], ancestor["id"], ancestor["hash"]))
                if "origin_run" in ledger:
                    origin = ledger_root / ledger["origin_run"]
                    ledgers.append((origin, record(origin / "input.json")))
        for directory, identity, expected in ancestors:
            if not re.fullmatch(r"[A-Za-z0-9-]+", identity):
                raise ValueError("unsafe ancestor identity")
            path = relative(directory, f"records/{identity}.json")
            if not args.metadata_only and digest(path) != expected:
                raise ValueError("ancestor checksum differs")
            ancestor = record(path)
            if ancestor["id"] != identity:
                raise ValueError("ancestor identity differs")
            pending.append((directory, ancestor, False))
    # A result bound to a hidden analysis run is one of several published files;
    # every output is named relative to the project folder, as the Rust reader does.
    hidden = ".basecraft" in bundle.resolve(strict=True).parts
    for index, artifact in enumerate(publication["outputs"]):
        if index == 0 and not hidden:
            live = args.result
        else:
            live = relative(args.result.parent, artifact["logical_name"])
        check(live, artifact)
    # The edits in effect: each record adds its own and withdraws undone ones,
    # parents first.
    effective = []
    order = []
    # A record is visited when it is expanded, so one reached through two
    # children still comes after all its own parents.
    walk = [(publication, False)]
    walked = set()
    while walk:
        current, done = walk.pop()
        if done:
            order.append(current)
            continue
        if current["id"] in walked:
            continue
        walked.add(current["id"])
        walk.append((current, True))
        for parent in reversed(current["parents"]):
            if parent not in walked:
                walk.append((record(relative(bundle, f"records/{parent}.json")), False))
    for current in order:
        withdrawn = set(current["withdrawn"])
        effective = [a for a in effective if a["id"] not in withdrawn]
        effective += [a for a in current["effective_edits"] if all(a["id"] != e["id"] for e in effective)]
    print(json.dumps({"publication": publication["id"], "effective_operations": [a["operation"] for a in effective], "completeness": publication["completeness"], "hashes_checked": not args.metadata_only, "records_checked": len(seen)}, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"fail: {error}") from error
