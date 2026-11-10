"""Typed missing-document refusals at raw MCP and Python save boundaries."""
import pathlib
import sys

from basecraft.session import OperationError, Session

root = pathlib.Path(sys.argv[1])
source = root / "gene.fasta"
original = b">gene\nACGT\n"
source.write_bytes(original)


def assert_error(value, route, code):
    assert value["schema"] == 1 and value["state"] == "failed", value
    assert value["operation"] == route, value
    assert value["errors"][0]["operation"] == route, value
    assert value["errors"][0]["code"] == code, value


def python_error(action, route, code):
    try:
        action()
    except OperationError as error:
        assert error.code == code, error.outcome
        assert_error(error.outcome, route, code)
        return
    raise AssertionError(f"{route} unexpectedly accepted document 999")


with Session(root, writes=True) as session:
    opened = session.open("gene.fasta")
    doc = opened["document"]
    state = session.state(doc)

    # Inspect the actual MCP tool envelope, including isError.
    for route in ("save_check", "save"):
        reply = session._request(
            "tools/call", {"name": route, "arguments": {"document": 999}}
        )
        assert reply["isError"] is True, reply
        assert_error(reply["structuredContent"], route, "unknown_document")

    # Python's public methods carry through the same typed outcome.
    python_error(lambda: session.save_check(999), "save_check", "unknown_document")
    python_error(lambda: session.save(999), "save", "unknown_document")

    # Malformed IDs are still input errors at both raw routes.
    for route in ("save_check", "save"):
        for arguments in ({}, {"document": None}, {"document": "999"}, {"document": -1}):
            reply = session._request("tools/call", {"name": route, "arguments": arguments})
            assert reply["isError"] is True, reply
            assert_error(reply["structuredContent"], route, "invalid_arguments")

    assert source.read_bytes() == original
    assert session.state(doc) == state
    assert session.documents() == [opened]
    assert session.read(doc, 0, 4)["residues"] == "ACGT"

assert source.read_bytes() == original
