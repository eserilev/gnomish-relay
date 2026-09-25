import Protocol.Spec.WowText

/-!
# Reply blocks (`SPEC.md` 7.3.1)

The bridge renders a Markdown reply as blocks for the game window:

```text
ESC M 1                       the marker
\n h US 1 US Title            a heading
\n p US Some |cffffd100bold|r text.
\n t US 1 US Name US Age      a table row
\n                            the end
```

A text is read left to right in tokens: `||` is one `|`, a color code opens a color,
`|r` closes it, and in a SimpleHTML text an entity stands for `<`, `>`, or `&`.
So `||r` is an escaped `|`, then the letter `r`.
-/

namespace Protocol.Spec

def nl : Byte := 0x0A
def us : Byte := 0x1F

def marker : List Byte := [0x1B, ch 'M', ch '1']

def boldCode : List Byte := ascii "|cffffd100"
def italicCode : List Byte := ascii "|cffc0c8ff"
def boldItalicCode : List Byte := ascii "|cffffe680"
def codeCode : List Byte := ascii "|cffb8e0b8"
def linkCode : List Byte := ascii "|cff69b4ff"

/-- The five color codes of `inline.rs`: the only ones that a text can hold. -/
def colorCodes : List (List Byte) := [boldCode, italicCode, boldItalicCode, codeCode, linkCode]

def resetCode : List Byte := ascii "|r"

def entities : List (List Byte) := [ascii "&lt;", ascii "&gt;", ascii "&amp;"]

/-- A byte that a field can hold: no `\n`, no US, no ESC, no other byte below `20`,
and no `7F`. -/
def fieldByte (b : Byte) : Prop := ch ' ' ≤ b ∧ b ≠ 0x7F

/-- A byte that stands for itself. In a SimpleHTML text (`html`), `<`, `>`, and `&`
never do. -/
def plainByte (html : Bool) (b : Byte) : Prop :=
  fieldByte b ∧ b ≠ pipe ∧ (html = true → b ≠ ch '<' ∧ b ≠ ch '>' ∧ b ≠ ch '&')

/-- `Text html o t o'`: the text `t`, read in tokens from a start where a color is open
(`o = true`) or not, ends with a color open (`o'`) or not. A color code comes only when
no color is open, and `|r` only when one is. -/
inductive Text (html : Bool) : Bool → List Byte → Bool → Prop
  | nil (o : Bool) : Text html o [] o
  | plain {o o' : Bool} {b : Byte} {t : List Byte} :
      plainByte html b → Text html o t o' → Text html o (b :: t) o'
  | pipes {o o' : Bool} {t : List Byte} : Text html o t o' → Text html o (pipe :: pipe :: t) o'
  | entity {o o' : Bool} {e t : List Byte} :
      html = true → e ∈ entities → Text html o t o' → Text html o (e ++ t) o'
  | color {o' : Bool} {c t : List Byte} :
      c ∈ colorCodes → Text html true t o' → Text html false (c ++ t) o'
  | reset {o' : Bool} {t : List Byte} : Text html false t o' → Text html true (resetCode ++ t) o'

/-- S24: a text field starts and ends with no color open. -/
def escapedText (html : Bool) (t : List Byte) : Prop := Text html false t false

/-- S23: a text field holds only field bytes. -/
def fieldText (_html : Bool) (t : List Byte) : Prop := ∀ b ∈ t, fieldByte b

def digitByte (b : Byte) : Prop := ch '0' ≤ b ∧ b ≤ ch '9'

/-- The cells of a table row: each is US and then a text. -/
inductive Cells (textOk : Bool → List Byte → Prop) : List Byte → Prop
  | nil : Cells textOk []
  | cons {t rest : List Byte} : textOk false t → Cells textOk rest → Cells textOk (us :: t ++ rest)

/-- One block. `textOk html t` says what a text field holds. The texts of headings,
paragraphs, list items, and quotes go into SimpleHTML (`html = true`). -/
inductive Block (textOk : Bool → List Byte → Prop) : List Byte → Prop
  | heading {l : Byte} {t : List Byte} :
      l = ch '1' ∨ l = ch '2' ∨ l = ch '3' → textOk true t →
      Block textOk ([nl, ch 'h', us, l, us] ++ t)
  | paragraph {t : List Byte} : textOk true t → Block textOk ([nl, ch 'p', us] ++ t)
  | item {l : Byte} {d t : List Byte} :
      ch '0' ≤ l ∧ l ≤ ch '4' → d.length ≤ 9 → (∀ b ∈ d, digitByte b) → textOk true t →
      Block textOk ([nl, ch 'l', us, l, us] ++ d ++ [us] ++ t)
  | quote {t : List Byte} : textOk true t → Block textOk ([nl, ch 'q', us] ++ t)
  | code {t : List Byte} : textOk false t → Block textOk ([nl, ch 'c', us] ++ t)
  | row {f : Byte} {cells : List Byte} :
      f = ch '0' ∨ f = ch '1' → Cells textOk cells → Block textOk ([nl, ch 't', us, f] ++ cells)
  | rule : Block textOk [nl, ch 'r']

/-- The marker, then zero or more blocks, then `\n`. -/
def Rendered (textOk : Bool → List Byte → Prop) (out : List Byte) : Prop :=
  ∃ blocks : List (List Byte), (∀ b ∈ blocks, Block textOk b) ∧
    out = marker ++ blocks.flatten ++ [nl]

end Protocol.Spec
