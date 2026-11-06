# Local feedback builds

These scripts build and validate a local feedback installation. They do not
assume a particular SSH host, home directory or mounted drive.

Set build storage with `CARGO_TARGET_DIR` or Cargo's user-level `build.target-dir`.
Set compiler temporary storage with `TMPDIR`. Pixi's `detached-environments`
setting belongs in user-level configuration, not the repository. With standard
Pixi settings, plugins find the checkout's `.pixi/envs/default` interpreter.
For detached environments, either link that directory deliberately to the
matching environment or set `BASECRAFT_PYTHON` to its interpreter. `pixi run`
also supplies the environment on PATH. Explicit interpreter overrides and
installed bundle environments take precedence over source discovery.

For remote feedback packaging, supply the SSH destination and the qualified
table2asn deployment configuration explicitly:

```sh
BASECRAFT_BUILD_HOST=your-build-host \
BASECRAFT_TABLE2ASN_DEPLOYMENT=/path/to/private/deployment \
just test-build
```

The deployment directory contains the qualified manifest and frozen lock;
see [table2asn packaging](../packaging/table2asn/README.md). Personal values
belong in your shell environment or private build wrapper. A missing host or
deployment configuration stops packaging before compilation.

`build.json` contains portable source, toolchain, binary and validation
identifiers. Machine-specific Python and log locations and the actual SSH
host live in a sibling `<release-directory>.local.json` deployment receipt,
outside the bundle directory. The launcher and plugin host read this receipt;
older feedback bundles with embedded local configuration remain readable.
Keep the sibling receipt with a local installation, but exclude it from exports.
Source and test environments are not a standalone redistributable Python
runtime. Local table2asn channel files also require replacement with a qualified
public deployment before redistributing a bundle.

The publisher validates an unpublished candidate before atomically switching
the current-build link, retaining the previous build. Launching never compiles.

Focused packaging regressions (no compilation):

```sh
python3 -m unittest discover -s tools -p test_delivery.py
python3 -m unittest discover -s packaging/table2asn -p test_stage_deployment.py
```
