import Protocol.Spec.Bytes
import Protocol.Code.Funs

/-!
# The global names of each app

Each file of an app sets one Lua global, and the name depends on the app
(SPEC.md 9.7, decision 5). So one app never overwrites a value that the other app
is about to read.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def slotGlobal : apps.App → String
  | .Relay => "GnomishRelay_SlotData"
  | .Timeways => "Timeways_SlotData"

def restoreGlobal : apps.App → String
  | .Relay => "GnomishRelay_Restore"
  | .Timeways => "Timeways_Restore"

def liveGlobal : apps.App → String
  | .Relay => "GnomishRelay_Live"
  | .Timeways => "Timeways_Live"

end Protocol.Spec
