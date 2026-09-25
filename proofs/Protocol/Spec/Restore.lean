import Protocol.Spec.Lua
import Protocol.Spec.Apps
import Protocol.Code.Funs

/-!
# The restore bundle

```lua
GnomishRelay_Restore = {token = "tok", chats = {
{id = "c1", name = "lighthouse", agent = "claude", cwd = "Code/x", history = {
{role = "user", id = 5, text = "..."},
}},
}}
```

Every string in it is a `luaLiteral`, every number is `decimal`, and every other
byte is fixed, as in the slot body. So a chat cannot change the shape of the table.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def maxChats : Nat := 16
def maxHistory : Nat := 10
def maxName : Nat := 64
def maxCwd : Nat := 1024
def maxEntryText : Nat := 500
def restoreLimit : Nat := 512 * 1024

def roleWord : restore.Role → String
  | .User => "user"
  | .Agent => "agent"
  | .Error => "error"

def entryLine (e : restore.Entry) : List Byte :=
  ascii "{role = " ++ luaLiteral (ascii (roleWord e.role)) ++ ascii ", id = " ++ decimal e.id.val ++
    ascii ", text = " ++ luaLiteral (bytes e.text.val) ++ ascii "},\n"

def chatLine (c : restore.Chat) : List Byte :=
  ascii "{id = " ++ luaLiteral (bytes c.id.val) ++ ascii ", name = " ++ luaLiteral (bytes c.name.val) ++
    ascii ", agent = " ++ luaLiteral (bytes c.agent.val) ++ ascii ", cwd = " ++
    luaLiteral (bytes c.cwd.val) ++ ascii ", history = {\n" ++ c.history.val.flatMap entryLine ++
    ascii "}},\n"

/-- The global name belongs to the app (`restoreGlobal`). -/
def restoreOf (app : apps.App) (token : List Byte) (chats : List restore.Chat) : List Byte :=
  ascii (restoreGlobal app) ++ ascii " = {token = " ++ luaLiteral token ++ ascii ", chats = {\n" ++
    chats.flatMap chatLine ++ ascii "}}\n"

/-- The restore file of the relay app, which S19 bounds. -/
def restoreBytes (token : List Byte) (chats : List restore.Chat) : List Byte :=
  restoreOf .Relay token chats

def fitsChat (c : restore.Chat) : Prop :=
  c.id.val.length ≤ 32 ∧ c.name.val.length ≤ maxName ∧ c.agent.val.length ≤ 32 ∧
    c.cwd.val.length ≤ maxCwd ∧ c.history.val.length ≤ maxHistory ∧
    ∀ e ∈ c.history.val, e.text.val.length ≤ maxEntryText

/-- What `prepare_restore` guarantees, plus a token that is a valid id. -/
def fitsRestore (token : List Byte) (chats : List restore.Chat) : Prop :=
  token.length ≤ 32 ∧ chats.length ≤ maxChats ∧ ∀ c ∈ chats, fitsChat c

/-- `prepare_restore` keeps an entry's role and id, and cuts only the end of its text. -/
def entryFrom (orig prepared : restore.Entry) : Prop :=
  prepared.role = orig.role ∧ prepared.id = orig.id ∧
    bytes prepared.text.val = (bytes orig.text.val).take maxEntryText

/-- It keeps the last messages of a chat, and cuts only the ends of its strings. -/
def chatFrom (orig prepared : restore.Chat) : Prop :=
  bytes prepared.id.val = (bytes orig.id.val).take 32 ∧
    bytes prepared.name.val = (bytes orig.name.val).take maxName ∧
    bytes prepared.agent.val = (bytes orig.agent.val).take 32 ∧
    bytes prepared.cwd.val = (bytes orig.cwd.val).take maxCwd ∧
    List.Forall₂ entryFrom (orig.history.val.drop (orig.history.val.length - maxHistory))
      prepared.history.val

end Protocol.Spec
