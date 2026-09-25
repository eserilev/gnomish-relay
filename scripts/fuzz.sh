#!/usr/bin/env bash
# Runs fuzz targets for SECONDS each (default 30): the TARGETs, or every target.
# CI runs each target short in its own job, and nightly runs them all long.
# A crash leaves its input in fuzz/artifacts/. Turn it into a regression test first.
set -euo pipefail
cd "$(dirname "$0")/../fuzz"
seconds="${1:-30}"
shift || true
targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
  targets=(frame records folder lua lua_model chat_text popup screenshot relay saved config live restore flags acp markdown)
fi
log=$(mktemp)
trap 'rm -f "$log"' EXIT

for target in "${targets[@]}"; do
  mkdir -p "corpus/$target"
  if ! cargo +nightly fuzz run "$target" "corpus/$target" "seeds/$target" -- \
      -max_total_time="$seconds" >"$log" 2>&1; then
    tail -40 "$log"
    echo "fuzz: $target failed" >&2
    exit 1
  fi
  echo "$target: $(grep -E '^Done' "$log")"
done
echo "fuzz ok"
