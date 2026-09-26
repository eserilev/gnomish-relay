import Protocol.Code.Funs

/-! # The version range of each app (S30)

The proof needs `oldest ≤ newest` for each app: a version below the range is then never
above it too. A new range needs only new values in the four facts below. -/

open Aeneas Aeneas.Std Result protocol

namespace Protocol.Version

@[simp] theorem relay_oldest_val : version.RELAY_OLDEST.val = 1 := by
  unfold version.RELAY_OLDEST; rfl

@[simp] theorem relay_newest_val : version.RELAY_NEWEST.val = 1 := by
  unfold version.RELAY_NEWEST; rfl

@[simp] theorem timeways_oldest_val : version.TIMEWAYS_OLDEST.val = 1 := by
  unfold version.TIMEWAYS_OLDEST; rfl

@[simp] theorem timeways_newest_val : version.TIMEWAYS_NEWEST.val = 1 := by
  unfold version.TIMEWAYS_NEWEST; rfl

theorem version_fit_spec (app : apps.App) (v : U32) :
    version.version_fit app v ⦃ r => ∃ lo hi : U32,
      version.oldest app = ok lo ∧ version.newest app = ok hi ∧
      (r = .Supported ↔ lo.val ≤ v.val ∧ v.val ≤ hi.val) ∧
      (r = .TooOld ↔ v.val < lo.val) ∧
      (r = .TooNew ↔ hi.val < v.val) ⦄ := by
  induction app <;>
  · simp only [version.version_fit, version.oldest, version.newest, bind_tc_ok]
    split <;> (try split) <;> (simp_all; try scalar_tac)

end Protocol.Version
