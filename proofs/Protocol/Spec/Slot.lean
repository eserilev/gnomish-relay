import Protocol.Spec.Lua
import Protocol.Code.Funs

/-!
# The slot body

```lua
GnomishRelay_SlotData = {proto = 1, now = 1790211079, replies = {
{chat = "c1", id = 12, status = "done", text = "..."},
}}
```

Every string in it is a `luaLiteral`, every number is `decimal`, and every other
byte is fixed. So a reply cannot change the shape of the table.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def maxReplies : Nat := 30
def maxText : Nat := 32 * 1024
def slotBodyLimit : Nat := 1024 * 1024

def statusWord : slot.Status → String
  | .Working => "working"
  | .Done => "done"
  | .Error => "error"

def replyLine (r : slot.Reply) : List Byte :=
  ascii "{chat = " ++ luaLiteral (bytes r.chat.val) ++ ascii ", id = " ++ decimal r.id.val ++
    ascii ", status = " ++ luaLiteral (ascii (statusWord r.status)) ++ ascii ", text = " ++
    luaLiteral (bytes r.text.val) ++ ascii "},\n"

def slotBodyBytes (now : Nat) (replies : List slot.Reply) : List Byte :=
  ascii "GnomishRelay_SlotData = {proto = 1, now = " ++ decimal now ++
    ascii ", replies = {\n" ++ replies.flatMap replyLine ++ ascii "}}\n"

/-- What `prepare_replies` guarantees. -/
def fitsSlot (replies : List slot.Reply) : Prop :=
  replies.length ≤ maxReplies ∧
    ∀ r ∈ replies, r.chat.val.length ≤ 32 ∧ (luaLiteral (bytes r.text.val)).length ≤ maxText

/-- `prepare_replies` keeps a reply's chat, id, and status, and cuts only the end of its text. -/
def preparedFrom (orig prepared : slot.Reply) : Prop :=
  prepared.id = orig.id ∧ prepared.status = orig.status ∧
    bytes prepared.chat.val = (bytes orig.chat.val).take 32 ∧
    bytes prepared.text.val <+: bytes orig.text.val

end Protocol.Spec
