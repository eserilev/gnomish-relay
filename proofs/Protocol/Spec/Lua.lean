import Protocol.Spec.Bytes

/-!
# How Lua 5.1 reads a string literal

A model of `read_string` in Lua 5.1 `llex.c`. WoW uses Lua 5.1. The model covers
every escape that Lua 5.1 knows, not only the ones that `lua_string` writes, so the
theorem is about the real lexer. The `lua_model` fuzz target checks the model
against a real Lua 5.1 VM.
-/

namespace Protocol.Spec

def isDigit (b : Byte) : Bool := ch '0' ≤ b ∧ b ≤ ch '9'

def isNewline (b : Byte) : Bool := b = ch '\n' ∨ b = ch '\r'

/-- One to three digits of a `\ddd` escape. Lua reads at most three. -/
def readDigits : List Byte → Nat → Nat → Nat × List Byte
  | b :: rest, count, value =>
    if count < 3 ∧ isDigit b then readDigits rest (count + 1) (10 * value + (b.toNat - 48))
    else (value, b :: rest)
  | [], _, value => (value, [])

theorem readDigits_length_le (l : List Byte) (count value : Nat) :
    (readDigits l count value).2.length ≤ l.length := by
  induction l generalizing count value with
  | nil => simp [readDigits]
  | cons b rest ih =>
    unfold readDigits
    split
    · exact Nat.le_succ_of_le (ih _ _)
    · simp

def consOut (b : Byte) : Option (List Byte × List Byte) → Option (List Byte × List Byte)
  | some (s, r) => some (b :: s, r)
  | none => none

/-- The part after the opening quote. Returns the string and the rest after the
closing quote, or `none` where Lua raises a lexer error. -/
def luaReadBody (delim : Byte) : List Byte → Option (List Byte × List Byte)
  | [] => none
  | c :: rest =>
    if c = delim then some ([], rest)
    else if isNewline c then none
    else if c = ch '\\' then
      match rest with
      | [] => none
      | e :: rest' =>
        if e = ch 'a' then consOut 7 (luaReadBody delim rest')
        else if e = ch 'b' then consOut 8 (luaReadBody delim rest')
        else if e = ch 'f' then consOut 12 (luaReadBody delim rest')
        else if e = ch 'n' then consOut 10 (luaReadBody delim rest')
        else if e = ch 'r' then consOut 13 (luaReadBody delim rest')
        else if e = ch 't' then consOut 9 (luaReadBody delim rest')
        else if e = ch 'v' then consOut 11 (luaReadBody delim rest')
        -- A backslash before a line break keeps the break. `\n\r` or `\r\n` count as one.
        else if isNewline e then
          match rest' with
          | f :: rest'' =>
            if isNewline f ∧ f ≠ e then consOut 10 (luaReadBody delim rest'')
            else consOut 10 (luaReadBody delim (f :: rest''))
          | [] => consOut 10 (luaReadBody delim [])
        else if isDigit e then
          match _hd : readDigits (e :: rest') 0 0 with
          | (value, after) =>
            if value > 255 then none else consOut (BitVec.ofNat 8 value) (luaReadBody delim after)
        -- Any other escaped byte stands for itself: `\\`, `\"`, `\'`.
        else consOut e (luaReadBody delim rest')
    else consOut c (luaReadBody delim rest)
termination_by l => l.length
decreasing_by
  all_goals simp_wf
  all_goals first
    | omega
    | (have := readDigits_length_le (e :: rest') 0 0; simp_all)

/-- A double-quoted literal: the string it reads as, and the bytes after it. -/
def luaReadString : List Byte → Option (List Byte × List Byte)
  | q :: rest => if q = ch '"' then luaReadBody (ch '"') rest else none
  | [] => none

/-- Three-digit decimal, so the next byte can never join the escape. -/
def decimal3 (b : Byte) : List Byte :=
  [ch '0' + BitVec.ofNat 8 (b.toNat / 100), ch '0' + BitVec.ofNat 8 (b.toNat / 10 % 10),
    ch '0' + BitVec.ofNat 8 (b.toNat % 10)]

/-- What `lua_string` writes: printable ASCII as is, everything else as `\ddd`. -/
def luaEscapeByte (b : Byte) : List Byte :=
  if ch ' ' ≤ b ∧ b ≤ ch '~' ∧ b ≠ ch '"' ∧ b ≠ ch '\\' then [b] else ch '\\' :: decimal3 b

def luaLiteral (s : List Byte) : List Byte := ch '"' :: s.flatMap luaEscapeByte ++ [ch '"']

end Protocol.Spec
