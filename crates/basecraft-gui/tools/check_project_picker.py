#!/usr/bin/env python3
"""Exercise the real project command/picker in isolated GUI profiles."""

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path
from urllib.parse import unquote, urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--native", action="store_true", help="use the desktop platform/theme"
    )
    args = parser.parse_args()
    output = args.output.resolve()
    repo = Path(__file__).resolve().parents[3]
    if output.is_relative_to(repo):
        parser.error("output must be outside the source tree")
    output.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="project-picker-", dir=output))
    home = Path.home().resolve()
    first = root / "projects/étude one #1"
    second = root / "projects/étude two %2"
    unrelated = root / "browse only"
    for folder in (first, second, unrelated):
        folder.mkdir(parents=True)
    missing = root / "unavailable"
    document = root / "document.afa"
    document.write_text(">one\nACGT\n>two\nACTT\n")

    def run(case, history=None, steps=None, damaged_preferences=False, source=None):
        profile = root / case
        history_file = profile / "state/recent-projects.json"
        if history is not None:
            history_file.parent.mkdir(parents=True)
            history_file.write_text(
                history
                if isinstance(history, str)
                else json.dumps(
                    {"version": 1, "projects": [str(path) for path in history]}
                )
            )
        if damaged_preferences:
            prefs = profile / "config/preferences.toml"
            prefs.parent.mkdir(parents=True)
            prefs.write_text("schema_version = 999\n")
        env = dict(
            os.environ,
            BASECRAFT_PROFILE_DIR=str(profile),
            QT_QUICK_BACKEND="software",
            QT_FORCE_STDERR_LOGGING="1",
        )
        if not args.native:
            env.update(
                QT_QPA_PLATFORM="offscreen",
                QT_QPA_PLATFORMTHEME="",
                QT_QUICK_CONTROLS_STYLE="Basic",
            )
        if steps is None:
            steps = [
                {"action": "open"},
                {"action": "reject"},
                {"action": "open"},
                {"action": "reject"},
            ]
        command = [str(args.executable.resolve())]
        if source:
            command.append(str(source))
        command.extend(
            [
                "--screenshot",
                str(root / f"{case}.png"),
                "--project-picker",
                json.dumps(steps),
            ]
        )
        try:
            result = subprocess.run(
                command,
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            log = (error.stdout or b"") + (error.stderr or b"")
            (root / f"{case}.log").write_bytes(log)
            raise RuntimeError(f"GUI timed out; see {root / f'{case}.log'}") from error
        log = result.stdout + result.stderr
        (root / f"{case}.log").write_text(log)
        assert result.returncode == 0, log
        assert "ReferenceError" not in log and "TypeError" not in log, log
        audits = [
            json.loads(line.split("PROJECT_PICKER_AUDIT ", 1)[1])
            for line in log.splitlines()
            if "PROJECT_PICKER_AUDIT " in line
        ]
        assert len(audits) == len(steps), log
        for audit in audits:
            url = urlparse(audit["folder"])
            assert url.scheme == "file", audit
            audit["folder"] = Path(unquote(url.path))
            if audit["action"] == "open":
                assert audit["visible"], audit
            if audit["action"] in ("reject", "accept"):
                assert not audit["visible"], audit
        return audits, history_file

    def check_initial(case, history, expected, **kwargs):
        audits, file = run(case, history, **kwargs)
        assert all(a["folder"] == expected for a in audits if a["action"] == "open"), (
            audits
        )
        return audits, file

    _, file = check_initial("empty", None, home)
    assert not file.exists(), "opening/cancelling must not create history"
    check_initial("common", [first, second, missing], first.parent)
    check_initial("single", [missing, first, document, Path("relative")], first)
    check_initial("missing", [missing, document, Path("relative")], home)
    check_initial("root", [first, Path(first.anchor)], home)
    audits, damaged = check_initial("damaged", "damaged", home)
    assert audits[0]["historyError"] and damaged.read_text() == "damaged"
    audits, _ = check_initial(
        "preferences-failure", [first, second], first.parent, damaged_preferences=True
    )
    assert audits[0]["preferencesError"] and not audits[0]["preferencesReady"], audits
    # An active document elsewhere must not change the project chooser policy.
    check_initial("active-document", [first, second], first.parent, source=document)

    steps = [
        {"action": "open"},
        {"action": "choose", "path": str(first)},
        {"action": "accept"},
        {"action": "open"},
        {"action": "browse", "path": str(unrelated)},
        {"action": "reject"},
        {"action": "select", "path": str(missing)},
        {"action": "open"},
        {"action": "choose", "path": str(second)},
        {"action": "accept"},
        {"action": "open"},
        {"action": "reject"},
    ]
    audits, file = run("session", [first, second], steps)
    opened = [a["folder"] for a in audits if a["action"] == "open"]
    assert opened == [first.parent, first, first, second], audits
    assert audits[6]["historyError"] and Path(audits[6]["project"]) == first, audits
    assert json.loads(file.read_text())["projects"] == [str(second), str(first)]
    if not args.native:
        # Native backends prevent selecting unavailable folders. The fallback
        # also lets us inject one to exercise the accepted-handler failure path.
        audits, _ = run(
            "failed-picker",
            [first],
            [
                {"action": "select", "path": str(first)},
                {"action": "open"},
                {"action": "choose", "path": str(missing)},
                {"action": "accept"},
                {"action": "open"},
                {"action": "reject"},
            ],
        )
        assert audits[3]["historyError"] and Path(audits[3]["project"]) == first, audits
        assert audits[4]["folder"] == first, audits
    # Successful project selection stays useful even if durable history is damaged.
    audits, damaged = run(
        "session-damaged",
        "damaged",
        [
            {"action": "select", "path": str(first)},
            {"action": "open"},
            {"action": "reject"},
            {"action": "select", "path": str(missing)},
            {"action": "open"},
            {"action": "reject"},
        ],
    )
    assert all(a["folder"] == first for a in audits if a["action"] == "open"), audits
    assert audits[0]["historyError"] and damaged.read_text() == "damaged"
    print(
        f"Project picker integration passed ({'native' if args.native else 'offscreen'}); artifacts: {root}"
    )


if __name__ == "__main__":
    main()
