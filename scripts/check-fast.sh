#!/usr/bin/env bash
# The quick checks for a local loop. `check-all.sh` is still the gate before every commit.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
cargo fmt --check
cargo clippy --all-targets -q -- -D warnings
cargo test -q --workspace
scripts/test-addon-clients.sh
stylua --check addon
selene --quiet addon/GnomishRelay addon/transport
selene --quiet --config addon/GnomishRelaySelfTest/selene.toml addon/GnomishRelaySelfTest addon/GnomishRelaySelfTest_Slot addon/GnomishRelaySelfTest_Off addon/GnomishRelaySelfTest_Old
echo "fast checks ok"
