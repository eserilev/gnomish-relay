import Protocol.Spec.Lua
import Protocol.Spec.Apps
import Protocol.Code.Funs

/-!
# The live file

```lua
GnomishRelay_Live = {progress = {
{chat = "c1", id = 12, lines = {"edit src/main.rs", "$ cargo test", }},
}, permissions = {
{request = "p7", chat = "c1", id = 12, text = "...", options = {
{id = "o1", kind = "allow_once", label = "Allow"},
}},
}}
```

Every string in it is a `luaLiteral`, every number is `decimal`, and every other
byte is fixed, as in the slot body. So an agent cannot change the shape of the table.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def maxProgress : Nat := 30
def maxLines : Nat := 5
def maxLine : Nat := 200
def maxRequests : Nat := 4
def maxOptions : Nat := 4
def maxPopup : Nat := 2000
def maxLabel : Nat := 64
def liveLimit : Nat := 256 * 1024

def kindWord : live.OptionKind → String
  | .AllowOnce => "allow_once"
  | .AllowAlways => "allow_always"
  | .RejectOnce => "reject_once"
  | .RejectAlways => "reject_always"

def lineItem (l : alloc.vec.Vec U8) : List Byte :=
  luaLiteral (bytes l.val) ++ ascii ", "

def progressLine (p : live.Progress) : List Byte :=
  ascii "{chat = " ++ luaLiteral (bytes p.chat.val) ++ ascii ", id = " ++ decimal p.id.val ++
    ascii ", lines = {" ++ p.lines.val.flatMap lineItem ++ ascii "}},\n"

def optionLine (o : live.PermOption) : List Byte :=
  ascii "{id = " ++ luaLiteral (bytes o.id.val) ++ ascii ", kind = " ++
    luaLiteral (ascii (kindWord o.kind)) ++ ascii ", label = " ++ luaLiteral (bytes o.label.val) ++
    ascii "},\n"

def requestLine (r : live.Request) : List Byte :=
  ascii "{request = " ++ luaLiteral (bytes r.request.val) ++ ascii ", chat = " ++
    luaLiteral (bytes r.chat.val) ++ ascii ", id = " ++ decimal r.id.val ++ ascii ", text = " ++
    luaLiteral (bytes r.text.val) ++ ascii ", options = {\n" ++ r.options.val.flatMap optionLine ++
    ascii "}},\n"

/-- The global name belongs to the app (`liveGlobal`). -/
def liveOf (app : apps.App) (progress : List live.Progress) (requests : List live.Request) :
    List Byte :=
  ascii (liveGlobal app) ++ ascii " = {progress = {\n" ++ progress.flatMap progressLine ++
    ascii "}, permissions = {\n" ++ requests.flatMap requestLine ++ ascii "}}\n"

/-- The live file of the relay app, which S21 bounds. -/
def liveBytes (progress : List live.Progress) (requests : List live.Request) : List Byte :=
  liveOf .Relay progress requests

def fitsProgress (p : live.Progress) : Prop :=
  p.chat.val.length ≤ 32 ∧ p.lines.val.length ≤ maxLines ∧ ∀ l ∈ p.lines.val, l.val.length ≤ maxLine

def fitsOption (o : live.PermOption) : Prop :=
  o.id.val.length ≤ 32 ∧ o.label.val.length ≤ maxLabel

def fitsRequest (r : live.Request) : Prop :=
  r.request.val.length ≤ 32 ∧ r.chat.val.length ≤ 32 ∧ r.text.val.length ≤ maxPopup ∧
    r.options.val.length ≤ maxOptions ∧ ∀ o ∈ r.options.val, fitsOption o

/-- What `prepare_progress` and `prepare_requests` guarantee. -/
def fitsLive (progress : List live.Progress) (requests : List live.Request) : Prop :=
  progress.length ≤ maxProgress ∧ (∀ p ∈ progress, fitsProgress p) ∧
    requests.length ≤ maxRequests ∧ ∀ r ∈ requests, fitsRequest r

/-- It keeps the id and the last lines of an entry, and cuts only the ends of strings. -/
def progressFrom (orig prepared : live.Progress) : Prop :=
  prepared.id = orig.id ∧ bytes prepared.chat.val = (bytes orig.chat.val).take 32 ∧
    List.Forall₂ (fun o p => bytes p.val = (bytes o.val).take maxLine)
      (orig.lines.val.drop (orig.lines.val.length - maxLines)) prepared.lines.val

def optionFrom (orig prepared : live.PermOption) : Prop :=
  prepared.kind = orig.kind ∧ bytes prepared.id.val = (bytes orig.id.val).take 32 ∧
    bytes prepared.label.val = (bytes orig.label.val).take maxLabel

/-- It keeps the id and the first options of a request, and cuts only the ends of strings. -/
def requestFrom (orig prepared : live.Request) : Prop :=
  prepared.id = orig.id ∧ bytes prepared.request.val = (bytes orig.request.val).take 32 ∧
    bytes prepared.chat.val = (bytes orig.chat.val).take 32 ∧
    bytes prepared.text.val = (bytes orig.text.val).take maxPopup ∧
    List.Forall₂ optionFrom (orig.options.val.take maxOptions) prepared.options.val

end Protocol.Spec
