import Protocol.Spec.Action

/-!
# The sandbox policy

A path is hidden with the predicate of the classifier (SPEC 6.6.3): it is inside a
hidden folder, or a run of its parts matches a hidden pattern. Both compare with no
regard to ASCII case. "Inside" is the parts prefix of S5.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def hiddenBy (policy : sandbox.SandboxPolicy) (p : List Byte) : Prop :=
  (∃ f ∈ strs policy.hidden_folders.val, insideCI f p) ∨
    matchesPattern (strs policy.hidden_paths.val) p ∨
    matchesPattern (strs policy.hidden_writes.val) p

end Protocol.Spec
