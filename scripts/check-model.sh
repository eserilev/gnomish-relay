#!/usr/bin/env bash
# Simulates each Quint model. Every property must hold, and every witness must fail.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v quint >/dev/null; then
  echo "error: quint not found. Install it: npm install -g @informalsystems/quint" >&2
  exit 1
fi

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

# The simulations share nothing, so they run side by side, one per core.
cores=$(nproc 2>/dev/null || sysctl -n hw.ncpu)

# simulate <model> <steps> <invariant> <seed>
simulate() {
  while [ "$(jobs -rp | wc -l)" -ge "$cores" ]; do
    wait -n
  done
  quint run "$1" --invariant="$3" --max-samples=20000 --max-steps="$2" --seed="$4" \
    > "$out/$(basename "$1").$3.$4" 2>&1 || true &
}

# start <model> <max steps> "<properties>" "<witnesses>"
start() {
  local model=$1 steps=$2 properties=$3 witnesses=$4
  quint typecheck "$model"
  for property in $properties; do
    for seed in 1 2 3; do
      simulate "$model" "$steps" "$property" "$seed"
    done
  done
  for witness in $witnesses; do
    simulate "$model" "$steps" "$witness" 1
  done
}

# An exit code cannot tell a violation from a typo in a name, so read the verdict.
# verify <model> "<properties>" "<witnesses>"
verify() {
  local model=$1 properties=$2 witnesses=$3 name
  name=$(basename "$model")
  for property in $properties; do
    for seed in 1 2 3; do
      if ! grep -q "No violation found" "$out/$name.$property.$seed"; then
        tail -20 "$out/$name.$property.$seed" >&2
        echo "error: $property fails (seed $seed). Run: quint run $model --invariant=$property --seed=$seed" >&2
        exit 1
      fi
    done
    echo "$model: $property holds"
  done

  # A witness that holds means the simulator never reached a hard state.
  for witness in $witnesses; do
    if ! grep -q "\[violation\] Found an issue" "$out/$name.$witness.1"; then
      tail -20 "$out/$name.$witness.1" >&2
      echo "error: witness $witness of $model was never reached" >&2
      exit 1
    fi
    echo "$model: $witness reached"
  done
}

# The first run downloads the evaluator of Quint. Parallel first runs break each other's download.
quint run models/transport.qnt --max-samples=1 --max-steps=1 > /dev/null

transport_properties="runsOnce noLostReply restoreSafe noStuckMessage bodyBounded"
transport_witnesses="neverFull neverRestored neverTwoAnswers neverOutboxReply"
corner_properties="oneStrip ownEvents retryPaused blockedInTime fairWait"
corner_witnesses="neverTurn neverWarned neverOutbox neverBoth neverRetry"

start models/transport.qnt 40 "$transport_properties" "$transport_witnesses"
start models/corner.qnt 100 "$corner_properties" "$corner_witnesses"
wait

verify models/transport.qnt "$transport_properties" "$transport_witnesses"
verify models/corner.qnt "$corner_properties" "$corner_witnesses"
