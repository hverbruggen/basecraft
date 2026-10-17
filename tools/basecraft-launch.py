#!/usr/bin/env python3
"""Launch a published, validated Basecraft build. Never invokes a compiler."""

import hashlib
import json
import os
import sys
from pathlib import Path


def main():
    args = sys.argv[1:]
    root = Path(
        os.environ.get("BASECRAFT_BUILDS", Path.home() / ".cache/basecraft/builds")
    )
    branch = None
    mode = "gui"
    if args and args[0] in ("--help", "-h"):
        print(
            "Usage: basecraft [--branch BRANCH] [--] [GUI arguments]\n"
            "       basecraft [--branch BRANCH] --cli [CLI arguments]\n"
            "       basecraft [--branch BRANCH] --build-info\n\n"
            "Launch the latest published test build, without compiling.\n"
            "Build and validate a new version with: just test-build"
        )
        return
    if args and args[0] == "--branch":
        if len(args) < 2 or not args[1]:
            raise ValueError("--branch requires a name")
        branch = args[1]
        args = args[2:]
    elif args and not args[0].startswith("-") and not Path(args[0]).exists():
        # Preserve the earlier launcher's positional branch shorthand.
        branch = args.pop(0)
    if args and args[0] in ("--cli", "--build-info"):
        mode = args.pop(0)[2:]
    if args and args[0] == "--":
        args.pop(0)
    link = root / (
        "channels/" + hashlib.sha256(branch.encode()).hexdigest()[:20]
        if branch
        else "current"
    )
    try:
        bundle = link.resolve(strict=True)
        manifest = json.loads((bundle / "build.json").read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError(
            "No published test build is available. Run 'just test-build' in the Basecraft checkout. "
            f"Details: {error}"
        ) from error
    if mode == "build-info":
        print(json.dumps(manifest, indent=2))
        return
    env = dict(os.environ)
    env.setdefault("BASECRAFT_PLUGINS", str(bundle / "plugins"))
    env.setdefault("BASECRAFT_PYTHONPATH", str(bundle / "python/src"))
    env.setdefault("BASECRAFT_DISTANCE", str(bundle / "bin/basecraft-distance"))
    env.setdefault("BASECRAFT_DELIMITATION", str(bundle / "bin/basecraft-delimitation"))
    environment = Path(manifest["python_environment"])
    if (environment / "python").is_file():
        env.setdefault("BASECRAFT_PYTHON", str(environment / "python"))
        env["PATH"] = str(environment) + os.pathsep + env.get("PATH", "")
    tracy = Path.home() / ".cache/basecraft/tools/tracy"
    if tracy.is_file():
        env.setdefault("BASECRAFT_TRACY", str(tracy))
    # Built separately by tools/install-asap.py; ASAP is not on PyPI or conda.
    asap = Path.home() / ".cache/basecraft/tools/asap/bin/python"
    if asap.is_file():
        env.setdefault("BASECRAFT_ASAP_PYTHON", str(asap))
    executable = bundle / "bin" / ("basecraft" if mode == "cli" else "basecraft-gui")
    # Keep the caller's working directory so relative input paths remain correct.
    os.execve(executable, [str(executable), *args], env)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        print(f"basecraft: {error}", file=sys.stderr)
        sys.exit(1)
