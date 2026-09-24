import Protocol.Spec.Bytes

/-!
# The permission popup

```text
rm -rf ~/x [... 812 bytes cut ...] && echo done
the agent says: run the tests
```

The raw command comes first. The label of the agent comes after it, marked.
-/

namespace Protocol.Spec

def commandBudget : Nat := 300
def labelBudget : Nat := 120

def hexDigit (n : Nat) : Byte := if n < 10 then ch '0' + BitVec.ofNat 8 n else ch 'a' + BitVec.ofNat 8 (n - 10)

def printable (b : Byte) : Bool := ch ' ' ≤ b ∧ b ≤ ch '~'

/-- Printable ASCII as is, `\` as `\\`, every other byte as `\xHH`. -/
def showByte (b : Byte) : List Byte :=
  if b = ch '\\' then [ch '\\', ch '\\']
  else if printable b then [b]
  else [ch '\\', ch 'x', hexDigit (b.toNat / 16), hexDigit (b.toNat % 16)]

def showBytes (l : List Byte) : List Byte := l.flatMap showByte

def cutMarker (n : Nat) : List Byte := ascii " [... " ++ decimal n ++ ascii " bytes cut ...] "

def commandPart (c : List Byte) : List Byte :=
  if c.length ≤ commandBudget then showBytes c
  else
    showBytes (c.take (commandBudget / 2)) ++ cutMarker (c.length - commandBudget) ++
      showBytes (c.drop (c.length - commandBudget / 2))

def labelPart (l : List Byte) : List Byte :=
  if l.length ≤ labelBudget then showBytes l else showBytes (l.take labelBudget) ++ ascii " [...]"

def popupBytes (command label : List Byte) : List Byte :=
  commandPart command ++ ascii "\nthe agent says: " ++ labelPart label

/-- Reads `showBytes` output back. `none` for anything `showBytes` never writes. -/
def unshow : List Byte → Option (List Byte)
  | [] => some []
  | b :: rest =>
    if b = ch '\\' then
      match rest with
      | c :: rest' =>
        if c = ch '\\' then (ch '\\' :: ·) <$> unshow rest'
        else if c = ch 'x' then
          match rest' with
          | h :: l :: rest'' =>
            let digit (d : Byte) : Option Nat :=
              if ch '0' ≤ d ∧ d ≤ ch '9' then some (d.toNat - 48)
              else if ch 'a' ≤ d ∧ d ≤ ch 'f' then some (d.toNat - 87) else none
            match digit h, digit l with
            | some hi, some lo => (BitVec.ofNat 8 (16 * hi + lo) :: ·) <$> unshow rest''
            | _, _ => none
          | _ => none
        else none
      | [] => none
    else if printable b then (b :: ·) <$> unshow rest else none

end Protocol.Spec
