# Sanger review

Opening an AB1/ABI immediately shows an instrument-call review. Alternative Tracy
calls run in the background through the public
`basecraft.tracy-sanger/consensus@1` operation, also available through the
CLI, Python API and MCP. Cancellation or caller/storage failure leaves instrument
review usable. AB1 bytes are never changed.

Use the **Calls** selector in the Trace workflow to choose a retained source set.
The signal lane displays its original calls; the consensus row displays its
reviewed calls. Each set retains its own edits, undo/redo, triage and masks.
Computation never switches sets or transfers decisions by column number.
Previous computed sets remain available when settings or caller identity change.
Alternative calls are not guaranteed to be more accurate.

Save the review to retain manual choices. Multi-set reviews use envelope version
2; old v1-only applications reject them. Existing v1 reviews and multi-read drafts
remain readable. File Open and the sidebar recognise validated draft/review JSON;
unknown JSON is not assumed to be Sanger evidence.

Completed public-operation receipts supply the cache. Reuse requires matching
source SHA-256, plugin package/contract, caller executable SHA-256 and effective
settings, and verified retained output hashes. Caller version output is retained
in the immutable draft. Execution checks the input and executable before and
after calling. Cancelled, failed, partial or stale runs cannot supply cached calls.
Review and supporting metadata use the existing atomic writers, input ledgers and
analysis store. Source relocation and packages preserve every retained set.

Call-set selection and rendering are GUI presentation over shared Rust review
operations. Computation uses the existing public operation contract; no new
numerical caller or GUI-only analysis algorithm is introduced. The existing
multi-read consensus and MAFFT placement pilot still run synchronously.

Normalized call evidence retains `sequence`, `secondary_sequence`, `positions`
and `scores` in the original read's event order. Positions are zero-based signal
samples. The optional secondary string has exactly one IUPAC symbol per primary
event; it is Tracy evidence, not an accepted call or calibrated mixture label.
Missing/null secondary evidence in an artifact means unknown, not agreement
with primary. Trimming and draft reverse complementation leave all native arrays
intact; use the read orientation and existing event mapping to interpret them.
The same JSON evidence is returned by the public calls/consensus plugin paths and
retained in shared draft/review serialization. Review decisions are unchanged.
