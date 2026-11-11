"""Extract taxa from a validated linked alignment without changing columns."""
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import read_alignment, write_atomic

plugin = Plugin("basecraft.clade-alignment", "0.1.0")


@plugin.operation("extract")
def extract(snapshot, params, context):
    source, target = Path(context.input("alignment")), Path(context.output("artifacts", "clade.afa"))
    names = params.get("taxa", [])
    if not names or not all(isinstance(n, str) for n in names) or len(set(names)) != len(names):
        raise ValueError("Select distinct taxa")
    records = read_alignment(source, minimum=1)
    available = {h.split()[0] for h, _ in records}
    missing = set(names) - available
    if missing:
        raise ValueError("Missing alignment rows: " + ", ".join(sorted(missing)))
    selected = set(names)
    write_atomic(target, "".join(f">{h}\n{s}\n" for h, s in records if h.split()[0] in selected))
    return {"ops": [],
            "message": f"Extracted {len(names)} aligned sequences; columns unchanged"}


if __name__ == "__main__":
    plugin.run()
