import Protocol.Spec.WowText

/-!
# The text of a notification

Any local process of the user can write a notification. So the text that the game
gets is a list of whole UTF-8 characters, and none of them is a control, bidi,
zero-width, or tag character.
-/

namespace Protocol.Spec

def isCont (b : Byte) : Prop := 0x80 ≤ b.toNat ∧ b.toNat ≤ 0xBF

/-- The second byte after a lead of 3 or 4 bytes. Out of range, it gives an overlong
form (`E0`, `F0`), a surrogate U+D800 to U+DFFF (`ED`), or a code point past U+10FFFF
(`F4`). -/
def secondFits (a b : Byte) : Prop :=
  (a.toNat = 0xE0 → 0xA0 ≤ b.toNat) ∧ (a.toNat = 0xED → b.toNat < 0xA0) ∧
    (a.toNat = 0xF0 → 0x90 ≤ b.toNat) ∧ (a.toNat = 0xF4 → b.toNat < 0x90)

/-- One whole, valid UTF-8 sequence: a lead byte and its continuation bytes. -/
def wholeChar : List Byte → Prop
  | [a] => a.toNat < 0x80
  | [a, b] => 0xC2 ≤ a.toNat ∧ a.toNat < 0xE0 ∧ isCont b
  | [a, b, c] => 0xE0 ≤ a.toNat ∧ a.toNat < 0xF0 ∧ isCont b ∧ isCont c ∧ secondFits a b
  | [a, b, c, d] =>
    0xF0 ≤ a.toNat ∧ a.toNat < 0xF5 ∧ isCont b ∧ isCont c ∧ isCont d ∧ secondFits a b
  | _ => False

/-- The C0 controls and DEL. -/
def controlChar : List Byte → Prop
  | [a] => a.toNat < 0x20 ∨ a.toNat = 0x7F
  | _ => False

/-- C1 controls (U+0080 to U+009F), U+00AD, U+034F, U+061C, U+115F, U+1160, U+180B to
U+180F, U+200B to U+200F, U+2028 to U+202E, U+2060 to U+206F, U+3164, U+FE00 to U+FE0F,
U+FEFF, U+FFA0, U+FFF9 to U+FFFB, U+1D173 to U+1D17A, the tags U+E0000 to U+E007F, and
U+E0100 to U+E01EF. -/
def hiddenChar : List Byte → Prop
  | [a, b] =>
    (a.toNat = 0xC2 ∧ (b.toNat < 0xA0 ∨ b.toNat = 0xAD)) ∨ (a.toNat = 0xCD ∧ b.toNat = 0x8F) ∨
    (a.toNat = 0xD8 ∧ b.toNat = 0x9C)
  | [a, b, c] =>
    (a.toNat = 0xE1 ∧ b.toNat = 0x85 ∧ (c.toNat = 0x9F ∨ c.toNat = 0xA0)) ∨
    (a.toNat = 0xE1 ∧ b.toNat = 0xA0 ∧ 0x8B ≤ c.toNat ∧ c.toNat ≤ 0x8F) ∨
    (a.toNat = 0xE2 ∧ b.toNat = 0x80 ∧ 0x8B ≤ c.toNat ∧ c.toNat ≤ 0x8F) ∨
    (a.toNat = 0xE2 ∧ b.toNat = 0x80 ∧ 0xA8 ≤ c.toNat ∧ c.toNat ≤ 0xAE) ∨
    (a.toNat = 0xE2 ∧ b.toNat = 0x81 ∧ 0xA0 ≤ c.toNat ∧ c.toNat ≤ 0xAF) ∨
    (a.toNat = 0xE3 ∧ b.toNat = 0x85 ∧ c.toNat = 0xA4) ∨
    (a.toNat = 0xEF ∧ b.toNat = 0xB8 ∧ 0x80 ≤ c.toNat ∧ c.toNat ≤ 0x8F) ∨
    (a.toNat = 0xEF ∧ b.toNat = 0xBB ∧ c.toNat = 0xBF) ∨
    (a.toNat = 0xEF ∧ b.toNat = 0xBE ∧ c.toNat = 0xA0) ∨
    (a.toNat = 0xEF ∧ b.toNat = 0xBF ∧ 0xB9 ≤ c.toNat ∧ c.toNat ≤ 0xBB)
  | [a, b, c, d] =>
    (a.toNat = 0xF3 ∧ b.toNat = 0xA0 ∧
      (c.toNat = 0x80 ∨ c.toNat = 0x81 ∨ (0x84 ≤ c.toNat ∧ c.toNat ≤ 0x86) ∨
        (c.toNat = 0x87 ∧ d.toNat ≤ 0xAF))) ∨
    (a.toNat = 0xF0 ∧ b.toNat = 0x9D ∧ c.toNat = 0x85 ∧ 0xB3 ≤ d.toNat ∧ d.toNat ≤ 0xBA)
  | _ => False

def visibleChar (p : List Byte) : Prop := wholeChar p ∧ ¬ controlChar p ∧ ¬ hiddenChar p

/-- The text never ends inside a UTF-8 sequence, and holds no broken one. -/
def endsOnChar (v : List Byte) : Prop := ∃ ps : List (List Byte), v = ps.flatten ∧ ∀ p ∈ ps, wholeChar p

/-- Every character of the text is visible. -/
def noticeSafe (v : List Byte) : Prop := ∃ ps : List (List Byte), v = ps.flatten ∧ ∀ p ∈ ps, visibleChar p

/-- WoW shows the text as plain text: each `|` comes as `||` (S10). -/
def pipesDoubled (v : List Byte) : Prop := ∃ plain, wowPlain v = some plain

end Protocol.Spec
