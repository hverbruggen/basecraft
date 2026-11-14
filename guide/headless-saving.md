# Headless save checks and one-attempt decisions

Ordinary CLI saves, Python `Session.save` and MCP `save` remain conservative.
When some analysis evidence cannot be inspected, they refuse to save and retain
the edits. A caller can explicitly acknowledge that uncertainty for one attempt.
Known affected analyses still need a separate policy: `cancel` (default),
`preserve_copy`, `break_evidence` or `remove_analysis`.

## Python and MCP

Use the same live headless Session for inspection and saving:

```python
from basecraft.session import Session, OperationError

with Session("project", writes=True) as session:
    document = session.open("gene.fasta")["document"]
    # Apply edits through session.apply(...).
    check = session.save_check(document)
    # Present check["evidence"]["issues"] and ["conflict"] for an explicit decision.
    # The following values must come from that decision, never an automatic retry.
    choice = {
        "token": check["token"],
        "known_policy": "preserve_copy",
        "acknowledge_incomplete": True,
    }
    try:
        result = session.save(document, decision=choice)
    except OperationError as error:
        print(error.code, error.outcome)
        # Required/stale errors include a fresh check. Ask again before retrying.
    else:
        for warning in result["warnings"]:
            print(warning)  # Saved. Some analysis evidence was not checked.
```

MCP exposes the same `save_check` and `save` tools. Both accept `document` and
optional project `path`; `save` additionally accepts the `decision` object shown
above. All three decision fields are required, with a boolean acknowledgement
and an exact policy name. Omitting `decision` retains the conservative behavior.
Check results contain an opaque token, document/session/revision/target context,
and `evidence` with known conflict, issues and inventory revision. Each issue has
`code`, `location`, `relevance` and a specific `reason`.

A token belongs to its live Session and document. A newer check supersedes it;
an attempted decision save spends the matching receipt even on failure. Session
closure discards it. A changed document, target or evidence requires a fresh
check and decision; never retry an old token automatically. No client token map
or persistent preference grants authority.

Refusals have `state: failed` and `errors: [{code, message}]`. Codes include
`evidence_check_incomplete`, `save_decision_stale`, `analysis_input_conflict`,
`permission_denied`, `io_error`, `format_error` and `invalid_arguments`.
Incomplete/stale Session refusals include `check`; known conflicts include
`conflict`. Python preserves all fields in `OperationError.outcome`. Successful
saves return the existing document state plus `warnings`, empty for a complete
check or containing `evidence_not_checked` with the unchecked issues. Present
these warnings to the person or agent requesting the save.

Read-only MCP sessions may inspect but cannot save. Paths stay inside the
configured project. Neither MCP nor Python can replace an existing other file,
even with acknowledgement. Changed-source checks, preservation, domain guards,
permissions and write errors remain authoritative.

## CLI

An explicit choice applies only to saves made by that invocation:

```sh
basecraft edit gene.fasta '{"op":"seq.replace","params":{"start":1,"end":2,"residues":"T"}}' \
  --acknowledge-incomplete-evidence --save-known-policy preserve_copy
```

The CLI checks and decides within its own Session. The known policy defaults
to `cancel`; acknowledgement alone does not choose preserve/break/remove.
These options apply to every save a command makes: direct edit, new-sequence,
import-support, Sanger reviews, metadata and submission editing, and `plugin`.
They do not waive checks inside recovery, extraction or registered operations.

`basecraft save-check gene.fasta [--output new.fasta]` prints an advisory JSON
check without saving. Its token expires when that process exits and cannot
authorize another command. Review the issue locations and reasons before an
explicit one-invocation decision.

Save refusals are JSON on stderr with a nonzero exit. Warned successful saves
emit `{"state":"saved","warnings":[...]}` on stderr; normal command stdout
contracts remain unchanged. A warning must remain visible even when stdout is
captured. This adapter stage does not add the GUI override dialog or recovery
consent workflow.
