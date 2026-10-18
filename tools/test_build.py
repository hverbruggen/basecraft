#!/usr/bin/env python3
"""Build on build-host with the local ABI, validate locally, publish atomically."""

import argparse
import fcntl
import hashlib
import io
import json
import os
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import time
from pathlib import Path

from build_runtime import output, runtime

REPO = Path(__file__).resolve().parents[1]
BINARIES = (
    "basecraft-gui",
    "basecraft",
    "basecraft-distance",
    "basecraft-delimitation",
)


def run(args, **kwargs):
    print("+ " + shlex.join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def ci_receipt(commit):
    """The workspace's passing remote CI receipt for a clean checkout of `commit`."""
    if output("git", "-C", str(REPO), "status", "--porcelain"):
        return None
    path = REPO.parent / "records" / "validation" / f"{commit}.json"
    try:
        receipt = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    return path if receipt.get("success") and receipt.get("commit") == commit else None


def snapshot(destination):
    names = (
        subprocess.check_output(
            [
                "git",
                "-C",
                str(REPO),
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ]
        )
        .decode()
        .split("\0")
    )
    archive = destination / "source.tar.gz"
    hasher = hashlib.sha256()
    with tarfile.open(archive, "w:gz") as tar:
        for name in sorted(set(names) - {""}):
            if name not in {
                "Cargo.toml",
                "Cargo.lock",
                "justfile",
                "pixi.toml",
                "pixi.lock",
                "rust-toolchain",
                "rust-toolchain.toml",
                "rustfmt.toml",
                ".rustfmt.toml",
                ".cargo/config.toml",
                "README.md",
                "AGENTS.md",
                "LICENSE",
            } and name.split("/")[0] not in {
                "crates",
                "plugins",
                "python",
                "vendor",
                "testdata",
                "tools",
            }:
                continue
            path = REPO / name
            if not path.is_file() and not path.is_symlink():
                continue  # Deleted tracked files are absent from the working tree.
            if path.is_symlink():
                raise RuntimeError(
                    f"Source symlink requires an explicit packaging rule: {name}"
                )
            data = path.read_bytes()
            mode = 0o755 if os.access(path, os.X_OK) else 0o644
            hasher.update(json.dumps([name, mode, len(data)]).encode() + b"\0" + data)
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            tar.addfile(info, io.BytesIO(data))
    source = destination / "source"
    source.mkdir()
    with tarfile.open(archive) as tar:
        tar.extractall(source, filter="data")
    return archive, source, hasher.hexdigest()


def switch_link(link, target):
    """Readers see either the previous complete release or the new complete one."""
    link.parent.mkdir(parents=True, exist_ok=True)
    temporary = link.with_name(link.name + f".new-{os.getpid()}")
    temporary.symlink_to(target)
    os.replace(temporary, link)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="build-host.example.invalid")
    parser.add_argument("--jobs", type=int, default=24)
    parser.add_argument(
        "--ribbon-screenshots", action="store_true",
        help="Run the ribbon/dialog screenshot batch (requested checks and release preparation)",
    )
    parser.add_argument(
        "--root", type=Path, default=Path.home() / ".cache/basecraft/builds"
    )
    args = parser.parse_args()
    if not 1 <= args.jobs <= 24:
        parser.error("--jobs must be between 1 and 24")
    if output("uname", "-m") != "x86_64":
        parser.error(
            "this runtime snapshot workflow currently supports x86_64 Arch hosts"
        )
    root = args.root.resolve()
    if root.is_relative_to(REPO):
        parser.error("build storage must be outside the source checkout")
    root.mkdir(parents=True, exist_ok=True)
    with (root / "publish.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError("Another test-build publication is running") from error
        build(args, root)


def build(args, root):
    stage = Path(tempfile.mkdtemp(prefix="candidate-", dir=root))
    print(f"Build logs and candidate: {stage}", flush=True)
    commit = output("git", "-C", str(REPO), "rev-parse", "HEAD")
    archive, source, source_hash = snapshot(stage)
    branch = output("git", "-C", str(REPO), "branch", "--show-current") or "detached"
    toolchains = root / "toolchains"
    toolchains.mkdir(exist_ok=True)
    runtime_archive, runtime_id, runtime_metadata = runtime(toolchains)

    def remote(*command, **kwargs):
        return run(["ssh", args.host, shlex.join(map(str, command))], **kwargs)

    remote_home = subprocess.check_output(
        ["ssh", args.host, "printf '%s' \"$HOME\""], text=True
    )
    remote_root = remote_home + "/.cache/basecraft-test-builds"
    work = remote_root + "/" + runtime_id
    remote_source = work + "/source"
    image = "basecraft-build:" + runtime_id
    remote(
        "mkdir", "-p", work + "/target", work + "/cargo", work + "/tmp", work + "/home"
    )
    present = (
        subprocess.run(
            ["ssh", args.host, shlex.join(["docker", "image", "inspect", image])],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    )
    if not present:
        run(
            [
                "rsync",
                "-a",
                "--",
                runtime_archive,
                f"{args.host}:{remote_root}/{runtime_archive.name}",
            ]
        )
        remote("docker", "import", remote_root + "/" + runtime_archive.name, image)
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    run(
        [
            "rsync",
            "-az",
            "--",
            str((cargo_home / "registry").resolve()) + "/",
            f"{args.host}:{work}/cargo/registry/",
        ]
    )
    run(
        [
            "rsync",
            "-a",
            "--",
            archive,
            f"{args.host}:{work}/source-{source_hash}.tar.gz",
        ]
    )
    # The remote lock covers source replacement, all compilation, tests, and the
    # immutable delivery copy. Parallel callers cannot mix source or artifacts.
    delivery = work + "/deliveries/" + source_hash + "-" + stage.name
    command = f"""set -eu
mkdir -p {shlex.quote(work + "/deliveries")}
mkdir -p {shlex.quote(remote_source)} {shlex.quote(work + "/incoming-" + source_hash)}
tar -xzf {shlex.quote(work + "/source-" + source_hash + ".tar.gz")} -C {shlex.quote(work + "/incoming-" + source_hash)}
# Preserve unchanged source mtimes; changed bytes receive a fresh mtime so
# Cargo cannot mistake a different snapshot for already-compiled input.
rsync -r --checksum --delete --perms --omit-dir-times {shlex.quote(work + "/incoming-" + source_hash + "/")} {shlex.quote(remote_source + "/")}
rm -rf -- {shlex.quote(work + "/incoming-" + source_hash)}
docker run --rm --network none --cpus {args.jobs} --user "$(id -u):$(id -g)" \\
  -v {shlex.quote(work + ":/work")} -w /work/source \\
  -e PATH=/opt/rust/bin:/usr/bin -e HOME=/work/home -e CARGO_HOME=/work/cargo \\
  -e CARGO_TARGET_DIR=/work/target -e TMPDIR=/work/tmp -e CARGO_INCREMENTAL=0 \\
  -e CARGO_NET_OFFLINE=true -e CARGO_BUILD_JOBS={args.jobs} \\
  {image} /bin/bash -c 'set -eu
    cargo build --locked --offline -p basecraft-gui -p basecraft-cli -p basecraft-distance -p basecraft-delimitation
    cargo test --locked --offline -p basecraft-docs -p basecraft-ops -p basecraft-viewmodel --lib --tests'
mkdir -p {shlex.quote(delivery)}
cp {" ".join(shlex.quote(work + "/target/debug/" + name) for name in BINARIES)} {shlex.quote(delivery + "/")}
cd {shlex.quote(delivery)}
sha256sum {" ".join(BINARIES)} > SHA256SUMS
"""
    with (stage / "remote-build.log").open("w") as log:
        remote(
            "flock",
            remote_root + "/compile.lock",
            "bash",
            "-c",
            command,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    bundle = stage / "bundle"
    (bundle / "bin").mkdir(parents=True)
    run(["rsync", "-az", "--", f"{args.host}:{delivery}/", str(bundle / "bin") + "/"])
    expected = {
        line.split()[1]: line.split()[0]
        for line in (bundle / "bin/SHA256SUMS").read_text().splitlines()
    }
    for name in BINARIES:
        if digest(bundle / "bin" / name) != expected.get(name):
            raise RuntimeError(f"Transfer checksum mismatch: {name}")
    shutil.copytree(source / "plugins", bundle / "plugins")
    shutil.copytree(source / "python", bundle / "python")
    shutil.copy2(source / "tools/basecraft-launch.py", bundle / "launcher.py")
    checks = stage / "checks"
    for backend in ("software", "opengl"):
        run(
            [
                "python3",
                source / "crates/basecraft-gui/tools/check_package.py",
                bundle / "bin/basecraft-gui",
                checks / backend,
                "--backend",
                backend,
            ]
        )
    if args.ribbon_screenshots:
        run(
            [
                "python3",
                source / "crates/basecraft-gui/tools/check_ribbon.py",
                bundle / "bin/basecraft-gui",
                checks / "ribbon",
            ]
        )
    else:
        print("Ribbon/dialog screenshot batch skipped; use --ribbon-screenshots for release preparation or requested checks", flush=True)
    # Remote CI already ran the QML suite on this exact source; release
    # preparation runs it again here.
    receipt = None if args.ribbon_screenshots else ci_receipt(commit)
    if receipt:
        print(f"QML tests skipped; remote CI passed them for this commit: {receipt}", flush=True)
    else:
        run(["just", "qml-test"], cwd=source)
    engines = engine_report(bundle)
    manifest = {
        "schema_version": 1,
        "source_sha256": source_hash,
        "branch": branch,
        "commit": commit,
        "built_on": args.host,
        "runtime": runtime_metadata,
        "runtime_id": runtime_id,
        "published_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "python_environment": str((REPO / ".pixi/envs/default/bin").resolve()),
        "binaries": expected,
        "validation": [
            "remote core tests",
            "local software startup",
            "local OpenGL startup",
            *(["local RHI canvas and ribbon checks"] if args.ribbon_screenshots else []),
            "QML tests in remote CI" if receipt else "local QML tests",
            "local delimitation engine check",
        ],
        "engines": engines,
        "logs": str(stage),
    }
    (bundle / "build.json").write_text(json.dumps(manifest, indent=2) + "\n")
    releases = root / "releases"
    releases.mkdir(exist_ok=True)
    release = releases / (source_hash[:20] + "-" + str(time.time_ns()))
    bundle.rename(release)
    previous = root / "current"
    if previous.is_symlink():
        switch_link(root / "previous", previous.resolve(strict=True))
    switch_link(
        root / "channels" / hashlib.sha256(branch.encode()).hexdigest()[:20], release
    )
    switch_link(root / "current", release)
    print(
        f"Published validated build {source_hash[:20]} from {args.host}: {release}",
        flush=True,
    )


# Engines a build must be able to run. ASAP is native to basecraft-delimitation;
# mPTP is reported but not yet provisioned.
REQUIRED_ENGINES = ()


def engine_report(bundle):
    """Ask the bundled adapters, in the launcher's environment, which engines run."""
    env = dict(os.environ)
    probe = (
        "import json, sys; sys.path.insert(0, sys.argv[1]);"
        "import mptp_adapter;"
        "print(json.dumps({'asap': None, 'mptp': mptp_adapter.engine_problem()}))"
    )
    python = REPO / ".pixi/envs/default/bin/python"
    output = subprocess.check_output(
        [str(python), "-c", probe, str(bundle / "plugins/delimitation")], env=env, text=True
    )
    report = json.loads(output)
    for problem in report.values():
        if problem:
            print(f"Delimitation engine unavailable: {problem}", flush=True)
    missing = [name for name in REQUIRED_ENGINES if report.get(name)]
    if missing:
        raise RuntimeError("Required delimitation engines are unavailable: " + ", ".join(missing))
    return report


if __name__ == "__main__":
    main()
