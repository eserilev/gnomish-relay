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
}, notices = {busy = 1, open = 2, list = {
{id = 7, at = 1790300100, source = "claude", kind = "waiting", repo = "x", took = 0, text = "..."},
}}}
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
def maxNotices : Nat := 20
def maxNoticeRepo : Nat := 64
def maxNoticeText : Nat := 600
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

def sourceWord : live.Source → String
  | .Claude => "claude"
  | .Codex => "codex"

def noticeKindWord : live.NoticeKind → String
  | .Waiting => "waiting"
  | .Finished => "finished"
  | .Failed => "failed"

def noticeLine (n : live.Notice) : List Byte :=
  ascii "{id = " ++ decimal n.id.val ++ ascii ", at = " ++ decimal n.at.val ++ ascii ", source = " ++
    luaLiteral (ascii (sourceWord n.source)) ++ ascii ", kind = " ++
    luaLiteral (ascii (noticeKindWord n.kind)) ++ ascii ", repo = " ++ luaLiteral (bytes n.repo.val) ++
    ascii ", took = " ++ decimal n.took.val ++ ascii ", text = " ++ luaLiteral (bytes n.text.val) ++
    ascii "},\n"

def noticesPart (ns : live.Notices) : List Byte :=
  ascii "}, notices = {busy = " ++ decimal ns.busy.val ++ ascii ", open = " ++ decimal ns.open.val ++
    ascii ", list = {\n" ++ ns.list.val.flatMap noticeLine

/-- The global name belongs to the app (`liveGlobal`). -/
def liveOf (app : apps.App) (progress : List live.Progress) (requests : List live.Request)
    (notices : live.Notices) : List Byte :=
  ascii (liveGlobal app) ++ ascii " = {progress = {\n" ++ progress.flatMap progressLine ++
    ascii "}, permissions = {\n" ++ requests.flatMap requestLine ++ noticesPart notices ++ ascii "}}}\n"

/-- The live file of the relay app, which S21 bounds. -/
def liveBytes (progress : List live.Progress) (requests : List live.Request) (notices : live.Notices) :
    List Byte :=
  liveOf .Relay progress requests notices

def fitsProgress (p : live.Progress) : Prop :=
  p.chat.val.length ≤ 32 ∧ p.lines.val.length ≤ maxLines ∧ ∀ l ∈ p.lines.val, l.val.length ≤ maxLine

def fitsOption (o : live.PermOption) : Prop :=
  o.id.val.length ≤ 32 ∧ o.label.val.length ≤ maxLabel

def fitsRequest (r : live.Request) : Prop :=
  r.request.val.length ≤ 32 ∧ r.chat.val.length ≤ 32 ∧ r.text.val.length ≤ maxPopup ∧
    r.options.val.length ≤ maxOptions ∧ ∀ o ∈ r.options.val, fitsOption o

def fitsNotice (n : live.Notice) : Prop :=
  n.repo.val.length ≤ maxNoticeRepo ∧ n.text.val.length ≤ maxNoticeText

/-- What `prepare_progress`, `prepare_requests`, and `prepare_notices` guarantee. -/
def fitsLive (progress : List live.Progress) (requests : List live.Request) (notices : live.Notices) :
    Prop :=
  progress.length ≤ maxProgress ∧ (∀ p ∈ progress, fitsProgress p) ∧
    requests.length ≤ maxRequests ∧ (∀ r ∈ requests, fitsRequest r) ∧
    notices.list.val.length ≤ maxNotices ∧ ∀ n ∈ notices.list.val, fitsNotice n

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

/-- It keeps every number and word of a notice, and cuts only the ends of strings. -/
def noticeFrom (orig prepared : live.Notice) : Prop :=
  prepared.id = orig.id ∧ prepared.at = orig.at ∧ prepared.source = orig.source ∧
    prepared.kind = orig.kind ∧ prepared.took = orig.took ∧
    bytes prepared.repo.val = (bytes orig.repo.val).take maxNoticeRepo ∧
    bytes prepared.text.val = (bytes orig.text.val).take maxNoticeText

end Protocol.Spec
