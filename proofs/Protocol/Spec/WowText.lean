import Protocol.Spec.Bytes

/-!
# How the WoW chat frame reads `|`

`||` shows one `|`. Any other `|` starts an escape code: a link (`|H`), a color
(`|c`), a texture (`|T`), and more. This model treats every such `|` as unsafe.
-/

namespace Protocol.Spec

def pipe : Byte := ch '|'

/-- The plain text that WoW shows, or `none` if the text has an escape code. -/
def wowPlain : List Byte → Option (List Byte)
  | [] => some []
  | c :: rest =>
    if c = pipe then
      match rest with
      | d :: rest' => if d = pipe then (pipe :: ·) <$> wowPlain rest' else none
      | [] => none
    else (c :: ·) <$> wowPlain rest

end Protocol.Spec
