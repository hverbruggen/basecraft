"""Exercise the bundled PTP engine through the public operation host."""
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


def check_ptp(bundle, output, python):
    output.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="ptp-", dir=output))
    project = root / "project"
    project.mkdir()
    tree = project / "input.nwk"
    original = b"((a:0.001,b:0.001):1.0,(c:0.001,d:0.001):1.0);\n"
    tree.write_bytes(original)
    env = dict(os.environ, BASECRAFT_PROFILE_DIR=str(root / "profile"),
               BASECRAFT_PLUGINS=str(bundle / "plugins"),
               BASECRAFT_PYTHONPATH=str(bundle / "python/src"),
               BASECRAFT_PYTHON=str(python),
               BASECRAFT_DELIMITATION=str(bundle / "bin/basecraft-delimitation"))
    # Prove bundle discovery, independent of a developer's configured engine.
    env.pop("BASECRAFT_MPTP", None)
    runs = []
    for method in ("ptp", "mptp"):
        request = project / f"{method}.json"
        request.write_text(json.dumps({
            "operation": f"basecraft.delimitation/{method}@1",
            "parameters": {"rooted": True, "min_branch": 0.0, "seed": 1},
            "inputs": {"tree": [{"kind": "file", "path": "input.nwk"}]},
            "destinations": {"artifacts": f"{method}-results"},
        }))
        command = [str(bundle / "bin/basecraft"), "operations", "run",
                   f"basecraft.delimitation/{method}@1", "--project", str(project),
                   "--request", str(request), "--json"]
        completed = subprocess.run(command, env=env, capture_output=True, text=True,
                                   timeout=60, check=True)
        (root / f"{method}-outcome.json").write_text(completed.stdout)
        (root / f"{method}-stderr.log").write_text(completed.stderr)
        outcome = json.loads(completed.stdout)
        assert outcome["state"] == "completed", outcome
        result_path = next(Path(a["path"]) for a in outcome["artifacts"]
                           if Path(a["path"]).name == "result.json")
        result = json.loads(result_path.read_text())
        hypothesis = result["workspace"]["hypotheses"][0]
        assert sorted(sorted(g["members"]) for g in hypothesis["groups"]) == [["a", "b"], ["c", "d"]]
        params = hypothesis["provenance"]["parameters"]
        assert params["lrt_passed"] is True, params
        assert "0.2.5" in params["mptp_version"], params
        assert Path(params["command"][0]).resolve() == (bundle / "bin/mptp").resolve()
        assert tree.read_bytes() == original
        verified = subprocess.run([str(bundle / "bin/basecraft"), "provenance", "verify",
                                   str(result_path)], env=env, capture_output=True,
                                  text=True, timeout=30, check=False)
        (root / f"{method}-verify.log").write_text(verified.stdout + verified.stderr)
        assert verified.returncode == 0, verified.stderr
        runs.append({"method": method, "run_id": outcome["run_id"],
                     "engine": params["mptp_version"], "groups": [["a", "b"], ["c", "d"]]})
    receipt = {"status": "passed", "runs": runs,
               "engine_sha256": hashlib.sha256((bundle / "bin/mptp").read_bytes()).hexdigest(),
               "source_unchanged": True}
    (root / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Real PTP/mPTP package checks passed: {root}", flush=True)
    return str(root / "receipt.json")
