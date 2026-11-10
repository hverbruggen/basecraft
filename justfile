# Basecraft dev commands. Run `just` to list them.

testdata_dir := env("BASECRAFT_TESTDATA", home_directory() / ".cache/basecraft-testdata")

default:
    @just --list

# Build on the build host, test locally, and publish the executable used by the launcher.
test-build jobs="24":
    python3 tools/test_build.py --jobs {{jobs}}

# Include the full ribbon/dialog screenshot batch for release preparation or on request.
test-build-release jobs="24":
    python3 tools/test_build.py --jobs {{jobs}} --ribbon-screenshots

# Install the desktop app and CLI alongside the shared distance engine.
install: install-distance
    cargo install --path crates/basecraft-cli --locked --root {{home_directory()}}/.local
    cargo install --path crates/basecraft-gui --locked --root {{home_directory()}}/.local

# Install the engine used by the neighbor-joining GUI buttons and distance plugin.
install-distance:
    cargo install --path crates/basecraft-distance --locked --root {{home_directory()}}/.local

# Point git at the repo's hooks. Run once after cloning.
hooks:
    git config core.hooksPath .githooks

# Format, lint, test. The pre-commit hook runs the first two.
check:
    python3 -m unittest discover -s testdata/compact
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo test -p basecraft-ops --features test-counters --test budgets
    cargo test -p basecraft-io --features test-heap heap::tests
    cargo test -p basecraft-ops --features test-heap --test heap_budgets
    pixi run ruff check python plugins

# Everything. Run before every push.
ci: check schemas-check deny protocol-license replay qml-test
    @echo "instruction-count benchmarks: not set up yet"

# Generate JSON Schema and Arrow schemas from basecraft-protocol.
schemas:
    cargo run -q -p basecraft-cli -- schemas schemas

# Fail if the committed schemas differ from freshly generated ones.
schemas-check: schemas
    git diff --exit-code -- schemas

deny:
    cargo deny check

# basecraft-protocol is Apache-2.0 and must not depend on GPL code.
protocol-license:
    #!/usr/bin/env bash
    set -euo pipefail
    tree=$(cargo tree -p basecraft-protocol -e normal --prefix none --format '{p} {l}')
    gpl=$(grep -E 'GPL' <<< "$tree" | grep -v 'LGPL' || true)
    if [ -n "$gpl" ]; then
        echo "basecraft-protocol depends on GPL code:" >&2
        echo "$gpl" >&2
        exit 1
    fi

# QML component tests, headless. File reads let sequencing tests load production QML.
qml-test:
    #!/usr/bin/env bash
    set -euo pipefail
    for scale in 1 1.25 1.5 2; do
        QML_XHR_ALLOW_FILE_READ=1 QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME= QT_QUICK_CONTROLS_STYLE=Basic QT_SCALE_FACTOR="$scale" \
            "$(qmake6 -query QT_HOST_BINS)/qmltestrunner" -input crates/basecraft-gui/tests/qml
    done

# Replay tests for every operation type.
replay:
    cargo test -q -p basecraft-ops --test replay

# Warm-cache headless timings. Run on the build host under its shared lock.
bench *args:
    python3 benches/run.py {{args}}

# Compact defaults need no compilation. Legacy small/full stress tiers remain opt-in.
# Run compiler-backed tiers on the configured remote build machine.
testdata scale="compact":
    #!/usr/bin/env bash
    set -euo pipefail
    case '{{scale}}' in
        compact) python3 testdata/compact/generate.py --out '{{testdata_dir}}/compact' ;;
        small|full) cargo run --release -q -p basecraft-testgen -- --out '{{testdata_dir}}' --scale '{{scale}}' ;;
        *) echo 'scale must be compact, small or full' >&2; exit 2 ;;
    esac

# Build the basecraft-core Python module into the pixi environment.
py:
    pixi run maturin develop -m crates/basecraft-py/Cargo.toml
