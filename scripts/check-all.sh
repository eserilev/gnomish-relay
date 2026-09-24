#!/usr/bin/env bash
# Every check that must pass before a commit.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
cargo fmt --check
cargo clippy --all-targets -q -- -D warnings
cargo test -q
scripts/check-proofs.sh
echo "all checks ok"
