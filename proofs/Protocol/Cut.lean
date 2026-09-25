import Protocol.Slot

/-! # Shared cuts: the first bytes of a string, the last items of a list -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Cut

theorem take_min_bytes (l : List U8) (n : Nat) : bytes (l.take (min l.length n)) = (bytes l).take n := by
  rcases le_total l.length n with h | h
  · rw [min_eq_left h, List.take_length, List.take_of_length_le (by simp [bytes, h])]
  · rw [min_eq_right h, bytes, bytes, List.map_take]

@[step]
theorem cut_spec (s : Slice U8) (max : Usize) :
    slot.cut s max ⦃ v => bytes v.val = (bytes s.val).take max.val ∧ v.val.length ≤ max.val ⦄ := by
  unfold slot.cut
  step*
  have hv : v.val = s.val.take (min s.val.length max.val) := by rw [v_post, i1_post]; simp
  exact ⟨by rw [hv]; exact take_min_bytes _ _, by rw [hv]; simp⟩

@[step]
theorem keep_from_spec (len max : Usize) : slot.keep_from len max ⦃ r => r.val = len.val - max.val ⦄ := by
  unfold slot.keep_from
  step*
  scalar_tac

theorem bytes_length (l : List U8) : (bytes l).length = l.length := by simp [bytes]

theorem sum_le {α : Type} (f : α → List Spec.Byte) (k : Nat) (l : List α)
    (h : ∀ a ∈ l, (f a).length ≤ k) : (l.flatMap f).length ≤ k * l.length := by
  rw [List.length_flatMap]
  have : ∀ x ∈ l.map (fun a => (f a).length), x ≤ k := by
    intro x hx
    obtain ⟨a, ha, rfl⟩ := List.mem_map.mp hx
    exact h a ha
  have := List.sum_le_card_nsmul _ _ this
  simpa [mul_comm] using this

end Protocol.Cut
