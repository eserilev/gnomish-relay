import Protocol.Code.Funs

/-! # The clock check and the ack of the link to a server (S42) -/

open Aeneas Aeneas.Std Result protocol

namespace Protocol.Link

@[simp, scalar_tac_simps]
theorem max_skew_val : link.MAX_SKEW_SECONDS.val = 60 := by
  unfold link.MAX_SKEW_SECONDS; rfl

theorem clocks_agree_spec (a b : U32) :
    link.clocks_agree a b ⦃ r => (r = true ↔ a.val ≤ b.val + 60 ∧ b.val ≤ a.val + 60) ⦄ := by
  unfold link.clocks_agree
  step*

/-- Every number in the front `n` of the outbox is at most the ack. -/
def AllAcked (outbox : List U32) (ack : U32) (n : Nat) : Prop :=
  ∀ x ∈ outbox.take n, x.val ≤ ack.val

theorem acked_count_loop_spec (outbox : Slice U32) (ack : U32) (i : Usize)
    (hi : i.val ≤ outbox.val.length) (hpre : AllAcked outbox.val ack i.val) :
    link.acked_count_loop outbox ack i ⦃ n =>
      n.val ≤ outbox.val.length ∧ AllAcked outbox.val ack n.val ∧
      (∀ x, outbox.val[n.val]? = some x → ack.val < x.val) ⦄ := by
  unfold link.acked_count_loop
  apply loop.spec_decr_nat (fun st => outbox.val.length - st.val)
    (fun st => st.val ≤ outbox.val.length ∧ AllAcked outbox.val ack st.val)
    _ _ _ _ ⟨hi, hpre⟩
  rintro i ⟨hi, hpre⟩
  unfold link.acked_count_loop.body
  step*
  · have hlt : i.val < outbox.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    intro x hx
    rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt] at hx
    simp only [Option.toList_some, List.mem_append, List.mem_singleton] at hx
    rcases hx with hx | hx
    · exact hpre x hx
    · rw [hx, ← i2_post]; scalar_tac
  · have hlt : i.val < outbox.val.length := by scalar_tac
    refine ⟨hi, hpre, ?_⟩
    intro x hx
    rw [List.getElem?_eq_getElem hlt] at hx
    simp only [Option.some.injEq] at hx
    rw [← hx, ← i2_post]; scalar_tac
  · refine ⟨hi, hpre, ?_⟩
    intro x hx
    have : outbox.val.length ≤ i.val := by scalar_tac
    simp [List.getElem?_eq_none this] at hx

theorem acked_count_spec (outbox : Slice U32) (ack : U32) :
    link.acked_count outbox ack ⦃ n =>
      n.val ≤ outbox.val.length ∧ AllAcked outbox.val ack n.val ∧
      (∀ x, outbox.val[n.val]? = some x → ack.val < x.val) ⦄ := by
  unfold link.acked_count
  exact acked_count_loop_spec outbox ack 0#usize (by simp) (by simp [AllAcked])

end Protocol.Link
