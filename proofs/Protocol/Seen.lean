import Protocol.Ascii
import Protocol.Spec.Seen

/-! # Replay protection (S7) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Seen

@[simp, scalar_tac_simps, grind =, agrind =]
theorem capacity_val : seen.SEEN_CAPACITY.val = 1000 := by unfold seen.SEEN_CAPACITY; rfl

theorem bytes_eq_iff (a b : List U8) : bytes a = bytes b ↔ a = b := by
  constructor
  · intro h; exact List.map_injective_iff.mpr (fun x y h => (UScalar.eq_equiv_bv_eq x y).mpr h) h
  · intro h; rw [h]

theorem bytes_equal_loop_spec (a b : Slice U8) (i : Usize) (hlen : a.val.length = b.val.length)
    (hi : i.val ≤ a.val.length) (hpre : a.val.take i.val = b.val.take i.val) :
    seen.bytes_equal_loop a b i ⦃ r => (r = true ↔ a.val = b.val) ⦄ := by
  unfold seen.bytes_equal_loop
  apply loop.spec_decr_nat (fun i => a.val.length - i.val)
    (fun i => i.val ≤ a.val.length ∧ a.val.take i.val = b.val.take i.val) _ _ _ _ ⟨hi, hpre⟩
  rintro i ⟨hi, hpre⟩
  unfold seen.bytes_equal_loop.body
  step*
  · -- Same byte: the equal prefix grows by one.
    have hlt : i.val < a.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have heq : ¬(i2 != i3) = true := by assumption
    simp only [bne_iff_ne, ne_eq, not_not] at heq
    rw [i4_post, List.take_add_one, List.take_add_one, hpre, List.getElem?_eq_getElem hlt,
      List.getElem?_eq_getElem (by omega), ← i2_post, ← i3_post, heq]
  · -- Every byte matched.
    simp only [true_iff]
    have : i.val = a.val.length := by scalar_tac
    rw [this, List.take_length, hlen, List.take_length] at hpre
    exact hpre

@[step]
theorem bytes_equal_spec (a b : Slice U8) :
    seen.bytes_equal a b ⦃ r => (r = true ↔ a.val = b.val) ⦄ := by
  unfold seen.bytes_equal
  step*
  apply bytes_equal_loop_spec a b 0#usize (by scalar_tac) (by simp) (by simp)

@[simp] theorem deref_val {α : Type} (v : alloc.vec.Vec α) : (alloc.vec.Vec.deref v).val = v.val := rfl

def keyOf (e : seen.Entry) : List Spec.Byte × Nat := (bytes e.token.val, e.id.val)

theorem keyOf_eq_iff (e : seen.Entry) (token : List U8) (id : U32) :
    keyOf e = (bytes token, id.val) ↔ e.token.val = token ∧ e.id = id := by
  simp only [keyOf, Prod.mk.injEq, bytes_eq_iff]
  constructor
  · rintro ⟨h1, h2⟩; exact ⟨h1, UScalar.eq_of_val_eq h2⟩
  · rintro ⟨h1, rfl⟩; exact ⟨h1, rfl⟩

theorem contains_loop_spec (entries : Slice seen.Entry) (token : Slice U8) (id : U32) (i : Usize)
    (hi : i.val ≤ entries.val.length)
    (hnone : ∀ j (hj : j < i.val), keyOf (entries.val[j]'(by omega)) ≠ (bytes token.val, id.val)) :
    seen.contains_loop entries token id i ⦃ r =>
      (r = true ↔ (bytes token.val, id.val) ∈ entries.val.map keyOf) ⦄ := by
  unfold seen.contains_loop
  apply loop.spec_decr_nat (fun i => entries.val.length - i.val)
    (fun i => ∃ _ : i.val ≤ entries.val.length,
      ∀ j (hj : j < i.val), keyOf (entries.val[j]'(by omega)) ≠ (bytes token.val, id.val))
    _ _ _ _ ⟨hi, hnone⟩
  rintro i ⟨hi, hnone⟩
  unfold seen.contains_loop.body
  step*
  · -- Found it at i.
    have hid : e.id = id := by assumption
    have hb : b = true := by assumption
    simp only [true_iff]
    exact List.mem_map.mpr ⟨e, by rw [e_post]; exact List.getElem_mem _,
      (keyOf_eq_iff e token.val id).mpr ⟨b_post.mp hb, hid⟩⟩
  all_goals try (
    -- Not at i: the invariant holds for i + 1.
    refine ⟨⟨by scalar_tac, fun j hj => ?_⟩, by scalar_tac⟩
    by_cases hji : j < i.val
    · exact hnone j hji
    · have : j = i.val := by scalar_tac
      subst this
      rw [← e_post, ne_eq, keyOf_eq_iff]
      rintro ⟨ht, hid⟩
      first
      | (have hb : ¬ b = true := by assumption
         exact hb (b_post.mpr ht))
      | (have hne : ¬ e.id = id := by assumption
         exact hne hid))
  · -- Checked every entry.
    simp only [Bool.false_eq_true, false_iff]
    intro hm
    obtain ⟨x, hx, hkx⟩ := List.mem_map.mp hm
    obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hx
    exact hnone j (by scalar_tac) hkx

@[step]
theorem contains_spec (entries : Slice seen.Entry) (token : Slice U8) (id : U32) :
    seen.contains entries token id ⦃ r =>
      (r = true ↔ (bytes token.val, id.val) ∈ entries.val.map keyOf) ⦄ := by
  unfold seen.contains
  exact contains_loop_spec entries token id 0#usize (by simp) (fun j hj => absurd hj (by simp))

@[step]
theorem copy_bytes_spec (src : Slice U8) : seen.copy_bytes src ⦃ v => v.val = src.val ⦄ := by
  unfold seen.copy_bytes
  step*
  simp [v_post]

@[step]
theorem first_kept_spec (n : Usize) :
    seen.first_kept n ⦃ r => r.val = (if 1000 ≤ n.val then 1 else 0) ⦄ := by
  unfold seen.first_kept
  step*

def CopyInv (entries : Slice seen.Entry) (from_ : Nat) (st : alloc.vec.Vec seen.Entry × Usize) : Prop :=
  from_ ≤ st.2.val ∧ st.2.val ≤ max from_ entries.val.length ∧
  st.1.val.map keyOf = ((entries.val.map keyOf).drop from_).take (st.2.val - from_)

@[step]
theorem copy_entries_spec (entries : Slice seen.Entry) (from_ : Usize) (hfrom : from_.val ≤ entries.val.length) :
    seen.copy_entries entries from_ ⦃ out =>
      out.val.map keyOf = (entries.val.map keyOf).drop from_.val ⦄ := by
  unfold seen.copy_entries seen.copy_entries_loop
  apply loop.spec_decr_nat (fun st => entries.val.length - st.2.val) (CopyInv entries from_.val)
    _ _ _ _ ⟨le_refl _, le_max_left _ _, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hout⟩
  simp only at hs hi hout
  unfold seen.copy_entries_loop.body
  step*
  · -- Room for one more entry: the copy is shorter than the input.
    have hl := congrArg List.length hout
    simp only [List.length_map, List.length_take, List.length_drop] at hl
    have := entries.property
    scalar_tac
  · have hlt : i.val < entries.val.length := by scalar_tac
    refine ⟨⟨by scalar_tac, by scalar_tac, ?_⟩, by scalar_tac⟩
    rw [out1_post, List.map_append, hout, i2_post]
    rw [show i.val + 1 - from_.val = (i.val - from_.val) + 1 by omega, List.take_add_one,
      List.getElem?_drop, List.getElem?_map, show from_.val + (i.val - from_.val) = i.val by omega,
      List.getElem?_eq_getElem hlt]
    simp [keyOf, v_post, e_post]
  · have : i.val = entries.val.length := by scalar_tac
    rw [hout, this]
    simp

/-- **S7.** -/
theorem admit_spec (history : seen.Seen) (token : Slice U8) (id : U32)
    (hcap : (seenKeys history).length ≤ 1000) (_htok : token.val.length ≤ 2 ^ 16) :
    seen.admit history token id ⦃ res =>
      let key := (bytes token.val, id.val)
      (res.1 = true ↔ key ∉ seenKeys history) ∧
      (res.1 = true → seenKeys res.2 =
        (seenKeys history ++ [key]).drop ((seenKeys history).length + 1 - 1000)) ∧
      (res.1 = false → seenKeys res.2 = seenKeys history) ⦄ := by
  have hk : seenKeys history = history.entries.val.map keyOf := rfl
  simp only [seenKeys, List.length_map] at hcap
  unfold seen.admit
  step*
  · -- A repeat: nothing changes.
    have hb : b = true := by assumption
    refine ⟨?_, fun h => absurd h (by simp), fun _ => trivial⟩
    simp only [Bool.false_eq_true, false_iff, not_not, hk]
    exact b_post.mp hb
  · simp only [deref_val] at *
    split at i1_post <;> scalar_tac
  · -- Room for the new entry.
    have hl := congrArg List.length entries_post
    simp only [List.length_map, List.length_drop, deref_val] at hl
    have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
    omega
  · -- A new message: remembered, and the oldest goes when full.
    have hb : ¬ b = true := by assumption
    have hnot : (bytes token.val, id.val) ∉ seenKeys history := by rw [hk]; exact fun h => hb (b_post.mpr h)
    refine ⟨by simp [hnot], fun _ => ?_, fun h => absurd h (by simp)⟩
    have hkey : keyOf { token := v, id := id } = (bytes token.val, id.val) := by simp [keyOf, v_post]
    have hn : (seenKeys history).length = history.entries.val.length := by simp [seenKeys]
    have hlenv : history.entries.len.val = history.entries.val.length := by simp
    have hi1 : i1.val = (seenKeys history).length + 1 - 1000 := by
      rw [hn]
      split_ifs at i1_post with h <;> omega
    rw [show seenKeys { entries := entries1 } = entries1.val.map keyOf from rfl, entries1_post,
      List.map_append, entries_post, deref_val, ← hk, List.map_singleton, hkey, hi1,
      List.drop_append_of_le_length (by rw [hn]; omega)]

end Protocol.Seen
