#!/usr/bin/env bash
# Translate crates/protocol to Lean with Charon and Aeneas.
# Set CHARON_DIR and AENEAS_DIR if the tools are not in ~/verif.
# The commits that the proofs expect are in proofs/TOOLS.
set -euo pipefail

charon_dir=${CHARON_DIR:-$HOME/verif/charon}
aeneas_dir=${AENEAS_DIR:-$HOME/verif/aeneas}
root=$(git rev-parse --show-toplevel)
llbc=$root/target/protocol.llbc

# `cargo miri setup` of any other nightly also writes ~/.cache/miri, and Charon then
# reads a std of the wrong compiler. So Charon gets a sysroot and a cache of its own.
export MIRI_SYSROOT=$HOME/.cache/charon-protocol/miri
export CHARON_CACHE_DIR=$HOME/.cache/charon-protocol

mkdir -p "$root/target"
# Start clean, so a file from an older run cannot hide a problem.
rm -rf "$root/proofs/Protocol/Code"
(cd "$root/crates/protocol" && PATH=$charon_dir/bin:$PATH charon cargo --preset=aeneas --dest-file="$llbc")
"$aeneas_dir/bin/aeneas" -backend lean "$llbc" -dest "$root/proofs" -subdir /Protocol/Code -split-files

# An axiom means Aeneas did not know a function. A sorry means it could not
# translate a body. Either way the proofs would trust code that nobody checked.
if grep -rnw 'axiom\|sorry' "$root/proofs/Protocol/Code"; then
    echo "error: the generated code contains an axiom or a sorry" >&2
    exit 1
fi
