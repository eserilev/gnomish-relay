import Protocol.Spec.Hosts

/-!
# The target of a `CONNECT` request (S35)

The first line of a request is `CONNECT <host>:<port> HTTP/1.<d>`. The port is 1 to 5
decimal digits.
-/

open Aeneas Aeneas.Std

namespace Protocol.Spec

def allDigits (ds : List Byte) : Prop := ∀ b ∈ ds, isDigit b

/-- The value of a string of decimal digits, the first digit on top. -/
def decimalValue (ds : List Byte) : Nat := ds.foldl (fun v b => 10 * v + (b.toNat - 48)) 0

end Protocol.Spec
