import Protocol.Spec.Action
import Protocol.Spec.Lua

/-!
# Host names and allow lists (S33)

A host name is split at each dot into its labels (`List.splitOn`). A good host name is
a DNS name with at least two labels. Its last label starts with a letter, so no IP
address passes in any form, and it is not `localhost`.
-/

open Aeneas Aeneas.Std

namespace Protocol.Spec

abbrev lowerAscii (l : List Byte) : List Byte := lower l

def isLetter (b : Byte) : Prop := (97 ≤ b.toNat ∧ b.toNat ≤ 122) ∨ (65 ≤ b.toNat ∧ b.toNat ≤ 90)

theorem isDigit_iff (b : Byte) : isDigit b = true ↔ 48 ≤ b.toNat ∧ b.toNat ≤ 57 := by
  simp [isDigit, ch, BitVec.le_def]

def isLabelByte (b : Byte) : Prop := isLetter b ∨ isDigit b ∨ b = ch '-'

/-- 1 to 63 bytes of `[A-Za-z0-9-]` that do not start or end with `-`. -/
def goodLabel (l : List Byte) : Prop :=
  1 ≤ l.length ∧ l.length ≤ 63 ∧ l.head? ≠ some (ch '-') ∧ l.getLast? ≠ some (ch '-') ∧
    ∀ b ∈ l, isLabelByte b

def hostLabels (h : List Byte) : List (List Byte) := h.splitOn (ch '.')

/-- It starts with a letter, and it is not `localhost` in any case. -/
def goodLastLabel (l : List Byte) : Prop :=
  (∃ c, l.head? = some c ∧ isLetter c) ∧ lowerAscii l ≠ ascii "localhost"

def goodHostName (h : List Byte) : Prop :=
  1 ≤ h.length ∧ h.length ≤ 253 ∧ 2 ≤ (hostLabels h).length ∧
    (∀ l ∈ hostLabels h, goodLabel l) ∧
    goodLastLabel ((hostLabels h).getLast (List.splitOn_ne_nil _ _))

/-- A good host name that equals a name of the list, without ASCII case. -/
def hostAllowed (list : Slice (alloc.vec.Vec U8)) (h : List Byte) : Prop :=
  goodHostName h ∧ ∃ n ∈ strs list.val, lowerAscii n = lowerAscii h

end Protocol.Spec
