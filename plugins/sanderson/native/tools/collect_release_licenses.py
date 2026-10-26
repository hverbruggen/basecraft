#!/usr/bin/env python3
"""Collect auditable notices for the validated Linux release dependency graph.

This is an offline packaging preparation step, not a build. Inputs are Cargo
metadata and the registry archives/cache used for the validated release. The
two Argmin crates omit license files from their published archives; supply
LICENSE-MIT and LICENSE-APACHE fetched from their exact upstream release tags.
"""

import argparse
import hashlib
import json
import shutil
import tomllib
from pathlib import Path

ARGMIN_UPSTREAM = {
    "argmin-0.11.0": "argmin-v0.11.0",
    "argmin-math-0.5.1": "argmin-math-v0.5.1",
}
UPSTREAM_HASHES = {
    "LICENSE-MIT": "23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3",
    "LICENSE-APACHE": "11f10b8009ae95f6213f13bf12cfb048e002abf4e05af77320dc248fb3efa404",
}
LINUX_GETRANDOM = ('cfg(all(any(target_os = "linux", target_os = "android"), '
                   'not(any(all(target_os = "linux", target_env = ""), '
                   'getrandom_backend = "custom", getrandom_backend = "linux_raw", '
                   'getrandom_backend = "rdrand", getrandom_backend = "rndr"))))')


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def release_ids(metadata):
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    seen = {root}
    pending = [root]
    while pending:
        for dep in nodes[pending.pop()]["deps"]:
            for kind in dep["dep_kinds"]:
                target = kind["target"]
                if kind["kind"] not in (None, "build"):
                    continue
                if target in (None, "cfg(unix)", LINUX_GETRANDOM):
                    if dep["pkg"] not in seen:
                        seen.add(dep["pkg"])
                        pending.append(dep["pkg"])
                    break
                # This pinned metadata contains only non-Linux alternatives
                # beyond the three accepted expressions. Refuse new Linux
                # expressions until their target conditions are reviewed.
                if 'target_os = "linux"' in target:
                    raise ValueError(f"unreviewed Linux dependency condition: {target}")
    return seen


def collect(metadata_path, lock_path, registry, upstream, output):
    metadata = json.loads(metadata_path.read_text())
    lock = tomllib.loads(lock_path.read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum")
                 for p in lock["package"]}
    by_id = {p["id"]: p for p in metadata["packages"]}
    selected = [by_id[ident] for ident in release_ids(metadata)]
    root = [p for p in selected if p["name"] == "basecraft-sanderson-native"]
    if len(root) != 1:
        raise ValueError("expected one native method root")
    packages = sorted((p for p in selected if p is not root[0]),
                      key=lambda p: (p["name"], p["version"]))
    if len(packages) != 49:
        raise ValueError(f"unexpected Linux release closure size: {len(packages)}")
    output.mkdir()
    records = []
    for package in packages:
        name, version = package["name"], package["version"]
        key = f"{name}-{version}"
        source_matches = list((registry / "src").glob("*/" + key))
        archive_matches = list((registry / "cache").glob("*/" + key + ".crate"))
        if len(source_matches) != 1 or len(archive_matches) != 1:
            raise ValueError(f"expected one cached source/archive pair: {key}")
        source, archive = source_matches[0], archive_matches[0]
        checksum = checksums[(name, version)]
        if checksum is None or digest(archive) != checksum:
            raise ValueError(f"Cargo.lock/archive identity mismatch: {key}")
        if not source.is_dir():
            raise ValueError(f"missing extracted crate source: {key}")
        destination = output / key
        destination.mkdir()
        files = []
        candidates = sorted(p for p in source.iterdir() if p.is_file()
                            and p.name.lower().startswith(("license", "licence", "notice",
                                                           "copying", "copyright")))
        for source_file in candidates + [source / "Cargo.toml"]:
            if not source_file.is_file():
                raise ValueError(f"missing Cargo manifest: {key}")
            target = destination / source_file.name
            shutil.copyfile(source_file, target)
            files.append({"name": target.name, "sha256": digest(target),
                          "origin": "crate archive"})
        if key in ARGMIN_UPSTREAM:
            if candidates:
                raise ValueError(f"Argmin crate unexpectedly ships license files: {key}")
            tag = ARGMIN_UPSTREAM[key]
            for filename, expected_hash in UPSTREAM_HASHES.items():
                external = upstream / f"{tag}-{filename}"
                if digest(external) != expected_hash:
                    raise ValueError(f"upstream release-tag license changed: {tag}/{filename}")
                target = destination / filename
                shutil.copyfile(external, target)
                files.append({"name": filename, "sha256": digest(target),
                              "origin": f"https://raw.githubusercontent.com/argmin-rs/argmin/{tag}/{filename}"})
        if not any(item["name"].lower().startswith(("license", "licence", "notice",
                                                     "copying", "copyright")) for item in files):
            raise ValueError(f"no license text or notice: {key}")
        records.append({"name": name, "version": version, "license": package["license"],
                        "authors": package.get("authors") or [],
                        "crate_sha256": checksum, "files": files})
    result = {"schema": 1, "target": "x86_64-unknown-linux-gnu",
              "selection": "Cargo metadata normal/build dependency edges for Linux GNU; no dev or other-target edges",
              "cargo_lock_sha256": digest(lock_path),
              "cargo_metadata_sha256": digest(metadata_path),
              "package_count": len(records), "packages": records}
    (output / "manifest.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("metadata", "lock", "registry", "upstream", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    manifest = collect(args.metadata, args.lock, args.registry, args.upstream, args.output)
    print(json.dumps({"package_count": manifest["package_count"],
                      "cargo_lock_sha256": manifest["cargo_lock_sha256"]}, sort_keys=True))


if __name__ == "__main__":
    main()
