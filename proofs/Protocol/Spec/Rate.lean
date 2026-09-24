import Protocol.Spec.Bytes

/-!
# The rate limit

A sliding window: a message is admitted only if fewer than 10 messages were
admitted in the 60 seconds before it.
-/

namespace Protocol.Spec

def maxMessages : Nat := 10
def windowSeconds : Nat := 60
def maxQueue : Nat := 20

/-- Admitted times that still count at `now`. -/
def inWindow (now : Nat) (times : List Nat) : List Nat :=
  times.filter fun t => t ≤ now ∧ now < t + windowSeconds

/-- One decision: admit or not, and the new list of times. -/
def admitSpec (times : List Nat) (now : Nat) : Bool × List Nat :=
  let kept := inWindow now times
  if kept.length < maxMessages then (true, kept ++ [now]) else (false, kept)

/-- All decisions for a sequence of message times, from an empty limiter. -/
def decisions (nows : List Nat) : List Bool :=
  (nows.foldl (fun (acc : List Bool × List Nat) now =>
    let (ok, times) := admitSpec acc.2 now
    (acc.1 ++ [ok], times)) ([], [])).1

/-- The times of the admitted messages. -/
def admittedTimes (nows : List Nat) : List Nat :=
  ((nows.zip (decisions nows)).filter (·.2)).map (·.1)

end Protocol.Spec
