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
import tomllib


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_licenses(package, expected):
    source = package / "native" / "licenses"
    inventory = json.loads((source / "manifest.json").read_text())
    if inventory.get("schema") != 1 or inventory.get("target") != expected["target"]:
        raise ValueError("invalid release dependency license inventory")
    if inventory.get("cargo_lock_sha256") != expected["cargo_lock_sha256"]:
        raise ValueError("dependency inventory uses a different Cargo.lock")
    packages = inventory.get("packages")
    if inventory.get("package_count") != 49 or not isinstance(packages, list) or len(packages) != 49:
        raise ValueError("unexpected release dependency inventory size")
    lock = tomllib.loads((package / "native" / "Cargo.lock").read_text())
    locked = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    seen = set()
    for item in packages:
        key = (item["name"], item["version"])
        if key in seen or locked.get(key) != item["crate_sha256"]:
            raise ValueError(f"invalid release dependency identity: {key}")
        seen.add(key)
        if not item.get("license") or not item.get("files"):
            raise ValueError(f"missing license declaration/text: {key}")
    verify_license_files(source, packages)
    return source, inventory


def verify_license_files(source, packages):
    for item in packages:
        key = (item["name"], item["version"])
        directory = source / f"{key[0]}-{key[1]}"
        for file in item["files"]:
            name = file["name"]
            if Path(name).name != name or sha256(directory / name) != file["sha256"]:
                raise ValueError(f"dependency notice changed: {key}/{name}")


def notice_text(inventory):
    lines = ["# Sanderson native engine: third-party notices", "",
             "The executable is distributed under GPL-3.0-or-later; its notice is `LICENSE-GPL`.",
             "The table covers the pinned Linux x86_64 release's normal and build dependency graph.",
             "License expressions and authors come from Cargo metadata. Source archive SHA-256",
             "values are checked against the isolated Cargo.lock; each original license, notice,",
             "copyright or manifest file is copied under `licenses/` with its SHA-256 in",
             "`licenses/manifest.json`. For Argmin's two crates, the published archives omit",
             "license files; the manifest records exact upstream release-tag URLs and hashes.",
             "An expression with `OR` permits a choice of terms; the copied corpus retains",
             "the available alternatives. `unicode-ident` additionally requires Unicode-3.0.",
             "", "| Crate | Declared license | Authors | Texts and notices |", "|---|---|---|---|"]
    for item in inventory["packages"]:
        package = f'{item["name"]}-{item["version"]}'
        authors = ", ".join(item["authors"]) or "See crate manifest"
        files = ", ".join(f'[{file["name"]}](licenses/{package}/{file["name"]})'
                          for file in item["files"] if file["name"] != "Cargo.toml")
        lines.append(f'| {package} | {item["license"]} | {authors.replace("|", "/")} | {files} |')
    return "\n".join(lines) + "\n"


def atomic_copy(source, destination, mode=None):
    with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".notice-", delete=False) as stream:
        temporary = Path(stream.name)
    try:
        shutil.copyfile(source, temporary)
        if mode is not None:
            temporary.chmod(mode)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_text(value, destination):
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=destination.parent,
                                     prefix=".notice-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(value)
    try:
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


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
    license_source, inventory = verify_licenses(package, expected)
    directory = package / "bin"
    if directory.is_symlink():
        raise ValueError("binary directory must not be a symlink")
    directory.mkdir(exist_ok=True)
    target = directory / "basecraft-sanderson-native"
    if target.exists() and sha256(target) != expected["binary_sha256"]:
        raise ValueError("a different native executable is already staged")
    notices = directory / "licenses"
    if notices.is_symlink():
        raise ValueError("dependency notices directory must not be a symlink")
    if notices.exists():
        if sha256(notices / "manifest.json") != sha256(license_source / "manifest.json"):
            raise ValueError("different dependency notices are already staged")
        verify_license_files(notices, inventory["packages"])
    else:
        temporary = Path(tempfile.mkdtemp(dir=directory, prefix=".licenses-"))
        try:
            shutil.copytree(license_source, temporary, dirs_exist_ok=True)
            os.replace(temporary, notices)
        finally:
            if temporary.exists():
                shutil.rmtree(temporary)
    atomic_copy(package / "LICENSE-GPL", directory / "LICENSE-GPL")
    atomic_text(notice_text(inventory), directory / "THIRD_PARTY_NOTICES.md")
    atomic_copy(binary, target, 0o755)
    metadata = directory / "engine.json"
    atomic_text(json.dumps(expected, indent=2, sort_keys=True) + "\n", metadata)
    return {"binary": str(target), "binary_sha256": expected["binary_sha256"],
            "source_commit": expected["source_commit"],
            "notices_sha256": sha256(directory / "THIRD_PARTY_NOTICES.md"),
            "dependency_count": inventory["package_count"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(stage(args.package, args.binary), sort_keys=True))


if __name__ == "__main__":
    main()
