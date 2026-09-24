import Protocol.Spec.Bytes

/-!
# Folder paths

A path is compared by its parts, never as text. So `/home/x/Code2` is not
inside `/home/x/Code`.
-/

namespace Protocol.Spec

def slash : Byte := ch '/'

/-- The non-empty parts of a path: `/a//b/` has the parts `a` and `b`. -/
def pathParts (p : List Byte) : List (List Byte) := (p.splitOn slash).filter (· ≠ [])

/-- A clean path: starts with `/`, no empty part, no `.` and no `..`, no trailing `/`. -/
def cleanPath (p : List Byte) : Prop :=
  p = (pathParts p).flatMap (slash :: ·) ∧ pathParts p ≠ [] ∧
    ∀ part ∈ pathParts p, part ≠ ascii "." ∧ part ≠ ascii ".."

def insideRoot (root p : List Byte) : Prop := pathParts root <+: pathParts p

/-- A plain relative path like `src/app`: no leading `/`, no empty part, no `.` or `..`. -/
def cleanRelative (q : List Byte) : Prop :=
  q ≠ [] ∧ q.head? ≠ some slash ∧
    ∀ part ∈ q.splitOn slash, part ≠ [] ∧ part ≠ ascii "." ∧ part ≠ ascii ".."

end Protocol.Spec
