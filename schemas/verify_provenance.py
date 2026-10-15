#!/usr/bin/env python3
"""Independent Basecraft v1 bundle reader. Optional hash verification needs blake3.

python verify_provenance.py RESULT --metadata-only
python verify_provenance.py RESULT    # requires the open-source blake3 Python package

Only reads JSON and artifact bytes; never executes bundle contents.
"""
import argparse
import json
from pathlib import Path


def relative(root, name):
    path = Path(name)
    if not name or path.is_absolute() or any(part in ("..", ".") for part in path.parts):
        raise ValueError(f"unsafe bundle path: {name}")
    resolved = (root / path).resolve(strict=True)
    if not resolved.is_relative_to(root.resolve(strict=True)):
        raise ValueError(f"bundle path escapes through symlink: {name}")
    return resolved


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("result", type=Path)
    parser.add_argument("--metadata-only", action="store_true")
    args = parser.parse_args()
    bundle = Path(str(args.result) + ".provenance")
    manifest = bundle / "manifest.json"
    if manifest.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("manifest exceeds 64 MiB")
    publication = json.loads(manifest.read_text(encoding="utf-8"))
    if publication["schema_version"] != 1:
        raise ValueError("unsupported schema version")
    if not publication["outputs"]:
        raise ValueError("publication has no outputs")
    if not args.metadata_only:
        try:
            from blake3 import blake3
        except ImportError as error:
            raise ValueError("hash verification needs the open-source blake3 Python package; --metadata-only reads records without verifying hashes") from error
    def check(path, artifact):
        if path.stat().st_size != artifact["size"]:
            raise ValueError(f"size differs: {path.name}")
        if not args.metadata_only:
            digest = blake3()
            with path.open("rb") as source:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(block)
            if "b3:" + digest.hexdigest() != artifact["hash"]:
                raise ValueError(f"hash differs: {path.name}")
    for artifact in publication["inputs"] + publication["outputs"]:
        retained = artifact.get("retained")
        if retained is None:
            raise ValueError(f"input bytes unavailable: {artifact['id']}")
        check(relative(bundle, retained), artifact)
    for index, artifact in enumerate(publication["outputs"]):
        live = args.result if index == 0 else relative(args.result.parent, artifact["logical_name"])
        check(live, artifact)
    print(json.dumps({"publication": publication["id"], "effective_operations": [a["operation"] for a in publication["effective_edits"]], "completeness": publication["completeness"], "hashes_checked": not args.metadata_only}, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"fail: {error}") from error
