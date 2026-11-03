"""Audit actual Rattler build/host prefixes against the retained resolved lock.

This only reads package metadata; it never builds or changes environments.
"""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build_directory", type=Path,
                        help="Rattler build directory containing build_env and host_env_*")
    args = parser.parse_args()
    lock = json.loads(Path(__file__).with_name("build-lock.json").read_text())
    prefixes = {"build": args.build_directory / "build_env",
                "host": next(args.build_directory.glob("host_env*"))}
    for kind, prefix in prefixes.items():
        actual = {p["name"]: p for f in (prefix / "conda-meta").glob("*.json")
                  if (p := json.loads(f.read_text()))}
        expected = {p["name"]: p for p in lock["environments"][kind]}
        assert actual.keys() == expected.keys(), (kind, actual.keys() ^ expected.keys())
        for name, record in expected.items():
            for field in ("version", "build", "sha256", "url"):
                assert actual[name][field] == record[field], (kind, name, field)
        print(f"{kind}: verified {len(actual)} exact package identities and SHA256 records")


if __name__ == "__main__":
    main()
