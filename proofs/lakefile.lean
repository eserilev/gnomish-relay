import Lake
open Lake DSL

-- Keep this commit in step with proofs/TOOLS.
require aeneas from git
  "https://github.com/AeneasVerif/aeneas" @ "453b09f98f2b593c0544a8ad654b77e2a3bc621a" / "backends/lean"

package protocol

@[default_target] lean_lib Protocol {}

-- The approved theorem statements, checked against the proofs on every build.
@[default_target] lean_lib Statements {}
