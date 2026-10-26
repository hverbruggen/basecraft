#!/usr/bin/env python3
"""Stage an already validated native binary into an isolated Sanderson package.

This script never builds or installs dependencies. Pass a disposable package
copy and the executable from the matching build-host validation receipt.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import tempfile


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stage(package, binary):
    package = package.resolve(strict=True)
    binary = binary.resolve(strict=True)
    if not (package / "plugin.toml").is_file():
        raise ValueError("expected a staged Sanderson plugin package")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise ValueError("validated binary targets Linux x86_64 only")
    if not binary.is_file():
        raise ValueError("native executable is not a regular file")
    with binary.open("rb") as stream:
        magic = stream.read(4)
    if magic != b"\x7fELF":
        raise ValueError("native executable is not an ELF regular file")
    expected = json.loads((package / "native/validated-engine.json").read_text())
    if expected.get("name") != "basecraft-sanderson-native" or expected.get("schema") != 1:
        raise ValueError("invalid validated engine identity")
    if sha256(binary) != expected["binary_sha256"]:
        raise ValueError("native executable differs from validated binary")
    if sha256(package / "native/Cargo.lock") != expected["cargo_lock_sha256"]:
        raise ValueError("standalone dependency lockfile differs from validation")
    for relative, digest in expected["source_sha256"].items():
        if sha256(package / "native" / relative) != digest:
            raise ValueError(f"native source differs from validation: {relative}")
    if not (package / "LICENSE-GPL").is_file():
        raise ValueError("the GPL-3 license notice is missing")
    directory = package / "bin"
    if directory.is_symlink():
        raise ValueError("binary directory must not be a symlink")
    directory.mkdir(exist_ok=True)
    target = directory / "basecraft-sanderson-native"
    if target.exists() and sha256(target) != expected["binary_sha256"]:
        raise ValueError("a different native executable is already staged")
    with tempfile.NamedTemporaryFile(dir=directory, prefix=".native-", delete=False) as stream:
        temporary = Path(stream.name)
    try:
        shutil.copyfile(binary, temporary)
        temporary.chmod(0o755)
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    metadata = directory / "engine.json"
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=directory,
                                     prefix=".engine-", delete=False) as stream:
        temporary = Path(stream.name)
        json.dump(expected, stream, indent=2, sort_keys=True)
        stream.write("\n")
    try:
        os.replace(temporary, metadata)
    finally:
        temporary.unlink(missing_ok=True)
    return {"binary": str(target), "binary_sha256": expected["binary_sha256"],
            "source_commit": expected["source_commit"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(stage(args.package, args.binary), sort_keys=True))


if __name__ == "__main__":
    main()
