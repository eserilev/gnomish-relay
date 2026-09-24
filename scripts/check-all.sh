#!/usr/bin/env bash
# Every check that must pass before a commit.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
cargo fmt --check
cargo clippy --all-targets -q -- -D warnings
cargo test -q
cargo deny --log-level error check
stylua --check addon
selene --quiet addon/GnomishRelay
scripts/check-proofs.sh
scripts/check-model.sh
echo "all checks ok"
