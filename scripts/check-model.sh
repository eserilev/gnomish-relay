#!/usr/bin/env bash
# Simulates the transport model and checks each property that the design meets.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v quint >/dev/null; then
  echo "error: quint not found. Install it: npm install -g @informalsystems/quint" >&2
  exit 1
fi

quint typecheck models/transport.qnt

# TODO: add noLostReply, restoreSafe, and noStuckMessage when the design fixes in
# VERIFICATION.md (item 16) are in SPEC and in the model.
for property in runsOnce; do
  for seed in 1 2 3; do
    quint run models/transport.qnt --invariant="$property" \
      --max-samples=20000 --max-steps=40 --seed="$seed" >/dev/null
  done
  echo "model: $property holds"
done
