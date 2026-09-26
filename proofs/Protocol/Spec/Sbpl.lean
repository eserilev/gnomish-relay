import Protocol.Spec.Lua

/-!
# How Seatbelt reads a string literal

`sandbox-exec` reads its profile language (SBPL) with a Scheme reader that comes from
TinyScheme. The model follows `readstrexp` of TinyScheme 1.41. The profile is a C
string, so a NUL byte ends the input. After a backslash, `n`, `t`, and `r` stand for a
control byte, and every other byte stands for itself. The model does not say what the
numeric escapes (`\0` to `\7`, `\x`) read as: it gives `none` for them, and
`sbpl_string` never writes one. The macOS tests in CI back the model with the real
`sandbox-exec`.
-/

namespace Protocol.Spec

def isOctalDigit (b : Byte) : Bool := ch '0' ≤ b ∧ b ≤ ch '7'

/-- The part after the opening quote. Returns the string and the rest after the
closing quote, or `none` where the reader fails or the model says nothing. -/
def sbplReadBody : List Byte → Option (List Byte × List Byte)
  | [] => none
  | c :: rest =>
    if c = 0 then none
    else if c = ch '"' then some ([], rest)
    else if c = ch '\\' then
      match rest with
      | [] => none
      | e :: rest' =>
        if e = 0 then none
        else if e = ch 'n' then consOut 10 (sbplReadBody rest')
        else if e = ch 't' then consOut 9 (sbplReadBody rest')
        else if e = ch 'r' then consOut 13 (sbplReadBody rest')
        else if isOctalDigit e ∨ e = ch 'x' ∨ e = ch 'X' then none
        else consOut e (sbplReadBody rest')
    else consOut c (sbplReadBody rest)
termination_by l => l.length
decreasing_by all_goals simp_wf

/-- A double-quoted literal: the string it reads as, and the bytes after it. -/
def sbplReadString : List Byte → Option (List Byte × List Byte)
  | q :: rest => if q = ch '"' then sbplReadBody rest else none
  | [] => none

/-- What `sbpl_string` writes: a backslash before `"` and `\`, every other byte as is. -/
def sbplEscapeByte (b : Byte) : List Byte :=
  if b = ch '"' ∨ b = ch '\\' then [ch '\\', b] else [b]

def sbplLiteral (s : List Byte) : List Byte := ch '"' :: s.flatMap sbplEscapeByte ++ [ch '"']

end Protocol.Spec
