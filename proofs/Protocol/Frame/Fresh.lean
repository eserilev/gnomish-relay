import Protocol.Code.Funs
import Protocol.Spec.Frame

/-! # Freshness of a frame (S11) and the frame check (S2 + S11) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Frame

@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_age_val : frame.MAX_AGE.val = 300 := by unfold frame.MAX_AGE; rfl

@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_ahead_val : frame.MAX_AHEAD.val = 60 := by unfold frame.MAX_AHEAD; rfl

@[step]
theorem is_stale_spec (frameTime now : U32) :
    frame.is_stale frameTime now ⦃ b => (b = true ↔ frameTime.val + 300 < now.val) ⦄ := by
  unfold frame.is_stale
  step*

@[step]
theorem is_ahead_spec (frameTime now : U32) :
    frame.is_ahead frameTime now ⦃ b => (b = true ↔ now.val + 60 < frameTime.val) ⦄ := by
  unfold frame.is_ahead
  step*

theorem is_fresh_spec (frameTime now : U32) :
    frame.is_fresh frameTime now ⦃ b => (b = true ↔ fresh frameTime.val now.val) ⦄ := by
  unfold frame.is_fresh
  step*
  all_goals simp_all [fresh]

theorem check_frame_spec (frameTime : U32) (tagOk : Bool) (now : U32) :
    frame.check_frame frameTime tagOk now ⦃ r =>
      (r = .Ok () ↔ (tagOk = true ∧ fresh frameTime.val now.val)) ⦄ := by
  unfold frame.check_frame
  step*
  all_goals simp_all [fresh]

end Protocol.Frame
