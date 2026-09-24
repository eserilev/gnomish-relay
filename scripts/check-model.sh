#!/usr/bin/env bash
# Simulates the transport model. Every property must hold, and every witness must fail.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v quint >/dev/null; then
  echo "error: quint not found. Install it: npm install -g @informalsystems/quint" >&2
  exit 1
fi

model=models/transport.qnt
quint typecheck "$model"

simulate() {
  quint run "$model" --invariant="$1" --max-samples=20000 --max-steps=40 --seed="$2" 2>&1 || true
}

# An exit code cannot tell a violation from a typo in a name, so read the verdict.
for property in runsOnce noLostReply restoreSafe noStuckMessage bodyBounded; do
  for seed in 1 2 3; do
    if ! simulate "$property" "$seed" | grep -q "No violation found"; then
      echo "error: $property fails (seed $seed). Run: quint run $model --invariant=$property --seed=$seed" >&2
      exit 1
    fi
  done
  echo "model: $property holds"
done

# A witness that holds means the simulator never reached a hard state.
for witness in neverFull neverRestored neverTwoAnswers neverOutboxReply; do
  if ! simulate "$witness" 1 | grep -q "\[violation\] Found an issue"; then
    echo "error: witness $witness was never reached" >&2
    exit 1
  fi
  echo "model: $witness reached"
done
