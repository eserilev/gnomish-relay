#!/usr/bin/env bash
# Check that the proofs match the Rust code and hold.
set -euo pipefail

root=$(git rev-parse --show-toplevel)

# The generated Lean must match the current Rust.
"$root/scripts/extract.sh" > /dev/null
if ! git -C "$root" diff --quiet -- proofs/Protocol/Code; then
    echo "error: proofs/Protocol/Code is out of date. Run scripts/extract.sh and commit." >&2
    exit 1
fi

cd "$root/proofs"
lake build

# Only the three standard axioms. No sorry, no native code from bv_decide or native_decide.
lake env lean Axioms.lean | python3 -c '
import re, sys
allowed = {"propext", "Classical.choice", "Quot.sound"}
text = sys.stdin.read()
bad = False
for name, axioms in re.findall(r"\x27(\S+)\x27 depends on axioms: \[([^\]]*)\]", text, re.S):
    extra = {a.strip() for a in axioms.split(",")} - allowed
    if extra:
        print(f"error: {name} depends on {sorted(extra)}", file=sys.stderr)
        bad = True
if "does not depend on any axioms" not in text and not re.search(r"depends on axioms", text):
    print("error: no axiom report found", file=sys.stderr)
    bad = True
sys.exit(1 if bad else 0)
'
echo "proofs ok"
