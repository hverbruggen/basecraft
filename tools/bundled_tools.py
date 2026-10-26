"""Stage pinned upstream tools in a candidate bundle, before atomic publication."""
import hashlib
import json
import os
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ASSETS = (
    ("https://github.com/Pas-Kapli/mptp/releases/download/v0.2.5/mptp-0.2.5-linux-x86_64.tar.gz",
     "397f7431c82d4189a0b3d93421fd649f087970b616db1a0875f86027164e7867", {
         "mptp-0.2.5-linux-x86_64/bin/mptp": "bin/mptp",
         "mptp-0.2.5-linux-x86_64/LICENSE.txt": "share/mptp/LICENSE.txt",
         "mptp-0.2.5-linux-x86_64/README.md": "share/mptp/README.md",
         "mptp-0.2.5-linux-x86_64/doc/mptp_manual.pdf": "share/mptp/mptp_manual.pdf",
     }),
    ("https://github.com/Pas-Kapli/mptp/releases/download/v0.2.5/mptp-src-0.2.5.tar.gz",
     "ffb3714abb9530c253593b4434c95a3ab6c818340f2f9c829c646ed26c52141d",
     "share/mptp/mptp-src-0.2.5.tar.gz"),
    ("https://github.com/prefix-dev/pixi/releases/download/v0.83.0/pixi-x86_64-unknown-linux-musl.tar.gz",
     "064f371e692aaa3400ac581c89da2843c91437f20c2d9155395a5d0037f33551", {
         "pixi": "bin/pixi",
     }),
    ("https://raw.githubusercontent.com/prefix-dev/pixi/v0.83.0/LICENSE",
     "679d42a76b82a6dc20ffd9048ab45fb2e5557a526113aaedc7419e12f3cfab38",
     "share/pixi/LICENSE"),
)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def cached_asset(cache, url, expected):
    cache.mkdir(parents=True, exist_ok=True)
    target = cache / expected
    if target.is_file():
        if digest(target) != expected:
            raise RuntimeError(f"Cached tool download failed SHA-256 verification: {target}")
        return target
    with tempfile.NamedTemporaryFile(dir=cache, delete=False) as stream:
        pending = Path(stream.name)
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                total = 0
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > 128 * 1024 * 1024:
                        raise RuntimeError(f"Tool download exceeds 128 MiB: {url}")
                    stream.write(chunk)
            stream.flush()
            if digest(pending) != expected:
                raise RuntimeError(f"Tool download failed SHA-256 verification: {url}")
            os.replace(pending, target)
        finally:
            pending.unlink(missing_ok=True)
    return target


def copy_member(archive, member_name, destination):
    # Copy explicitly selected regular members, never extract archive paths.
    member = archive.getmember(member_name)
    if not member.isfile() or member.size > 128 * 1024 * 1024:
        raise RuntimeError(f"Invalid tool archive member: {member_name}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with archive.extractfile(member) as source, destination.open("xb") as output:
        shutil.copyfileobj(source, output)


def stage_tools(bundle, cache):
    if (platform.system(), platform.machine()) != ("Linux", "x86_64"):
        raise RuntimeError("Bundled tools currently require Linux x86_64")
    receipts = []
    for url, expected, members in ASSETS:
        asset = cached_asset(cache, url, expected)
        if isinstance(members, dict):
            with tarfile.open(asset) as archive:
                for member, relative in members.items():
                    copy_member(archive, member, bundle / relative)
        else:
            destination = bundle / members
            destination.parent.mkdir(parents=True, exist_ok=True)
            with asset.open("rb") as source, destination.open("xb") as output:
                shutil.copyfileobj(source, output)
        receipts.append({"url": url, "sha256": expected, "members": members})
    tools = {}
    for name, expected_version in (("mptp", "0.2.5"), ("pixi", "0.83.0")):
        executable = bundle / "bin" / name
        executable.chmod(0o755)
        result = subprocess.run([str(executable), "--version"], capture_output=True,
                                text=True, timeout=15, check=True)
        version = result.stdout.strip()
        if expected_version not in version:
            raise RuntimeError(f"Unexpected {name} version: {version}")
        tools[name] = {"sha256": digest(executable), "version": version}
    metadata = {"schema": 1, "downloads": receipts, "tools": tools}
    (bundle / "bundled-tools.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata
