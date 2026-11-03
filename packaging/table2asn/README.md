# Pinned table2asn runtime

The Linux feedback runtime uses Toolkit30.8.0 commit
`01ab652af9da9d557b1553504d4dcd4db12f5b86` (archive SHA256
`de56c4be23c50d93edf0014e310471be03bb2c8f114b8b414f99149518201d95`).
The Conda **package version is30.8.0**; measured **tool output is
`table2asn: 1.30.0\n`**. These are separate identities. The source supplies
SC_VERSION30 and generated build number0; no NCBI release metadata is invented.
`source.toml` records the common source; `linux-64.json` records the measured
artifact. A Toolkit version must never be presented as the tool version.

## Linux build and dependency closure

`recipe.yaml` and `build.sh` build the real Conda artifact from source.
`build-lock.json` retains every solved build/host package's URL, SHA256,
version, build, dependencies and license. The recipe pins the complete closure
by version/build. Use Rattler-Build0.80.0; its install verifies artifact hashes.
Run `verify-build-lock.py` against the actual Rattler build directory to verify
that both installed environments match the retained hashes and identities.

All configure/compiler probes/builds run on build-host under the shared
`~/.cache/basecraft-test-builds/compile.lock`, with external output and TMP.
Example (absolute task-owned paths, no application directory build output):

```sh
export TMPDIR=/external/table2asn/tmp
mkdir -p "$TMPDIR"
flock ~/.cache/basecraft-test-builds/compile.lock \
  rattler-build build --no-config --recipe /external/table2asn/recipe \
  --output-dir /external/table2asn/output -c conda-forge \
  --keep-build --compression-threads 4
python3 /external/table2asn/recipe/verify-build-lock.py \
  /external/table2asn/output/bld/rattler-build_basecraft-table2asn_...
```

The build uses GCC13.4 and a glibc2.17 sysroot, static Toolkit libraries and
shared Conda dependencies. CMake searches only the locked host prefix and
compiler sysroot for includes/libraries. The actual link/ELF audit, rather
than `--without-dll`, determines runtime dependencies. Optional Cloudflare
compression is disabled because it requires SSE4.2; locked zlib remains.
Compiler CPU flags end with `-march=x86-64 -mtune=generic`; NCBI's explicit SSE
setting is `-msse2`. The binary is stripped and its RPATH is `$ORIGIN/../lib`.
The full Python/SDK runtime may have a higher glibc floor than the tool;
consult the actual frozen runtime lock and artifact record.

The locked Conda Berkeley DB6.2.32 package is **AGPL-3.0-only**, and iconv is
LGPL-2.1-only. No non-AGPL5.3 Conda artifact was found in the inspected channels.
NCBI's government-authored code is public domain with explicit third-party
exceptions; do not label the complete executable/source archive public domain.
The recipe declares the combined license identity. Dependencies remain separate
Conda artifacts with their own license metadata/notices. `retain-notices.py`
copies actual package licenses and source recipes, plus hashes, from the build
cache to external evidence. Retain those, the NCBI LICENSE, source archive,
build logs, actual linker command, compiler commands, CMake cache and ELF audit.
External distribution must include the applicable source/license materials;
public distribution is pending and not authorized by this local feedback work.

The older WF-0046 binary (`6dc72c0f...`, system/deb dependencies and builder
RPATH) is only baseline evidence. It is not repackaged or selected here.

## Frozen shared runtime and deployment

`plugins/table2asn/plugin.toml` declares a runtime only for `linux-64`.
`plugins/table2asn/pixi.lock` is generated from RuntimeSpec's manifest:
workspace `basecraft-plugin`, same ordered channels/platform, declared
basecraft-table2asn dependency, and SDK defaults Python>=3.11,<3.14,
pyarrow and numpy. The lock freezes their complete transitive closure and
artifact hashes. The SDK itself is the matching bundled `python/src`, not an
unrelated PyPI package; host process setup supplies that SDK via PYTHONPATH.
There are no Pixi tasks, shell activation scripts or custom downloader.

This phase uses a retained immutable **local SSD file channel**:
`file:///private/local-path`.
It is intentionally specific to this workstation. A file URL is not a public
channel or evidence of cross-machine portability. Keep this directory and
locked artifacts available; never overwrite a package under its existing
version/build. A changed release requires a new retained artifact/channel and
lock. Conda-forge dependencies have exact URLs and SHA256 in the frozen lock.

The coordinator runs this against the **final bundled plugins**, using the
same profile/config locations that normal GUI/CLI launch will use, before the
atomic current-build switch:

```sh
/path/to/candidate/bin/basecraft plugins provision-bundled basecraft.table2asn
/path/to/candidate/bin/basecraft plugins list --json
/path/to/candidate/bin/basecraft operations describe basecraft.table2asn/validate@1
```

Candidate deployment must include matching `bin/pixi`, plugins and SDK. If
provisioning from an explicit isolated profile, use BASECRAFT_PROFILE_DIR for
BOTH provisioning and subsequent validation launches. Provisioning a source
checkout or different plugin directory does not activate a later bundle:
selections are keyed by plugin identity and complete package digest. Re-run the
command after bundle assembly. No table2asn override or launcher edit is needed.

The command reuses `registry::provision_bundled`, `RuntimeSpec`, frozen
`pixi install --locked`, SDK import checks and exact executable hash/version
receipts. Discovery remains read-only. Atomic index selection occurs only
after verification; failures preserve older selections and retained environments.
The host supplies selected absolute executable paths; invalid selected Linux
runtimes fail without trying PATH or BASECRAFT_TABLE2ASN. Older package digests
remain available to older bundles/receipts. Installed Conda prefixes must not
be renamed: install the frozen package again at a new prefix when relocating.

Unsupported targets retain their existing unmanaged resolver. Explicit
provisioning reports that no runtime is declared for that platform; it does
not substitute a different native package. macOS/Windows packages and locks,
public distribution and automatic first-use activation are still pending.

## Real installation and submission qualification

Use the matching already built CLI and SDK Python. The harness creates a new
isolated profile, provisions through the actual shared CLI command, and clears
inherited tool overrides. It validates through CLI, Python and raw MCP using
the shared runner, then saves/reopens projects in new processes:

```sh
PYTHONPATH=python/src /path/to/sdk/python packaging/table2asn/qualify.py \
  --binary /path/to/basecraft --provision-runtime \
  --plugins /path/to/assembled/plugins --root /external/new-qualification
```

Repeat with a relocated bundle and a different new destination prefix. Retain
receipt, full command transcript, runtime index/receipts and inventory. The
historical `--table2asn /explicit/path` mode remains for baseline comparisons;
it does not prove normal deployment activation.

COI (code5, explicitly confirmed frame2) and18S must pass with zero errors.
rbcL's intentional frameshift/internal stops and ITS's invalid geography must
remain mapped errors. Confirmation/frame choices must survive reopen, and all
four validator inputs must match across routes. A created `.sqn` or zero process
exit alone is not validation success. Reports retain exact native output and
binary/runtime identities. GUI uses the same registry/runner selection; final
bundle normal GUI acceptance remains the coordinator's delivery check.

All examples are DEMO-ONLY with fictional author/submitter metadata. No study
is submitted or uploaded. Local validation does not establish archive acceptance.
For a real study replace fixture sequences/metadata and review all scientific
choices before fresh export/validation.

## Matching-version cross-platform plan

Native Linux x86_64, Windows MSVC x86_64, macOS Intel and Apple Silicon must use
the same pinned source/effective version settings, with target-specific locked
compilers/dependency closures and notices. Measure identical tool-version output;
do not infer it from the Toolkit tag. Qualify the same positive/negative fixtures
and selection/failure behavior per target before supporting that target.
Native macOS/Windows builders, packages, complete locks and public immutable
hosting are outstanding. No cross-compilation or unsupported native package
claim is made by this Linux feedback runtime.

Primary references: [pinned target](https://github.com/ncbi/ncbi-cxx-toolkit-public/blob/01ab652af9da9d557b1553504d4dcd4db12f5b86/src/app/table2asn/CMakeLists.table2asn.app.txt),
[pinned license](https://github.com/ncbi/ncbi-cxx-toolkit-public/blob/01ab652af9da9d557b1553504d4dcd4db12f5b86/LICENSE),
[NCBI CMake guide](https://ncbi.github.io/cxx-toolkit/pages/ch_cmconfig),
[Rattler build CLI](https://rattler-build.prefix.dev/dev/reference/cli/rattler-build/build/).
