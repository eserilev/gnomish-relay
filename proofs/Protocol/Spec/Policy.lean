import Protocol.Code.Funs

/-! # Permission levels, lowest first -/

open protocol

namespace Protocol.Spec

def rank : policy.Level → Nat
  | .Ask => 0
  | .AutoEdit => 1
  | .FullAuto => 2

end Protocol.Spec
