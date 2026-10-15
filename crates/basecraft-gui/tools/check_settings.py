#!/usr/bin/env python3
"""Exercise GUI preference persistence in a fresh profile, without GPU access."""

import argparse
import json
import os
import subprocess
import tempfile
import tomllib
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    output = args.output.resolve()
    if output.is_relative_to(repo):
        parser.error("output must be outside the source tree")
    output.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="settings-", dir=output))
    source = root / "input.afa"
    source.write_text(">one\nACGT\n>two\nACTT\n")
    env = dict(
        os.environ,
        QT_QPA_PLATFORM="offscreen",
        QT_QPA_PLATFORMTHEME="",
        QT_QUICK_CONTROLS_STYLE="Basic",
        QT_QUICK_BACKEND="software",
        QT_FORCE_STDERR_LOGGING="1",
        QT_LOGGING_RULES="qml.debug=true",
        BASECRAFT_PROFILE_DIR=str(root / "profile"),
        BASECRAFT_CACHE_DIR=str(root / "cache"),
        XDG_CONFIG_HOME=str(root / "legacy"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_CACHE_HOME=str(root / "cache"),
    )
    prefs = root / "profile/config/preferences.toml"

    def run(case, extra):
        result = subprocess.run(
            [
                str(args.executable.resolve()),
                str(source),
                "--screenshot",
                str(root / f"{case}.png"),
                *extra,
            ],
            capture_output=True,
            text=True,
            env=env,
            timeout=20,
            check=False,
        )
        log = result.stdout + result.stderr
        (root / f"{case}.log").write_text(log)
        assert result.returncode == 0, log
        # Software mode is intentional: this checks settings, not RHI canvases.
        assert "ReferenceError" not in log and "TypeError" not in log, log
        return log

    run("initial-read", [])
    assert not prefs.exists(), "reading defaults must not create preferences"
    run("save", ["--cpu-threads", "2"])
    assert tomllib.loads(prefs.read_text())["compute"]["local_cpu_threads"] == 2
    before = prefs.read_bytes()
    assert "local CPU threads 2" in run("reload", [])
    assert prefs.read_bytes() == before, "startup must not rewrite preferences"
    assert "interface font points 11" in run("font-save", ["--font-points", "11"])
    assert tomllib.loads(prefs.read_text())["ui"]["font_size_points"] == 11
    log = run("font-reload", [])
    font_audit = json.loads(next(line.split("PREFERENCES_AUDIT ", 1)[1] for line in log.splitlines() if "PREFERENCES_AUDIT " in line))
    assert font_audit["fontPoints"] == 11, font_audit
    run("font-reset", ["--font-points", "null"])
    assert "font_size_points" not in tomllib.loads(prefs.read_text())["ui"]
    log = run("font-default", [])
    font_audit = json.loads(next(line.split("PREFERENCES_AUDIT ", 1)[1] for line in log.splitlines() if "PREFERENCES_AUDIT " in line))
    assert font_audit["fontPoints"] == font_audit["defaultFontPoints"], font_audit
    prefs.write_text("schema_version = 999\n")
    log = run("future-version", ["--cpu-threads", "3"])
    assert prefs.read_text() == "schema_version = 999\n"
    audit = json.loads(next(line.split("PREFERENCES_AUDIT ", 1)[1] for line in log.splitlines() if "PREFERENCES_AUDIT " in line))
    assert audit["error"] and not audit["modal"] and not audit["ready"], audit

    # Exercise real QSettings INI conversion, not just JSON fixtures. Every
    # application location stays isolated even though profile mode is disabled.
    env.pop("BASECRAFT_PROFILE_DIR")
    legacy = root / "legacy/Basecraft/Basecraft.conf"
    legacy.parent.mkdir(parents=True, exist_ok=True)
    legacy.write_text("[ui]\nalignmentInsert=false\nshowGc=true\nsidebarOpen=false\norfMinCodons=234\nlocalCpuThreads=2\n")
    before = legacy.read_bytes()
    log = run("legacy-ini", [])
    migrated = tomllib.loads((root / "legacy/basecraft/preferences.toml").read_text())
    assert migrated["ui"]["alignment_insert"] is False
    assert migrated["ui"]["sidebar_open"] is False
    assert migrated["ui"]["orf_min_codons"] == 234
    assert migrated["compute"]["local_cpu_threads"] == 2
    assert migrated["migration"]["qsettings_imported"] is True
    assert legacy.read_bytes() == before
    audit = json.loads(next(line.split("PREFERENCES_AUDIT ", 1)[1] for line in log.splitlines() if "PREFERENCES_AUDIT " in line))
    assert not audit["error"] and not audit["modal"] and audit["ready"], audit
    print(
        f"GUI settings persistence and future-version preservation passed; artifacts: {root}"
    )


if __name__ == "__main__":
    main()
