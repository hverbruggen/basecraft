# Matching table2asn packages (staged, not released)

The selected source candidate is Toolkit release 30.8.0 at immutable commit
`01ab652af9da9d557b1553504d4dcd4db12f5b86`. `source.toml` pins the
63 MB source archive, streamed and hashed on 2026-10-28 without compiling it.
`meta.yaml.in` is an intentionally incomplete recipe template, not a usable
package or proof of platform support. No binaries, dependency locks, channels
or uploads are supplied here. Do not enable this runtime in the plugin until
those inputs exist.

## Source and dependency assessment

The pinned `src/app/table2asn/CMakeLists.table2asn.app.txt` links
`data_loaders_util`, `prosplign`, `xdiscrepancy`, `xmlwrapp`, `xvalidate`, and
`xobjwrite`; it excludes Cygwin. `src/misc/xmlwrapp/CMakeLists.xmlwrapp.lib.txt`
requires XML, XSLT and EXSLT. These are direct requirements, not the full
transitive closure. Toolkit uses C++20 and supplies native Unix/macOS and
Visual Studio configuration scripts. Windows must use native MSVC, not Cygwin.

The pinned Toolkit `LICENSE` declares NCBI government-authored code public
domain with explicit third-party exceptions. Examples include XMLWrapp,
compression libraries, regular-expression code, TLS and hashing code. It also
lists AGPL Gnomon and LGPL database code elsewhere in the tree: do not label
the entire source archive public domain or assume those libraries enter this
target. Audit the actual target closure and preserve all applicable notices.

The pinned `src/build-system/cmake/conanfile.py` uses dependency ranges,
platform-dependent defaults and optional locally cached/private NCBI packages.
Do not use its default network solve as a reproducible release recipe. Either
provide all required components through a locked Conda build environment or
freeze a reviewed Conan lock/cache with exact revisions and hashes per target.
Exclude incidental private packages and ambient machine libraries.

## Native feasibility plan (not executed)

| Target | Build route | Open checks |
| --- | --- | --- |
| linux-64 | GCC/Clang C++20, native Conda package | minimum glibc, compiler runtime, full linked closure |
| osx-64 | Intel Apple Clang, native Conda package | deployment target, remaining Intel support, SDK/toolchain |
| osx-arm64 | ARM Apple Clang, native Conda package | SDK/deployment target, arm64 dependencies |
| win-64 | MSVC 2022, native Conda package | XML/XSLT/EXSLT discovery, runtime DLL closure, environment activation |

After the coordinator releases the build hold, use external build/scratch
storage and the shared build-host lock for Linux. Native macOS/Windows builders
are still required; a Linux build is not evidence for them. Proposed Unix
configuration (check native output before freezing a build script):

```sh
./cmake-configure --with-build-root=table2asn-build \
  --with-projects='app/table2asn' --with-targets='^table2asn$' \
  --without-dll --with-components='XML;XSLT;EXSLT'
cmake --build table2asn-build/build --target table2asn --parallel "$CPU_COUNT"
```

Windows has `cmake-configure.bat` and accepts the same project/component/build
root options, with native batch quoting and Release configuration. Confirm
its generated build/bin directories. Static Toolkit libraries do not prove
that external libraries are static. Package the executable plus its actual
runtime dependencies; retain configure/link reports and source notices.

Capture `table2asn -version` on each native build. In this source the version
is constructed from `NCBI_SC_VERSION_PROXY` and
`NCBI_TEAMCITY_BUILD_NUMBER_PROXY`, so the Toolkit tag is NOT itself a promised
binary-reported table2asn version. Investigate release build metadata before
claiming equivalence with NCBI/Bioconda binaries. Require matching reported
versions and common source/settings across all four targets; preserve the
complete per-target output. Refuse publication if these disagree.

Build versioned Conda artifacts from that one commit. Retain source and binary
archives under immutable version/build URLs, together with SHA-256, package
build identity, tool version output, dependency closure and licenses. Windows
needs a channel that accepts Windows packages; Bioconda's lack of Windows
support does not imply Conda lacks it. No channel/hosting publication is part
of this task.

## Shared runtime integration

Reuse `basecraft-plugin-host::runtime` and `registry::provision_bundled`.
RuntimeSpec now accepts `lock` (package-relative `pixi.lock`) and
`version-checks.<executable>` (`args`, exact trimmed combined stdout/stderr in
`expected`). It always uses `pixi install --locked` when a lock is declared
and rejects any rewritten lock. Runtime receipts retain binary hashes and
verified exact version output as well as the existing package/manifest/lock
hashes. Index selections use plugin identity plus complete package digest;
older environments remain available for older bundles and accepted jobs.

For each target, generate a lock from the runtime-generated manifest with the
same channels, dependencies, platform and SDK defaults. Freeze the entire
closure (Python, NumPy, Arrow and table2asn included), immutable artifact URLs
and SHA-256. Each delivered plugin bundle must carry the lock matching its
manifest/platform. A lock produced against different project metadata may be
rejected by Pixi: verify with the real bundled Pixi before delivery.

Only then replace table2asn's informational `[environment]` with `[runtime]`,
including the pinned Conda package, executable and native verified version
check. The shared deployment API provisions bundled dependencies explicitly;
registry listing stays read-only. Wire the deployment/first-use lifecycle
through the existing GUI, CLI, Python and MCP routes after coordination with
the runner owner. No caller or automatic trigger is connected in this staged
change. Tighten table2asn's executable resolver at that point so ambient PATH
or `BASECRAFT_TABLE2ASN` cannot supersede a selected pinned runtime. Until
then, the current plugin and its explicit development override are unchanged.

## Required validation before activation

Prepared host tests cover bundled frozen locks, version mismatch, lock
replacement, changed binaries, idempotent installation and retained versions.
They have NOT run. Once permitted, run focused `runtime_install` tests, then
exact-commit `just check`/`just ci` on build-host under the shared compile lock.
On every supported native target: install offline from retained artifacts,
check versions and hashes, run the same GenBank submission fixture suite
(valid records plus validation errors), compare normalized validation issues
and verify cancellation/failure preservation through the shared runner.
Missing historical packages must fail with a version-specific diagnosis,
never use latest. A successful install or a created `.sqn` alone is not a
scientific validation pass. Native GUI checks and publication remain with the
coordinator.

## Primary references inspected

- [Pinned target definition](https://github.com/ncbi/ncbi-cxx-toolkit-public/blob/01ab652af9da9d557b1553504d4dcd4db12f5b86/src/app/table2asn/CMakeLists.table2asn.app.txt)
- [Pinned license and exceptions](https://github.com/ncbi/ncbi-cxx-toolkit-public/blob/01ab652af9da9d557b1553504d4dcd4db12f5b86/LICENSE)
- [Pinned Conan dependencies](https://github.com/ncbi/ncbi-cxx-toolkit-public/blob/01ab652af9da9d557b1553504d4dcd4db12f5b86/src/build-system/cmake/conanfile.py)
- [NCBI build guide](https://ncbi.github.io/cxx-toolkit/pages/ch_cmconfig)
- [NCBI releases](https://github.com/ncbi/ncbi-cxx-toolkit-public/releases)
- [Bioconda recipe guidelines](https://bioconda.github.io/contributor/guidelines.html)

## Offline package qualification

`qualify.py` exercises the existing CLI, Python SDK and raw MCP submission tools
against a **real, explicitly supplied** native validator. It creates only demo
projects and packages in a new external directory; it never submits or uploads.
Use an already built CLI and its matching SDK Python, with the matching SDK on
`PYTHONPATH`:

```sh
PYTHONPATH=python/src /path/to/sdk/python packaging/table2asn/qualify.py \
  --binary /path/to/basecraft --table2asn /path/to/bin/table2asn \
  --root /external/new-demo-directory
```

Keep the executable's basename `table2asn`: upstream includes its program name
in `-version` output. All profiles, input copies, command transcripts, reviews,
saved/reopened submission documents, run evidence and packages remain under the
new directory. `receipt.json` records executable/SDK/script hashes and actual
version output; `inventory.json` hashes all retained files, including hidden
project evidence. Interrupted/failed qualification is never marked passed.

Each of CLI, Python and MCP must produce locally passing COI (mitochondrial
code 5, explicitly confirmed frames) and 18S packages. The rbcL frameshift and
ITS invalid-country examples must remain errors mapped to the affected record.
The script tests refusal before row confirmation, checks saved confirmation
and frames after reopening, and compares exported validator inputs across the
three routes. Local tool success requires the shared adapter's `passed` status,
zero errors and retained `.sqn`/`.gbf`; a created `.sqn` alone is insufficient.

Each package has `DEMO-ONLY.txt`. The submitter and author are deliberately
fictional placeholders using `example.invalid`; specimen/sequence data are
representative fixtures, not an approved real study. Real collection metadata,
vouchers, taxon identifications, permissions, submitter details, authors and
publication title must replace these examples and be reviewed before a fresh
export/validation. Passing these checks is not GenBank acceptance.

## Pinned executable handoff

For a selected shared runtime, the host sets `BASECRAFT_RUNTIME_EXECUTABLES`
to a JSON map of declared executable names to selected absolute paths. The
runtime receipt verifies executable hashes before invocation. table2asn honors
this selection before `BASECRAFT_TABLE2ASN`, sibling Python tools or PATH;
missing/invalid selected paths report unavailable without selecting a different
version. The host clears inherited mappings for unprovisioned plugins. Until
real packages/locks are available and deployment activation is connected,
unprovisioned development keeps its existing explicit override behavior.
