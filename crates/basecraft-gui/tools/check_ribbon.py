#!/usr/bin/env python3
"""Render the real ribbon pages and check geometry using an isolated profile.

Requires an RHI-capable Qt platform; QT_QUICK_BACKEND=software cannot render
Basecraft's scientific canvases. Artifacts belong outside the source tree.
"""

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--trace-fixture", type=Path)
    args = parser.parse_args()
    if not shutil.which("magick"):
        parser.error("ImageMagick is required to validate rendered screenshots")
    repo = Path(__file__).resolve().parents[3]
    output = args.output.resolve()
    if output.is_relative_to(repo):
        parser.error("output must be outside the source tree")
    output.mkdir(parents=True, exist_ok=True)
    sequence = output / "sequence.fasta"
    sequence.write_text(">sample\n" + "ACGT" * 100 + "\n")
    alignment = output / "alignment.afa"
    alignment.write_text(">one\nACGTACGT\n>two\nACGTTCGT\n")
    tree = output / "tree.nwk"
    tree.write_text("(one:0.1,two:0.2);\n")
    partition = output / "partition.spart"
    shutil.copyfile(repo / "testdata/fixtures/delimitation/manual.spart", partition)
    pages = [
        (sequence, i, name)
        for i, name in enumerate(["File", "View", "Annotations", "Settings", "Help"])
    ]
    pages += [
        (alignment, 2, "Alignment"),
        (alignment, 3, "Build phylogeny"),
        (alignment, 3, "Distance matrix"),
        (alignment, 3, "Distance nj"),
        (alignment, 3, "Distance upgma"),
        (tree, 2, "Tree"),
        (partition, 2, "Delimitation"),
    ]
    if args.trace_fixture:
        trace = output / "trace.ab1"
        shutil.copyfile(args.trace_fixture, trace)
        pages.append((trace, 2, "Trace"))
    else:
        print("Trace page not exercised: supply --trace-fixture", flush=True)
    env = dict(
        os.environ,
        QT_QPA_PLATFORM="offscreen",
        QT_QPA_PLATFORMTHEME="",
        QT_QUICK_CONTROLS_STYLE="Basic",
        QT_LOGGING_RULES="qml.debug=true",
        QT_FORCE_STDERR_LOGGING="1",
        BASECRAFT_PROFILE_DIR=str(output / "profile"),
        XDG_CONFIG_HOME=str(output / "config"),
        XDG_CACHE_HOME=str(output / "cache"),
    )
    env["QT_QUICK_BACKEND"] = "rhi"
    failures = []
    for scale, width, font in [
        ("1", 1200, 11),
        ("1.25", 640, 18),
        ("1.5", 800, 14),
        ("2", 1200, 22),
    ]:
        for source, tab, name in pages:
            case = output / f"{name.replace(' ', '-')}-{scale}"
            env["QT_SCALE_FACTOR"] = scale
            result = subprocess.run(
                [
                    str(args.executable.resolve()),
                    str(source),
                    "--screenshot",
                    str(case) + ".png",
                    "--ribbon",
                    str(tab),
                    "--ribbon-audit",
                    "1",
                    "--window-width",
                    str(width),
                    "--font-size",
                    str(font),
                    *(
                        ["--distance-dialog", name.split()[-1]]
                        if name.startswith("Distance ")
                        else []
                    ),
                ],
                env=env,
                capture_output=True,
                text=True,
                timeout=40,
                check=False,
            )
            log = result.stdout + result.stderr
            Path(str(case) + ".log").write_text(log)
            try:
                assert result.returncode == 0, (name, scale, log)
                assert Path(str(case) + ".png").exists(), (
                    name,
                    scale,
                    "no rendered image",
                    log,
                )
                assert not any(
                    error in log
                    for error in [
                        "Binding loop",
                        "ReferenceError",
                        "TypeError",
                        "failed to load component",
                        "No QRhi found",
                        "Failed to create QRhi",
                    ]
                ), log
                records = [
                    json.loads(line.split("RIBBON_AUDIT ", 1)[1])
                    for line in log.splitlines()
                    if "RIBBON_AUDIT " in line
                ]
                colors = subprocess.run(
                    [
                        "magick",
                        str(case) + ".png",
                        "-gravity",
                        "northwest",
                        "-crop",
                        "25%x10%+0+0",
                        "+repage",
                        "-format",
                        "%k",
                        "info:",
                    ],
                    capture_output=True,
                    text=True,
                    check=True,
                    timeout=10,
                ).stdout
                assert int(colors) > 1, "Screenshot header is blank/cropped"
                opaque = subprocess.run(
                    ["magick", str(case) + ".png", "-format", "%[opaque]", "info:"],
                    capture_output=True,
                    text=True,
                    check=True,
                    timeout=10,
                ).stdout
                assert opaque.lower() == "true", "Screenshot lost the window background"
                assert len(records) == 1, log
                assert "saved true" in log, log
                record = records[0]
                expected = "Build phylogeny" if name.startswith("Distance ") else name
                assert record["page"] == expected, record
                assert record["groups"], record
                caption = record["groups"][0]["caption"]
                center = record["groups"][0]["center"]
                for group in record["groups"]:
                    assert abs(group["caption"] - caption) <= 1, record
                    assert abs(group["center"] - center) <= 1, record
                    assert group["height"] + 1 >= group["implicitHeight"], record
                    assert group["width"] + 1 >= group["implicitWidth"], record
                    assert group["y"] + group["height"] <= record["height"] + 1, record
                print(
                    f"{name} scale={scale} width={width} font={font}: passed",
                    flush=True,
                )
            except AssertionError as error:
                failures.append(f"{name} scale={scale}: {error}")
                print(f"{name} scale={scale}: FAILED (see {case}.log)", flush=True)
    assert not failures, "\n".join(failures)


if __name__ == "__main__":
    main()
