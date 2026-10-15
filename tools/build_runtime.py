#!/usr/bin/env python3
"""Snapshot installed Arch build packages for an offline build-host container.

Only package-owned /usr files and the selected Rust toolchain are exported.
No home configuration, credentials, project data or host /etc is included.
"""

import gzip
import hashlib
import json
import os
import re
import subprocess
import tarfile
from pathlib import Path


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def packages():
    installed = {}
    providers = {}
    for line in output("expac", "%n\t%D\t%P").splitlines():
        name, dependencies, provides = (line + "\t\t").split("\t")[:3]
        installed[name] = dependencies.split()
        for provided in provides.split():
            providers.setdefault(re.split(r"[<>=]", provided)[0], name)
    pending = [
        "gcc",
        "binutils",
        "make",
        "cmake",
        "pkgconf",
        "bash",
        "coreutils",
        "file",
        "findutils",
        "qt6-base",
        "qt6-declarative",
        "qt6-shadertools",
    ]
    selected = set()
    while pending:
        dependency = re.split(r"[<>=]", pending.pop())[0]
        name = dependency if dependency in installed else providers.get(dependency)
        if name is None:
            raise RuntimeError(
                f"Cannot resolve installed build dependency: {dependency}"
            )
        if name not in selected:
            selected.add(name)
            pending.extend(installed[name])
    return sorted(selected)


def runtime(destination):
    selected = packages()
    toolchain = Path(output("rustc", "--print", "sysroot"))
    metadata = {
        "format": 3,
        "packages": output("pacman", "-Q", *selected),
        "rust": output("rustc", "-vV"),
        "qt": output("qmake6", "-query", "QT_VERSION"),
    }
    identity = hashlib.sha256(
        json.dumps(metadata, sort_keys=True).encode()
    ).hexdigest()[:20]
    archive = destination / f"runtime-{identity}.tar.gz"
    if archive.exists():
        return archive, identity, metadata
    paths = output("pacman", "-Qql", *selected).splitlines()
    temporary = archive.with_suffix(".partial")
    print(
        f"Snapshotting {len(selected)} installed build packages and Rust {identity}",
        flush=True,
    )
    with (
        gzip.open(temporary, "wb", compresslevel=1) as compressed,
        tarfile.open(fileobj=compressed, mode="w|") as tar,
    ):
        for name in sorted(set(paths)):
            path = Path(name)
            if not (
                name.startswith("/usr/") or name in ("/bin", "/sbin", "/lib", "/lib64")
            ):
                continue
            if name.startswith(
                (
                    "/usr/share/doc/",
                    "/usr/share/man/",
                    "/usr/share/locale/",
                    "/usr/share/factory/",
                )
            ):
                continue
            if path.is_dir() and not path.is_symlink():
                continue
            # Privileged host service helpers are unnecessary for compilation.
            if not path.is_symlink() and path.exists() and path.stat().st_mode & 0o6000:
                continue
            if not path.is_symlink() and not os.access(path, os.R_OK):
                print(f"Omitting unreadable host package file: {path}", flush=True)
                continue
            if path.exists() or path.is_symlink():
                tar.add(path, arcname=name.lstrip("/"), recursive=False)
        for name in ("bin", "lib", "libexec", "etc"):
            if (toolchain / name).exists():
                tar.add(toolchain / name, arcname=f"opt/rust/{name}")
        for name in ("tmp", "work"):
            entry = tarfile.TarInfo(name)
            entry.type = tarfile.DIRTYPE
            entry.mode = 0o1777
            tar.addfile(entry)
    temporary.replace(archive)
    archive.with_suffix(".json").write_text(json.dumps(metadata, indent=2) + "\n")
    return archive, identity, metadata
