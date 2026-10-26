"""Independent sealed-snapshot proposal conformance fixture."""
import time

from basecraft.plugin import Plugin, feature

plugin = Plugin("org.example.registry-sequence", "1.0.0")


@plugin.context_operation("annotate")
def annotate(snapshot, parameters, _context):
    if parameters["slow"]:
        time.sleep(0.2)
    if len(snapshot.residues) != snapshot.length:
        raise ValueError("snapshot length does not match its sealed residues")
    if not 0 <= parameters["start"] < parameters["end"] <= snapshot.length:
        raise ValueError("annotation range is outside the frozen sequence")
    return {"data": None, "ops": [feature("misc_feature", parameters["start"], parameters["end"], note="Conformance fixture")]}


if __name__ == "__main__":
    plugin.run()
