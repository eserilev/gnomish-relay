import Protocol.Code.Funs
import Protocol.Spec.Policy

/-! # Permissions (S6) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Policy

@[step]
theorem rank_spec (level : policy.Level) : policy.rank level ⦃ r => r.val = rank level ⦄ := by
  induction level <;> simp [policy.rank, rank]

theorem effective_level_spec (config requested : policy.Level) :
    policy.effective_level config requested ⦃ e => rank e = min (rank config) (rank requested) ⦄ := by
  unfold policy.effective_level
  step*

theorem answer_from_game_spec (a : policy.Answer) :
    policy.answer_from_game a ⦃ b => b ≠ .AllowAlways ∧ (a ≠ .AllowAlways → b = a) ⦄ := by
  induction a <;> simp [policy.answer_from_game]

end Protocol.Policy
