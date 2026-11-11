# Frozen Sanger writer fixtures

Captured with production source at `4044160b17a8de501fe7518faa3e5d67ca03dd8c`,
before extracting the five protocol payloads. The only capture additions were
the test module and its `cfg(test)` declaration in storage. Production writer,
reader and review methods were unchanged. This is synthetic compatibility data,
not biological accuracy evidence.

`single.json` is actual v1 `save` output. `multi.json` is actual v2 `save` output
with B selected and A inactive. A contains every ReviewOp variant, both Evaluate
versions, all choice variants, manual decisions, mask, undo/redo and historical
Export. B has independent manual/undo history. Its first-event Evaluate is not
undoable; A's first Evaluate follows AutoAccept and is undoable. The original
pretty draft JSON, trailing newline and unknown draft extension are retained
verbatim. `sha256.json` freezes every captured file included here.

The `.basecraft/analyses-v1` runs retain all source/evidence dependencies and
published support. The synthetic AB1 comes from the existing compact fixture.
The checkpoint and its manifest retain their original job ID, owner/time and
synthetic `/fixture` paths. `capture-index.json` identifies it. No captured bytes
were edited or normalized. File stamps are historical evidence, not assertions
about the inode/time of a checkout copy. Runtime logs, machine/toolchain receipts,
profile registry, lock and result-discovery bindings are kept outside Git.

## Capture recipe and comparison scope

Use an isolated empty directory mounted as `/fixture`, with application profile
`/fixture/profile`, and the pinned production source plus
`crates/basecraft-ops/src/sanger_review/legacy_fixtures.rs` and its test-module
registration. Set `BASECRAFT_CAPTURE=legacy-writer`. Run only the ignored
`capture_legacy_writer` unit test in the pre-migration source. That writer has
been retired; use the pinned historical source for an intentional recapture. It copies the small synthetic trace, creates
two drafts, executes the real review methods, exports, saves v1, selects B,
saves v2 and records the actual checkpoint location. Compiler output and TMP
must be outside source. The capture guard refuses an existing `single.json`.

Copy capture files unchanged, excluding `profile/state/analysis-stores.json`,
`profile/state/analysis-evidence.lock`, and `.basecraft/analyses-v1/bindings/`.
Generate the SHA-256 index over included captured files (excluding this README
and the index itself). Regeneration allocates fresh IDs/times/stamps and is not
a byte oracle for these fixtures; normal tests never regenerate them.

Parity tests load the frozen evidence and call the existing serializer at the
same logical destination, keeping original run IDs. Durable-save bytes have no
trailing newline; checkpoint `write_json` bytes have one. Tests also recognize the
checkpoint in an isolated child with its captured profile layout, replay both
histories, verify selected/manual state and check source/support digests. The
historical Export's synthetic absolute path need not exist at test time.

Discovery bindings are deliberately excluded: the existing reader adds a bound
result run to every loaded set, including inactive sets. Thus normal bound
load/save is not asserted byte-identical. The complete raw capture, including
bindings, is retained in the private runtime evidence. No reader/writer behavior
was changed to make the serializer parity test pass. The test does not re-run
triage, reallocate runs, refresh file stamps or rewrite original drafts.
