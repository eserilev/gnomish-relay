#!/usr/bin/env bash
# Translate crates/protocol to Lean with Charon and Aeneas.
# Set CHARON_DIR and AENEAS_DIR if the tools are not in ~/verif.
# The commits that the proofs expect are in proofs/TOOLS.
set -euo pipefail

charon_dir=${CHARON_DIR:-$HOME/verif/charon}
aeneas_dir=${AENEAS_DIR:-$HOME/verif/aeneas}
root=$(git rev-parse --show-toplevel)
llbc=$root/target/protocol.llbc

mkdir -p "$root/target"
(cd "$root/crates/protocol" && PATH=$charon_dir/bin:$PATH charon cargo --preset=aeneas --dest-file="$llbc")
"$aeneas_dir/bin/aeneas" -backend lean "$llbc" -dest "$root/proofs" -subdir /Protocol/Code -split-files

# An axiom means Aeneas did not know a function. The proofs would trust it blindly.
if grep -rn '^axiom\|^  axiom\|axiom$' "$root/proofs/Protocol/Code"; then
    echo "error: the generated code contains axioms" >&2
    exit 1
fi
