"""Write deterministic native PL fixtures outside the synced checkout.

Example: python3 generate_fixture.py --tips 64 --kind nonclock --out /media/.../SCI-0011/medium
"""

import argparse
import json
import math
from pathlib import Path


def make(tip_count, kind):
    if tip_count < 4 or tip_count > 2048 or tip_count & (tip_count - 1):
        raise ValueError("tips must be a power of two between 4 and 2048")
    if kind not in ("clock", "nonclock"):
        raise ValueError("kind must be clock or nonclock")
    nodes = []
    ages = []
    def build(first, count, parent, age):
        index = len(nodes)
        nodes.append(None)
        ages.append(age)
        children = []
        if count > 1:
            next_age = age / 2 if count > 2 else 0.0
            children = [build(first, count // 2, index, next_age),
                        build(first + count // 2, count // 2, index, next_age)]
        rate = 0.1 if kind == "clock" else 0.1 * (1 + 0.35 * math.sin(index * 1.7))
        length = None if parent is None else rate * (ages[parent] - age)
        nodes[index] = {"parent": parent, "children": children,
                        "label": f"t{first}" if count == 1 else "", "length": length}
        return index
    build(1, tip_count, None, 1.0)
    def newick(index):
        node = nodes[index]
        part = "(" + ",".join(newick(child) for child in node["children"]) + ")" if node["children"] else node["label"]
        return part + (":" + format(node["length"], ".17g") if node["length"] is not None else "")
    return nodes, newick(0) + ";\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--tips", type=int, required=True)
    parser.add_argument("--kind", choices=("clock", "nonclock"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    nodes, tree = make(args.tips, args.kind)
    args.out.mkdir(parents=True, exist_ok=False)
    (args.out / "tree.nwk").write_text(tree)
    request = {"nodes": nodes, "cancel_file": None,
               "parameters": {"branch_units": "substitutions_per_site", "sites": 953,
                              "root_age": 1.0, "time_unit": "relative units", "penalty": "log",
                              "lambda": 1.0, "starts": 3, "seed": 17, "constraints": [],
                              "cross_validation": None, "max_iterations": 2000, "max_seconds": 600}}
    (args.out / "request.json").write_text(json.dumps(request, indent=2) + "\n")


if __name__ == "__main__":
    main()
