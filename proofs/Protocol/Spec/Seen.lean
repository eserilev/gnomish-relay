import Protocol.Spec.Bytes
import Protocol.Code.Funs

/-! # The keys of the replay window, oldest first -/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def seenKeys (s : seen.Seen) : List (List Byte × Nat) :=
  s.entries.val.map fun e => (bytes e.token.val, e.id.val)

end Protocol.Spec
