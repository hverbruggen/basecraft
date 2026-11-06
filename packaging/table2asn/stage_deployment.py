#!/usr/bin/env python3
"""Stage a qualified, private table2asn deployment into a candidate plugin copy.

This only stages retained metadata. The shared plugin installer remains the
owner of frozen installation, executable checks and atomic runtime selection.
"""

import argparse
import hashlib
import json
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def deployment_files(deployment, source=ROOT / "plugins/table2asn",
                     artifact=ROOT / "packaging/table2asn/linux-64.json"):
    manifest_bytes = (deployment / "plugin.toml").read_bytes()
    lock_bytes = (deployment / "pixi.lock").read_bytes()
    manifest = tomllib.loads(manifest_bytes.decode())
    expected = tomllib.loads((source / "plugin.toml").read_text())
    record = json.loads(artifact.read_text())
    if hashlib.sha256(lock_bytes).hexdigest() != record["qualified_deployment_lock_sha256"]:
        raise ValueError("deployment lock is not the qualified frozen lock")
    runtime = manifest.pop("runtime")
    requirements = expected.pop("runtime")
    if manifest != expected:
        raise ValueError("deployment changes the plugin identity or entry contract")
    if runtime.get("lock") != "pixi.lock":
        raise ValueError("deployment must use its retained pixi.lock")
    channels = runtime.get("channels", [])
    if len(channels) != 2 or channels[1] != "conda-forge":
        raise ValueError("deployment must retain the qualified channel order")
    channel = channels[0]
    if not isinstance(channel, str) or not channel.startswith("file:///"):
        raise ValueError("this qualified deployment requires an absolute local file channel")
    local = channel.removeprefix("file://")
    if any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/-_." for c in local):
        raise ValueError("invalid deployment channel")
    if any(part in (".", "..") for part in local.split("/")):
        raise ValueError("deployment channel must not contain traversal")
    if {k: v for k, v in runtime.items() if k not in ("lock", "channels")} != {
        k: v for k, v in requirements.items() if k not in ("lock", "channels")
    }:
        raise ValueError("deployment changes the pinned runtime requirements")
    package = record["package"]
    archive = Path(local) / record["target"] / package["filename"]
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if archive.stat().st_size != package["bytes"] or digest != package["sha256"]:
        raise ValueError("deployment package differs from the qualified artifact")
    return manifest_bytes, lock_bytes


def stage(deployment, plugin, source=ROOT / "plugins/table2asn",
          artifact=ROOT / "packaging/table2asn/linux-64.json"):
    plugin = plugin.resolve(strict=True)
    if plugin == source.resolve():
        raise ValueError("stage into a candidate plugin copy, never the source plugin")
    current = tomllib.loads((plugin / "plugin.toml").read_text())
    expected = tomllib.loads((source / "plugin.toml").read_text())
    # Permit an idempotent restage, but never replace a different plugin.
    if {k: v for k, v in current.items() if k != "runtime"} != {
        k: v for k, v in expected.items() if k != "runtime"
    }:
        raise ValueError("candidate is not the matching table2asn plugin")
    manifest, lock = deployment_files(deployment, source, artifact)
    # Candidate is unpublished. Write the lock before declaring its use; the
    # existing publisher only switches complete, verified bundles into place.
    for name, data in (("pixi.lock", lock), ("plugin.toml", manifest)):
        temporary = plugin / (name + ".partial")
        with temporary.open("xb") as stream:
            stream.write(data)
        temporary.replace(plugin / name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deployment", type=Path, required=True)
    parser.add_argument("--plugin", type=Path, required=True)
    args = parser.parse_args()
    try:
        stage(args.deployment, args.plugin)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"Cannot stage table2asn deployment: {error}\n")


if __name__ == "__main__":
    main()
