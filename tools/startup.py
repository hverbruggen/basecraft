"""Real offscreen application startup and same-process idle memory measurements."""
import json
import os
from pathlib import Path
import selectors
import statistics
import subprocess
import time


def startup_samples(executable, destination, backend, previous=None, *, count=5,
                    cold=False, validation=True):
    destination.mkdir(parents=True, exist_ok=True)
    samples = []
    prefix = b"basecraft-gui-startup-v1 "
    for index in range(count):
        profile = destination / f"profile-{backend}-{index}"
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QPA_PLATFORMTHEME="",
                   QT_QUICK_BACKEND="software" if backend == "software" else "rhi",
                   BASECRAFT_PROFILE_DIR=str(profile), BASECRAFT_STATE_DIR=str(profile / "state"),
                   BASECRAFT_CACHE_DIR=str(profile / "cache"), XDG_CACHE_HOME=str(profile / "xdg-cache"),
                   XDG_CONFIG_HOME=str(profile / "xdg-config"))
        env.pop("BASECRAFT_STARTUP_TRACE_TOKEN", None)
        env.pop("BASECRAFT_GUI_TEST", None)
        if validation:
            env["BASECRAFT_GUI_TEST"] = "1"
        if backend == "opengl":
            env["QSG_RHI_BACKEND"] = "opengl"
        else:
            env.pop("QSG_RHI_BACKEND", None)
        if cold:
            with open(executable, "rb") as source:
                os.fsync(source.fileno())
                os.posix_fadvise(source.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
        log = destination / f"startup-{backend}-{index}.log"
        with log.open("wb") as output_log, selectors.DefaultSelector() as selector:
            started = time.monotonic_ns()
            process = subprocess.Popen([str(executable), "--startup-trace"], env=env,
                                       stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT)
            try:
                selector.register(process.stdout, selectors.EVENT_READ)
                events, pending, observed = {}, b"", None
                while time.monotonic_ns() - started < 30_000_000_000:
                    for key, _ in selector.select(timeout=0.1):
                        chunk = os.read(key.fd, 65536)
                        received = time.monotonic_ns()
                        output_log.write(chunk)
                        output_log.flush()
                        pending += chunk
                        lines = pending.split(b"\n")
                        pending = lines.pop()
                        for line in lines:
                            if line.startswith(prefix):
                                row = json.loads(line[len(prefix):])
                                if row["pid"] == process.pid:
                                    events[row["event"]] = row["elapsed_ns"] / 1e6
                                    if row["event"] == "first_frame":
                                        observed = (received - started) / 1e6
                    if observed is not None and "qml_load_returned" in events:
                        break
                    if process.poll() is not None:
                        raise RuntimeError(f"startup exited before first frame: {log}")
                else:
                    raise RuntimeError(f"no first frame within 30 seconds: {log}")
                time.sleep(1)
                rss = []
                for _ in range(3):
                    if process.poll() is not None:
                        raise RuntimeError(f"application exited before idle measurement: {log}")
                    status = Path(f"/proc/{process.pid}/status")
                    rss.append(next(int(line.split()[1]) * 1024 for line in status.read_text().splitlines()
                                    if line.startswith("VmRSS:")))
                    time.sleep(0.1)
                samples.append(dict(spawn_to_first_frame_ms=observed,
                                    qml_loaded_ms=events["qml_load_returned"],
                                    first_frame_ms=events["first_frame"], idle_rss_bytes=statistics.median(rss)))
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                process.stdout.close()
    medians = {key: statistics.median(sample[key] for sample in samples) for key in samples[0]}
    result = dict(samples=samples, medians=medians, backend=backend,
                  clock="spawn to trace receipt includes loader and pipe delivery; trace times start at Rust main",
                  cache="fresh profile; executable DONTNEED; shared libraries uncontrolled" if cold else "fresh profile; OS cache uncontrolled",
                  idle="median of three RSS readings 1.0–1.2 s after first frame")
    if previous and previous.get("medians"):
        result["previous_ratios"] = {key: value / previous["medians"][key] for key, value in medians.items()
                                     if previous["medians"].get(key, 0) > 0}
    return result
