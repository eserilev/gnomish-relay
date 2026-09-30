#!/usr/bin/env bash
# The line coverage gates of CLAUDE.md.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo llvm-cov -q -p protocol --fail-under-lines 95 --summary-only > /dev/null
cargo llvm-cov -q -p app-protocol --fail-under-lines 95 --summary-only > /dev/null
cargo llvm-cov -q -p bridge --fail-under-lines 80 --summary-only > /dev/null
echo "coverage ok"
