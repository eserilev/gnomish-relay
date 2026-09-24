#!/usr/bin/env bash
# Every check that must pass before a commit.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
cargo fmt --check
cargo clippy --all-targets -q -- -D warnings
# Code behind cfg(windows) is lint-free only if clippy sees it. The tests need a
# C compiler for Windows, so this checks the library and the binary.
if rustup target list --installed | grep -q x86_64-pc-windows-gnu; then
  cargo clippy -p bridge --lib --bins --target x86_64-pc-windows-gnu -q -- -D warnings
fi
cargo test -q
cargo deny --log-level error check
stylua --check addon
selene --quiet addon/GnomishRelay
# A WoW name that the client does not have, or has only as deprecated, fails here.
scripts/wow-api.sh > /dev/null
if ! git diff --quiet -- addon/tests/api.lua; then
  echo "error: addon/tests/api.lua is out of date. Run scripts/wow-api.sh and commit." >&2
  exit 1
fi
scripts/check-proofs.sh
scripts/check-model.sh
echo "all checks ok"
