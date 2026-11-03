"""Copy public package notices/recipes from the exact locked Rattler cache.

Run on the builder after solving/building. Large evidence stays outside Git.
"""
import argparse
import hashlib
import json
import shutil
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    lock = json.loads(Path(__file__).with_name("build-lock.json").read_text())
    records = {p["sha256"]: p for packages in lock["environments"].values() for p in packages}
    inventory = []
    for record in records.values():
        identity = f"{record['name']}-{record['version']}-{record['build']}"
        matches = []
        for candidate in args.cache.glob(f"{record['name']}-{record['version']}-*/info"):
            index = json.loads((candidate / "index.json").read_text())
            if all(index[k] == record[k] for k in ("name", "version", "build")):
                matches.append(candidate)
        assert len(matches) == 1, (identity, matches)
        destination = args.output / identity
        destination.mkdir()
        for name in ("licenses", "recipe", "about.json", "index.json"):
            source = matches[0] / name
            if source.is_dir():
                shutil.copytree(source, destination / name)
            elif source.is_file():
                shutil.copyfile(source, destination / name)
        inventory.append(record)
    (args.output / "packages.json").write_text(json.dumps(inventory, indent=2) + "\n")
    hashes = {str(p.relative_to(args.output)): hashlib.sha256(p.read_bytes()).hexdigest()
              for p in sorted(args.output.rglob("*")) if p.is_file()}
    (args.output / "hashes.json").write_text(json.dumps(hashes, indent=2) + "\n")
    print(f"Retained notices and source recipes for {len(records)} exact dependency artifacts")


if __name__ == "__main__":
    main()
