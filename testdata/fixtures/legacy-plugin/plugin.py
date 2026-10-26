"""Unmodified two-argument public SDK compatibility fixture."""
from basecraft.plugin import Plugin

plugin = Plugin("org.example.legacy", "1.0.0")


@plugin.operation("noop")
def noop(_snapshot, _parameters):
    return {"ops": [], "message": "legacy"}


if __name__ == "__main__":
    plugin.run()
