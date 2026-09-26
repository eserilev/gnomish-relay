#!/usr/bin/env bash
# The end-to-end test of the bridge with the real story program of Timeways (SPEC.md 9.8).
# Usage: scripts/e2e-timeways.sh [--claude]
# TIMEWAYS_REPO is the Timeways checkout; the default is the folder next to this repo.
# With --claude, a second case also asks the real `claude` for words.
set -euo pipefail
cd "$(dirname "$0")/.."
repo="${TIMEWAYS_REPO:-$(cd .. && pwd)/timeways}"
# A target folder of our own, so a build never clashes with a build in the Timeways repo.
target="$(pwd)/target/timeways"
CARGO_TARGET_DIR="$target" cargo build -q --release \
  --bin timeways-story --bin timeways-pack --manifest-path "$repo/Cargo.toml"
export TIMEWAYS_STORY="$target/release/timeways-story"
export TIMEWAYS_PACK="$target/release/timeways-pack"
export TIMEWAYS_ADDON="$repo/addon/Timeways"
tests=(the_real_story_program_answers_real_batches_through_the_real_bridge_with_no_model)
if [[ "${1:-}" == "--claude" ]]; then
  tests+=(the_real_story_program_gets_words_from_claude_through_the_real_bridge)
fi
for test in "${tests[@]}"; do
  cargo test -q -p bridge --test timeways_e2e -- --ignored --exact --nocapture "$test"
done
