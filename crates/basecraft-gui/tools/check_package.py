#!/usr/bin/env python3
"""Smoke-test a GUI executable outside its source tree with isolated user settings.

This checks local runtime deployment, not portability to another operating system.
Use the final installed/bundled executable to exercise its actual dependency paths.
"""

import argparse
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--backend", choices=["software", "opengl"], default="software")
    args = parser.parse_args()
    executable = args.executable.resolve(strict=True)
    output = args.output.resolve()
    repo = Path(__file__).resolve().parents[3]
    if output.is_relative_to(repo):
        parser.error("output must be outside the source tree")
    output.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="package-", dir=output))
    home = root / "home"
    home.mkdir()
    source = root / "specimen 100% # é 藻.afa"
    source.write_text(">one\nACGT\n>two\nACTT\n")
    screenshot = root / "startup.png"
    env = dict(os.environ)
    # Do not let developer module paths or control-style overrides mask deployment bugs.
    for key in ["QML_IMPORT_PATH", "QML2_IMPORT_PATH", "QT_PLUGIN_PATH", "QT_QUICK_CONTROLS_STYLE", "QT_QUICK_CONTROLS_CONF"]:
        env.pop(key, None)
    env.update(
        XDG_CONFIG_HOME=str(home / "config"),
        XDG_DATA_HOME=str(home / "data"),
        XDG_STATE_HOME=str(home / "state"),
        XDG_CACHE_HOME=str(home / "cache"),
        BASECRAFT_PROFILE_DIR=str(root / "profile"),
        BASECRAFT_CACHE_DIR=str(root / "cache"),
        QT_QPA_PLATFORM="offscreen",
        QT_QPA_PLATFORMTHEME="",
        QT_FORCE_STDERR_LOGGING="1",
        QT_DEBUG_PLUGINS="1",
    )
    if args.backend == "software":
        env["QT_QUICK_BACKEND"] = "software"
        env.pop("QSG_RHI_BACKEND", None)
    else:
        env["QT_QUICK_BACKEND"] = "rhi"
        env["QSG_RHI_BACKEND"] = "opengl"
    log_path = root / "startup.log"
    with log_path.open("w") as stream:
        try:
            result = subprocess.run(
                [str(executable), str(source), "--screenshot", str(screenshot), "--fit", "1"],
                cwd=root, env=env, stdout=stream, stderr=subprocess.STDOUT, text=True, timeout=45,
            )
        except subprocess.TimeoutExpired as error:
            raise RuntimeError(f"GUI startup timed out; see {log_path}") from error
    log = log_path.read_text()
    failures = re.findall(r"^.*(?:is not installed|is not a type|Cannot load library|Failed to load|Cannot open:|Error decoding|ReferenceError:|TypeError:|No such file or directory|Could not create scene graph context|could not load its interface).*$", log, re.MULTILINE)
    assert result.returncode == 0, f"GUI exited {result.returncode}; see {root / 'startup.log'}"
    assert not failures, f"Runtime dependency errors: {failures}; see {root / 'startup.log'}"
    image = screenshot.read_bytes()
    assert image[:8] == b"\x89PNG\r\n\x1a\n", "screenshot is not a PNG"
    width, height = struct.unpack(">II", image[16:24])
    assert width >= 320 and height >= 240, f"unexpected screenshot size {width}x{height}"
    assert not (root / ".basecraft").exists(), "passive viewing created source-side records"
    svg_loaded = bool(re.search(r"(?:loaded library.*svg|svg[^\n]*loaded library)", log, re.IGNORECASE))
    assert svg_loaded, f"SVG image plugin was not loaded; see {root / 'startup.log'}"
    report = {
        "executable": str(executable), "backend": args.backend,
        "platform": os.sys.platform, "screenshot_pixels": [width, height],
        "svg_plugin_loaded": svg_loaded,
        "limitations": ["Uses host system Qt/Kirigami runtime unless the executable supplies a bundle.",
                         "Software mode checks UI startup/resources only; it does not validate scientific canvas rendering.",
                         "Other operating systems and display servers are not exercised."],
    }
    (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Package runtime smoke passed ({args.backend}); artifacts: {root}")


if __name__ == "__main__":
    main()
