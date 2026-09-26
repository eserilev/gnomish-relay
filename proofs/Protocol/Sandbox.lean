import Protocol.PathRules
import Protocol.Spec.Sandbox

/-! # The sandbox policy (S31) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.PathRules

namespace Protocol.Sandbox

@[step]
theorem copy_all_spec (list : Slice (alloc.vec.Vec U8)) :
    sandbox.copy_all list ⦃ v => strs v.val = strs list.val ⦄ := by
  unfold sandbox.copy_all sandbox.copy_all_loop
  apply loop.spec_decr_nat (fun st => list.val.length - st.2.val)
    (fun st => st.2.val ≤ list.val.length ∧ st.1.val.length = st.2.val ∧
      strs st.1.val = strs (list.val.take st.2.val)) _ _ _ _
    ⟨by simp, by simp, by simp [strs]⟩
  rintro ⟨out, i⟩ ⟨hi, hlen, hout⟩
  simp only at hi hlen hout
  unfold sandbox.copy_all_loop.body
  step*
  · have hlt : i.val < list.val.length := by scalar_tac
    refine ⟨by scalar_tac, by simp [out1_post, hlen, i2_post], ?_, by scalar_tac⟩
    rw [out1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [strs, List.map_append, List.map_cons, List.map_nil, Option.toList_some] at hout ⊢
    rw [hout, v1_post, v_post]
    simp
  · have : i.val = list.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact hout

@[step]
theorem is_hidden_spec (policy : sandbox.SandboxPolicy) (path : Slice U8)
    (hroom : path.val.length < Usize.max) :
    sandbox.is_hidden policy path ⦃ r => (r = true ↔ hiddenBy policy (bytes path.val)) ⦄ := by
  unfold sandbox.is_hidden
  step*
  all_goals simp only [deref_val] at *
  · simp only [true_iff, hiddenBy]
    exact Or.inl (b_post.mp (by assumption))
  · simp only [true_iff, hiddenBy]
    exact Or.inr (Or.inl (b1_post.mp (by assumption)))
  · simp only [hiddenBy]
    rw [← b_post, ← b1_post, ← r_post]
    simp_all

/-- A path that the sandbox can make writable: clean, at most 1 MiB, and not hidden. -/
def Writable (policy : sandbox.SandboxPolicy) (p : List Spec.Byte) : Prop :=
  p.length ≤ 2 ^ 20 ∧ cleanPath p ∧ ¬ hiddenBy policy p

@[step]
theorem can_write_spec (policy : sandbox.SandboxPolicy) (path : Slice U8) :
    sandbox.can_write policy path ⦃ r => (r = true ↔ Writable policy (bytes path.val)) ⦄ := by
  unfold sandbox.can_write
  step as ⟨b, hb⟩
  split
  · have ⟨hmax, hclean⟩ := hb.mp (by assumption)
    step with is_hidden_spec policy path (by scalar_tac) as ⟨b1, hb1⟩
    simp only [Writable, bytes_len]
    rw [← hb1]
    cases b1 <;> simp_all
  · simp only [WP.spec_ok, Bool.false_eq_true, false_iff, Writable, bytes_len]
    intro ⟨hmax, hclean, _⟩
    exact (by assumption : ¬ b = true) (hb.mpr ⟨hmax, hclean⟩)

theorem keep_writable_spec (list : alloc.vec.Vec (alloc.vec.Vec U8))
    (policy : sandbox.SandboxPolicy) (path : Slice U8) (hlen : list.val.length ≤ 1) :
    sandbox.keep_writable list policy path ⦃ v =>
      v.val.length ≤ list.val.length + 1 ∧
      ∀ w ∈ strs v.val, w ∈ strs list.val ∨ (w = bytes path.val ∧ Writable policy w) ⦄ := by
  unfold sandbox.keep_writable
  step as ⟨b, hb⟩
  split
  · have hw := hb.mp (by assumption)
    step as ⟨c, hc⟩
    step as ⟨v, hv⟩
    refine ⟨by simp [hv], ?_⟩
    intro w hw'
    have hw'' : w ∈ strs list.val ∨ w = bytes c.val := by
      simpa [strs, hv] using hw'
    rcases hw'' with h | h
    · exact Or.inl h
    · right
      rw [h, hc]
      exact ⟨rfl, hc ▸ hw⟩
  · simp only [WP.spec_ok]
    exact ⟨by simp, fun w hw => Or.inl hw⟩

/-- **S31.** -/
theorem sandbox_policy_spec (chat temp : Slice U8)
    (deny desktop writesOnly : Slice (alloc.vec.Vec U8)) :
    sandbox.sandbox_policy chat temp deny desktop writesOnly ⦃ pol =>
      (∀ p, (∃ f ∈ strs deny.val, insideCI f p) ∨ matchesPattern (strs desktop.val) p ∨
          matchesPattern (strs writesOnly.val) p → hiddenBy pol p) ∧
      (∀ w ∈ strs pol.writable.val, cleanPath w ∧ ¬ hiddenBy pol w) ∧
      (∀ w ∈ strs pol.writable.val, w = bytes chat.val ∨ w = bytes temp.val) ∧
      pol.network = .Off ⦄ := by
  unfold sandbox.sandbox_policy
  step as ⟨v, hv⟩
  step as ⟨v1, hv1⟩
  step as ⟨v2, hv2⟩
  step with keep_writable_spec (alloc.vec.Vec.new (alloc.vec.Vec U8)) _ chat (by simp) as ⟨w1, hw1len, hw1⟩
  step with keep_writable_spec w1 _ temp (by simp at hw1len; omega) as ⟨w2, _, hw2⟩
  refine ⟨?_, ?_, ?_⟩
  · intro p hp
    simp only [hiddenBy, hv, hv1, hv2]
    exact hp
  · intro w hw
    rcases hw2 w hw with h | ⟨_, hwr⟩
    · rcases hw1 w (by simpa [strs] using h) with h' | ⟨_, hwr'⟩
      · simp [strs] at h'
      · exact ⟨hwr'.2.1, by simpa [hiddenBy] using hwr'.2.2⟩
    · exact ⟨hwr.2.1, by simpa [hiddenBy] using hwr.2.2⟩
  · intro w hw
    rcases hw2 w hw with h | ⟨he, _⟩
    · rcases hw1 w (by simpa [strs] using h) with h' | ⟨he', _⟩
      · simp [strs] at h'
      · exact Or.inl he'
    · exact Or.inr he

end Protocol.Sandbox
