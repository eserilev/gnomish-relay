#!/usr/bin/env bash
# Packs the release build of TARGET into dist/, with its SHA-256 sum (SPEC.md 11.3).
# The release job runs it on each OS. install.sh and install.ps1 read what it makes.
set -euo pipefail
target=$1
root=$(git rev-parse --show-toplevel)
cd "$root"
mkdir -p dist
build=target/$target/release
if [[ $target == *windows* ]]; then
  name=gnomish-relay-$target.zip
  pwsh -NoProfile -Command "Compress-Archive -Force -Path '$build/gnomish-relay.exe' -DestinationPath 'dist/$name'"
else
  name=gnomish-relay-$target.tar.gz
  tar -czf "dist/$name" -C "$build" gnomish-relay
fi
cd dist
if command -v sha256sum > /dev/null; then
  sha256sum "$name" > "$name.sha256"
else
  shasum -a 256 "$name" > "$name.sha256"
fi
echo "dist/$name"
