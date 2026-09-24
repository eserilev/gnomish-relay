#!/usr/bin/env bash
# The line coverage gates of CLAUDE.md. `main.rs` is only the command line, so it is not counted.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo llvm-cov -q -p protocol --fail-under-lines 95 --summary-only > /dev/null
cargo llvm-cov -q -p bridge --fail-under-lines 80 --ignore-filename-regex 'main\.rs' --summary-only > /dev/null
echo "coverage ok"
