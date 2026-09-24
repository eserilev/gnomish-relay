import Protocol.Spec.Bytes
import Protocol.Code.Funs

/-!
# What a record is

`token US chat US id US cwd US flags US name US text`, records divided by RS.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def RS : Byte := 0x1E
def US : Byte := 0x1F

def maxRecords : Nat := 16

def idByte (b : Byte) : Prop :=
  (ch 'a' ≤ b ∧ b ≤ ch 'z') ∨ (ch '0' ≤ b ∧ b ≤ ch '9') ∨ b = ch '_' ∨ b = ch '-'

/-- Safe in a file name and a state key: no `/`, no `.`, no space. -/
def validId (l : List Byte) : Prop := 1 ≤ l.length ∧ l.length ≤ 32 ∧ ∀ b ∈ l, idByte b

/-- A field that cannot shift the fields after it. -/
def cleanField (l : List Byte) : Prop := RS ∉ l ∧ US ∉ l

def recordBytes (r : record.Record) : List Byte :=
  bytes r.token.val ++ [US] ++ bytes r.chat.val ++ [US] ++ decimal r.id.val ++ [US] ++
    bytes r.cwd.val ++ [US] ++ bytes r.flags.val ++ [US] ++ bytes r.«name».val ++ [US] ++
    bytes r.text.val

def recordsBytes (rs : List record.Record) : List Byte :=
  [RS].intercalate (rs.map recordBytes)

/-- The text is the last field, so it can hold US. Nothing can hold RS. -/
def wellFormed (r : record.Record) : Prop :=
  validId (bytes r.token.val) ∧ validId (bytes r.chat.val) ∧
    cleanField (bytes r.cwd.val) ∧ cleanField (bytes r.flags.val) ∧
    cleanField (bytes r.«name».val) ∧ RS ∉ bytes r.text.val

end Protocol.Spec
