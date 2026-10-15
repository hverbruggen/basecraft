#!/usr/bin/env python3
"""Exercise keyboard regions, text inspection and real canvas rendering."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def records(log, prefix):
    return [json.loads(line.split(prefix + " ", 1)[1]) for line in log.splitlines() if prefix + " " in line]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--trace-fixture", type=Path)
    parser.add_argument("--software", action="store_true")
    args = parser.parse_args()
    root = args.output.resolve()
    repo = Path(__file__).resolve().parents[3]
    if root.is_relative_to(repo):
        parser.error("output must be outside the repository")
    root.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="accessibility-", dir=root))
    fixtures = {"sequence.fasta": ">sample\n" + "ACGT" * 500 + "\n",
                "alignment.afa": ">one\nACGTACGT\n>two\nACGTTCGT\n",
                "tree.nwk": "(one:0.1,two:0.2);\n"}
    for name, content in fixtures.items():
        (root / name).write_text(content)
    if args.trace_fixture:
        shutil.copyfile(args.trace_fixture, root / "trace.ab1")
        fixtures["trace.ab1"] = ""
    failures = []
    for scale in ["1", "1.25", "2"]:
        for name in fixtures:
            image = root / f"{name}-{scale}.png"
            env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QPA_PLATFORMTHEME="",
                       QT_QUICK_BACKEND="software" if args.software else "rhi",
                       QT_SCALE_FACTOR=scale, QT_FORCE_STDERR_LOGGING="1", BASECRAFT_RENDER_DIAGNOSTICS="1", QSG_INFO="1",
                       QT_LOGGING_RULES="qml.debug=true", BASECRAFT_PROFILE_DIR=str(root / "profile"))
            for kind in ["CONFIG", "DATA", "STATE", "CACHE"]:
                env[f"XDG_{kind}_HOME"] = str(root / kind.lower())
            try:
                result = subprocess.run([str(args.executable.resolve()), str(root / name),
                                         "--screenshot", str(image), "--accessibility-audit", "1",
                                         "--render-audit", "1", "--fit", "1",
                                         "--window-capture", str(image.with_name(image.stem + "-window.png"))],
                                        env=env, capture_output=True, text=True, timeout=30)
            except subprocess.TimeoutExpired as error:
                output = (error.stdout or b"") + (error.stderr or b"")
                image.with_suffix(".log").write_bytes(output)
                raise
            log = result.stdout + result.stderr
            image.with_suffix(".log").write_text(log)
            assert result.returncode == 0, log
            assert not any(s in log for s in ["Binding loop", "ReferenceError", "TypeError", "failed to load component"]), log
            focus = records(log, "ACCESSIBILITY_AUDIT")
            assert len(focus) == 1 and all(focus[0].values()), (name, scale, focus, log)
            render = records(log, "RENDER_AUDIT")
            assert len(render) == 1 and not render[0]["error"], render
            assert render[0]["software"] == args.software, render
            assert image.is_file(), log
            if not args.software:
                assert "BASECRAFT_RENDER instancing true" in log, log
                # Inspect the actual canvas, excluding its focus border and surrounding UI.
                data = render[0]
                dimensions = subprocess.check_output(["magick", str(image), "-format", "%w %h", "info:"], text=True).split()
                ratio = int(dimensions[0]) / data["windowWidth"]
                x, y = int((data["x"] + 4) * ratio), int((data["y"] + 4) * ratio)
                width = int((min(data["width"], data["windowWidth"] - data["x"]) - 8) * ratio)
                height = int((min(data["height"], data["windowHeight"] - data["y"]) - 8) * ratio)
                colors = subprocess.check_output(["magick", str(image), "-crop", f"{width}x{height}+{x}+{y}", "+repage", "-format", "%k", "info:"], text=True)
                if int(colors) <= 16:
                    failures.append(f"Canvas appears blank: {image} ({colors} colors)")
                    print(failures[-1], flush=True)
                    continue
            print(f"{name} scale={scale}: keyboard and {'fallback' if args.software else 'canvas'} passed", flush=True)
    print(f"Artifacts: {root}")
    assert not failures, "\n".join(failures)


if __name__ == "__main__":
    main()
