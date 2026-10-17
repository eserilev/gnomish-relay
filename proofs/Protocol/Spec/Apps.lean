import Protocol.Spec.Bytes
import Protocol.Code.Funs

/-!
# The app of a strip, and the global names of each app

A strip goes to the one app whose key verifies its tag (SPEC.md 9.7, decision 2).

Each file of an app sets one Lua global, and the name depends on the app
(SPEC.md 9.7, decision 5). So one app never overwrites a value that the other app
is about to read.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

/-- The apps whose key verifies the tag, in the order of the checks. -/
def verifiedApps (checks : List apps.TagCheck) : List apps.App :=
  checks.filterMap fun c => if c.tag_ok then some c.app else none

def slotGlobal : apps.App → String
  | .Relay => "GnomishRelay_SlotData"
  | .Timeways => "Timeways_SlotData"
  | .Workshop => "GnomishWorkshop_SlotData"

def restoreGlobal : apps.App → String
  | .Relay => "GnomishRelay_Restore"
  | .Timeways => "Timeways_Restore"
  | .Workshop => "GnomishWorkshop_Restore"

def liveGlobal : apps.App → String
  | .Relay => "GnomishRelay_Live"
  | .Timeways => "Timeways_Live"
  | .Workshop => "GnomishWorkshop_Live"

end Protocol.Spec
