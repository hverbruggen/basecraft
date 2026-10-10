# Basecraft dev commands. Run `just` to list them.

testdata_dir := env("BASECRAFT_TESTDATA", home_directory() / ".cache/basecraft-testdata")

default:
    @just --list

# Point git at the repo's hooks. Run once after cloning.
hooks:
    git config core.hooksPath .githooks

# Format, lint, test. The pre-commit hook runs the first two.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    pixi run ruff check python plugins

# Everything. Run before every push.
ci: check schemas-check deny protocol-license replay
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

# Replay tests for every operation type.
replay:
    cargo test -q -p basecraft-ops --test replay

# Wall-clock and frame-time budgets on this machine.
bench:
    @echo "wall-clock benchmarks: not set up yet"

# Generate synthetic test data outside the synced folder. scale: small or full.
testdata scale="small":
    cargo run --release -q -p basecraft-testgen -- --out {{testdata_dir}} --scale {{scale}}

# Build the basecraft-core Python module into the pixi environment.
py:
    pixi run maturin develop -m crates/basecraft-py/Cargo.toml
