import Protocol.Spec.Bytes
import Protocol.Code.Funs

/-!
# The terminal sessions and their notices

The bridge keeps at most 32 sessions. Each has an id and at most one notice. An event
of a session changes only that session. A new session in a full table takes the place of
a session with no notice; only when every session has a notice, the session with the
oldest notice loses it.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def maxSessions : Nat := 32

def sessionId (s : sessions.Session) : List Byte := bytes s.id.val

/-- The ids are distinct, so each session has at most one notice. -/
def oneNoticeEach (table : List sessions.Session) : Prop := (table.map sessionId).Nodup

def sessionsFit (table : List sessions.Session) : Prop := table.length ≤ maxSessions ∧ oneNoticeEach table

def noticeKindOf : sessions.EventKind → Option live.NoticeKind
  | .Waiting => some .Waiting
  | .Finished => some .Finished
  | .Failed => some .Failed
  | _ => none

/-- The notice of the event: its id, kind, source, repo, and text. -/
def noticeOfEvent (e : sessions.Event) (k : live.NoticeKind) (n : live.Notice) : Prop :=
  n.id = e.id ∧ n.kind = k ∧ n.source = e.source ∧ n.repo.val = e.repo.val ∧ n.text.val = e.text.val

/-- After a `waiting`, `finished`, or `failed` event, its session holds exactly the notice of
the event. After a start, its session holds no notice. After an end, the session is gone. -/
def noticeAfter (table : List sessions.Session) (e : sessions.Event) : Prop :=
  match e.kind with
  | .SessionEnd => ∀ s ∈ table, sessionId s ≠ bytes e.session.val
  | kind =>
    ∃ s ∈ table, sessionId s = bytes e.session.val ∧
      match noticeKindOf kind with
      | some k => ∃ n, s.notice = some n ∧ noticeOfEvent e k n
      | none => s.notice = none

def noticeTime (s : sessions.Session) : Nat :=
  match s.notice with
  | some n => n.at.val
  | none => 0

def others (table : List sessions.Session) (e : sessions.Event) : List sessions.Session :=
  table.filter (fun s => sessionId s ≠ bytes e.session.val)

/-- No other session changes or comes new. A notice of another session goes only in a full
table where every session has a notice, and then only the oldest one, and only one session. -/
def othersKept (before after : List sessions.Session) (e : sessions.Event) : Prop :=
  (∀ s ∈ after, sessionId s ≠ bytes e.session.val → s ∈ before) ∧
  (∀ s ∈ before, sessionId s ≠ bytes e.session.val → s.notice ≠ none → s ∈ after ∨
    (before.length = maxSessions ∧ (∀ t ∈ before, t.notice ≠ none) ∧
      ∀ t ∈ before, noticeTime s ≤ noticeTime t)) ∧
  (others before e).length ≤ (others after e).length + 1

end Protocol.Spec
