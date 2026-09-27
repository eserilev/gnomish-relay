import Protocol.Code.Funs
import Protocol.Spec.Ip

/-! # Public addresses (S34) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Ip

/-! ## A table of ranges: pairs of the first and the last value -/

def pairsOf (t : List U32) : List (Nat × Nat) :=
  (List.range (t.length / 2)).map fun k => ((t.getD (2 * k) 0#u32).val, (t.getD (2 * k + 1) 0#u32).val)

def inRangesIdx (t : List U32) (x : Nat) : Prop :=
  ∃ k, 2 * k + 1 < t.length ∧ (t.getD (2 * k) 0#u32).val ≤ x ∧ x ≤ (t.getD (2 * k + 1) 0#u32).val

theorem inRangesIdx_iff (t : List U32) (x : Nat) : inRangesIdx t x ↔ inRanges (pairsOf t) x := by
  unfold inRangesIdx inRanges pairsOf
  simp only [List.mem_map, List.mem_range]
  constructor
  · rintro ⟨k, hk, h1, h2⟩
    exact ⟨_, ⟨k, by omega, rfl⟩, h1, h2⟩
  · rintro ⟨r, ⟨k, hk, rfl⟩, h1, h2⟩
    exact ⟨k, by omega, h1, h2⟩

def InRangesInv (table : Slice U32) (x : U32) (st : Bool × Usize) : Prop :=
  st.2.val % 2 = 0 ∧ st.2.val ≤ table.val.length ∧
  (st.1 = true ↔ ∃ k, 2 * k < st.2.val ∧ 2 * k + 1 < table.val.length ∧
    (table.val.getD (2 * k) 0#u32).val ≤ x.val ∧ x.val ≤ (table.val.getD (2 * k + 1) 0#u32).val)

theorem getD_eq_getElem (t : List U32) (i : Nat) (h : i < t.length) : t.getD i 0#u32 = t[i] := by
  simp [List.getD_eq_getElem?_getD, List.getElem?_eq_getElem h]

@[step]
theorem in_ranges_spec (table : Slice U32) (x : U32) (hlen : table.val.length < Usize.max) :
    ip.in_ranges table x ⦃ r => (r = true ↔ inRangesIdx table.val x.val) ⦄ := by
  unfold ip.in_ranges ip.in_ranges_loop
  apply loop.spec_decr_nat (fun st => table.val.length + 2 - st.2.val) (InRangesInv table x) _ _ _ _
    ⟨by simp, by simp, by simp⟩
  rintro ⟨found, i⟩ ⟨hev, hle, hfound⟩
  simp only at hev hle hfound
  unfold ip.in_ranges_loop.body
  by_cases hf : found = true
  · -- Found in an earlier pair.
    simp only [hf, if_true]
    simp only [WP.spec_ok, true_iff]
    obtain ⟨k, _, hk, h1, h2⟩ := hfound.mp hf
    exact ⟨k, hk, h1, h2⟩
  have hnot := hf
  rw [hfound] at hnot
  simp only [Bool.not_eq_true] at hf
  simp only [hf, Bool.false_eq_true, if_false]
  step as ⟨i1, i1_post⟩
  split
  · rename_i hlt
    have hi1 : i.val + 1 < table.val.length := by scalar_tac
    have e0 : table.val.getD i.val 0#u32 = table.val[i.val] := getD_eq_getElem _ _ (by omega)
    have e1 : table.val.getD (i.val + 1) 0#u32 = table.val[i.val + 1] := getD_eq_getElem _ _ hi1
    step as ⟨i3, i3_post⟩
    -- The pair at `i` holds `x` exactly when the new `found` is true.
    have key : ∀ f : Bool, (f = true ↔ i3.val ≤ x.val ∧ x.val ≤ (table.val[i.val + 1]).val) →
        (i + 2#usize) ⦃ i4 => InRangesInv table x (f, i4) ∧
          table.val.length + 2 - i4.val < table.val.length + 2 - i.val ⦄ := by
      intro f hf'
      step as ⟨i4, i4_post⟩
      simp only [InRangesInv]
      refine ⟨⟨by scalar_tac, by scalar_tac, ?_⟩, by scalar_tac⟩
      rw [hf', i3_post, i4_post]
      constructor
      · intro h
        refine ⟨i.val / 2, by omega, by omega, ?_⟩
        have hk : 2 * (i.val / 2) = i.val := by omega
        rw [hk, e0, e1]
        exact h
      · rintro ⟨k, hk, hk1, h1, h2⟩
        by_cases hlt : 2 * k < i.val
        · exact absurd ⟨k, hlt, hk1, h1, h2⟩ hnot
        · have hk : 2 * k = i.val := by omega
          rw [hk, e0] at h1
          rw [hk, e1] at h2
          exact ⟨h1, h2⟩
    split
    · rename_i hle3
      step as ⟨i4, i4_post⟩
      have := key (decide (x ≤ i4)) (by
        rw [i4_post]; simp only [decide_eq_true_iff]
        constructor
        · intro h; exact ⟨by scalar_tac, by scalar_tac⟩
        · intro h; scalar_tac)
      apply WP.spec_bind this
      intro i4' h
      simp only [WP.spec_ok]
      exact h
    · rename_i hgt
      have := key false (by simp only [Bool.false_eq_true, false_iff]; intro h; scalar_tac)
      apply WP.spec_bind this
      intro i4' h
      simp only [WP.spec_ok]
      exact h
  · -- The loop ran out of pairs.
    rename_i hge
    simp only [WP.spec_ok, Bool.false_eq_true, false_iff]
    rintro ⟨k, hk1, h1, h2⟩
    exact hnot ⟨k, by scalar_tac, hk1, h1, h2⟩

/-! ## The tables -/

theorem v4_table : pairsOf ip.V4_NOT_PUBLIC.val = v4NotPublic := by
  unfold ip.V4_NOT_PUBLIC; decide

/-- Each IPv6 range, by its first 32 bits. -/
def scale (r : Nat × Nat) : Nat × Nat := (r.1 * 2 ^ 96, r.2 * 2 ^ 96 + (2 ^ 96 - 1))

theorem v6_table : (pairsOf ip.V6_NOT_PUBLIC.val).map scale = v6NotPublic := by
  unfold ip.V6_NOT_PUBLIC; decide

theorem v4_table_len : (Array.to_slice ip.V4_NOT_PUBLIC).val.length = 28 := by
  unfold ip.V4_NOT_PUBLIC; rfl

theorem v6_table_len : (Array.to_slice ip.V6_NOT_PUBLIC).val.length = 18 := by
  unfold ip.V6_NOT_PUBLIC; rfl

theorem in_scaled (rs : List (Nat × Nat)) (x : Nat) :
    inRanges (rs.map scale) x ↔ inRanges rs (x / 2 ^ 96) := by
  unfold inRanges
  simp only [List.mem_map]
  have hk : 0 < 2 ^ 96 := by positivity
  constructor
  · rintro ⟨_, ⟨r, hr, rfl⟩, h1, h2⟩
    refine ⟨r, hr, ?_, ?_⟩
    · exact (Nat.le_div_iff_mul_le hk).mpr h1
    · have : x < (r.2 + 1) * 2 ^ 96 := by simp only [scale] at h2; rw [Nat.add_mul]; omega
      have := (Nat.div_lt_iff_lt_mul hk).mpr this
      omega
  · rintro ⟨r, hr, h1, h2⟩
    refine ⟨scale r, ⟨r, hr, rfl⟩, ?_, ?_⟩
    · exact (Nat.le_div_iff_mul_le hk).mp h1
    · have : x / 2 ^ 96 < r.2 + 1 := by omega
      have := (Nat.div_lt_iff_lt_mul hk).mp this
      simp only [scale]; rw [Nat.add_mul] at this; omega

@[step]
theorem is_public_value_spec (v : U32) :
    ip.is_public_value v ⦃ r => (r = true ↔ ¬ inRanges v4NotPublic v.val) ⦄ := by
  unfold ip.is_public_value
  step*
  have hs : s.val = ip.V4_NOT_PUBLIC.val := by rw [s_post]; rfl
  rw [inRangesIdx_iff, hs, v4_table] at b_post
  cases b <;> simp_all

/-! ## Addresses as numbers -/

theorem getD_getElem {α : Type} (l : List α) (i : Nat) (d : α) (h : i < l.length) :
    l.getD i d = l[i] := by
  simp [List.getD_eq_getElem?_getD, List.getElem?_eq_getElem h]

@[step]
theorem v4_value_spec (o : Array U8 4#usize) : ip.v4_value o ⦃ v => v.val = v4Nat o ⦄ := by
  unfold ip.v4_value
  step*
  have hl : o.val.length = 4 := by simp
  rw [v_post, i10_post, i6_post, i2_post, i5_post, i9_post, i1_post, i4_post, i8_post, i12_post]
  simp only [UScalar.cast_val_eq, v4Nat, v4, getD_getElem _ _ _ (by omega : 0 < o.val.length),
    getD_getElem _ _ _ (by omega : 1 < o.val.length), getD_getElem _ _ _ (by omega : 2 < o.val.length),
    getD_getElem _ _ _ (by omega : 3 < o.val.length), i_post, i3_post, i7_post, i11_post]
  scalar_tac

/-- **S34, IPv4.** -/
theorem is_public_v4_spec (o : Array U8 4#usize) :
    ip.is_public_v4 o ⦃ r => r = true ↔ ¬ inRanges v4NotPublic (v4Nat o) ⦄ := by
  unfold ip.is_public_v4
  step*

@[step]
theorem pair_value_spec (high low : U16) :
    ip.pair_value high low ⦃ v => v.val = high.val * 2 ^ 16 + low.val ⦄ := by
  unfold ip.pair_value
  step*

/-- The segments of an IPv6 address, by their index. -/
abbrev seg (s : Array U16 8#usize) (i : Nat) : Nat := (s.val.getD i 0#u16).val

theorem seg_eq (s : Array U16 8#usize) (i : Nat) (h : i < 8) : seg s i = (s.val[i]'(by simp; omega)).val := by
  unfold seg; rw [getD_getElem _ _ _ (by simp; omega)]

theorem seg_lt (s : Array U16 8#usize) (i : Nat) : seg s i < 2 ^ 16 := by
  unfold seg
  have := (s.val.getD i 0#u16).hBounds
  simpa using this

@[step]
theorem is_mapped_spec (s : Array U16 8#usize) :
    ip.is_mapped s ⦃ r => (r = true ↔ seg s 0 = 0 ∧ seg s 1 = 0 ∧ seg s 2 = 0 ∧ seg s 3 = 0 ∧
      seg s 4 = 0 ∧ seg s 5 = 0xffff) ⦄ := by
  unfold ip.is_mapped
  simp only [seg_eq s _ (by omega : 0 < 8), seg_eq s _ (by omega : 1 < 8), seg_eq s _ (by omega : 2 < 8),
    seg_eq s _ (by omega : 3 < 8), seg_eq s _ (by omega : 4 < 8), seg_eq s _ (by omega : 5 < 8)]
  step*

@[step]
theorem is_nat64_spec (s : Array U16 8#usize) :
    ip.is_nat64 s ⦃ r => (r = true ↔ seg s 0 = 0x64 ∧ seg s 1 = 0xff9b ∧ seg s 2 = 0 ∧ seg s 3 = 0 ∧
      seg s 4 = 0 ∧ seg s 5 = 0) ⦄ := by
  unfold ip.is_nat64
  simp only [seg_eq s _ (by omega : 0 < 8), seg_eq s _ (by omega : 1 < 8), seg_eq s _ (by omega : 2 < 8),
    seg_eq s _ (by omega : 3 < 8), seg_eq s _ (by omega : 4 < 8), seg_eq s _ (by omega : 5 < 8)]
  step*

theorem v6Nat_eq (s : Array U16 8#usize) :
    v6Nat s = v6 (seg s 0) (seg s 1) (seg s 2) (seg s 3) (seg s 4) (seg s 5) (seg s 6) (seg s 7) := rfl

/-- The facts about the number of an address that the code reads from its segments. -/
theorem v6_facts (s0 s1 s2 s3 s4 s5 s6 s7 : Nat) (h0 : s0 < 2 ^ 16) (h1 : s1 < 2 ^ 16)
    (h2 : s2 < 2 ^ 16) (h3 : s3 < 2 ^ 16) (h4 : s4 < 2 ^ 16) (h5 : s5 < 2 ^ 16) (h6 : s6 < 2 ^ 16)
    (h7 : s7 < 2 ^ 16) :
    let x := v6 s0 s1 s2 s3 s4 s5 s6 s7
    (x / 2 ^ 32 = 0xffff ↔ s0 = 0 ∧ s1 = 0 ∧ s2 = 0 ∧ s3 = 0 ∧ s4 = 0 ∧ s5 = 0xffff) ∧
    (x / 2 ^ 32 = v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 ↔
      s0 = 0x64 ∧ s1 = 0xff9b ∧ s2 = 0 ∧ s3 = 0 ∧ s4 = 0 ∧ s5 = 0) ∧
    x % 2 ^ 32 = s6 * 2 ^ 16 + s7 ∧
    x / 2 ^ 112 = s0 ∧
    x / 2 ^ 80 % 2 ^ 32 = s1 * 2 ^ 16 + s2 ∧
    x / 2 ^ 96 = s0 * 2 ^ 16 + s1 := by
  simp only [v6]
  norm_num at *
  omega

/-- **S34, IPv6.** -/
theorem is_public_v6_spec (a : Array U16 8#usize) :
    ip.is_public_v6 a ⦃ r => r = true ↔ match embeddedV4 (v6Nat a) with
      | some v4 => ¬ inRanges v4NotPublic v4
      | none => ¬ inRanges v6NotPublic (v6Nat a) ⦄ := by
  obtain ⟨fm, fn, flow, f112, f80, f96⟩ := v6_facts _ _ _ _ _ _ _ _ (seg_lt a 0) (seg_lt a 1)
    (seg_lt a 2) (seg_lt a 3) (seg_lt a 4) (seg_lt a 5) (seg_lt a 6) (seg_lt a 7)
  rw [← v6Nat_eq] at fm fn flow f112 f80 f96
  have e : ∀ i (h : i < 8), (a.val[i]'(by simp; omega)).val = seg a i := fun i h => (seg_eq a i h).symm
  have n6 : v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 ≠ 0xffff := by decide
  unfold ip.is_public_v6
  step*
  · -- IPv4-mapped.
    have hm : v6Nat a / 2 ^ 32 = 0xffff := fm.mpr (b_post.mp ‹_›)
    have he : embeddedV4 (v6Nat a) = some (v6Nat a % 2 ^ 32) := by
      unfold embeddedV4; rw [if_pos hm]
    rw [he, r_post, i2_post, flow, i_post, i1_post, e 6 (by omega), e 7 (by omega)]
  · -- NAT64.
    have hm : v6Nat a / 2 ^ 32 ≠ 0xffff := fun h => ‹¬b = true› (b_post.mpr (fm.mp h))
    have hn : v6Nat a / 2 ^ 32 = v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 := fn.mpr (b1_post.mp ‹_›)
    have he : embeddedV4 (v6Nat a) = some (v6Nat a % 2 ^ 32) := by
      unfold embeddedV4; rw [if_neg hm, if_pos hn]
    rw [he, r_post, i2_post, flow, i_post, i1_post, e 6 (by omega), e 7 (by omega)]
  · -- 6to4.
    have hm : v6Nat a / 2 ^ 32 ≠ 0xffff := fun h => ‹¬b = true› (b_post.mpr (fm.mp h))
    have hn : v6Nat a / 2 ^ 32 ≠ v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 :=
      fun h => ‹¬b1 = true› (b1_post.mpr (fn.mp h))
    have h0 : v6Nat a / 2 ^ 112 = 0x2002 := by
      have hi : i.val = 8194 := by rw [‹i = 8194#u16›]; rfl
      rw [f112, ← e 0 (by omega), ← i_post, hi]
    have he : embeddedV4 (v6Nat a) = some (v6Nat a / 2 ^ 80 % 2 ^ 32) := by
      unfold embeddedV4; rw [if_neg hm, if_neg hn, if_pos h0]
    rw [he, r_post, i3_post, f80, i1_post, i2_post, e 1 (by omega), e 2 (by omega)]
  · -- No IPv4 inside: the ranges of IPv6, by the first 32 bits.
    have hm : v6Nat a / 2 ^ 32 ≠ 0xffff := fun h => ‹¬b = true› (b_post.mpr (fm.mp h))
    have hn : v6Nat a / 2 ^ 32 ≠ v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 :=
      fun h => ‹¬b1 = true› (b1_post.mpr (fn.mp h))
    have h0 : v6Nat a / 2 ^ 112 ≠ 0x2002 := by
      rw [f112, ← e 0 (by omega), ← i_post]
      intro h; apply ‹¬i = 8194#u16›; scalar_tac
    have he : embeddedV4 (v6Nat a) = none := by
      unfold embeddedV4; rw [if_neg hm, if_neg hn, if_neg h0]
    have hs : s.val = ip.V6_NOT_PUBLIC.val := by rw [s_post]; rfl
    rw [he, inRangesIdx_iff, hs] at *
    rw [← v6_table, in_scaled, f96, ← e 0 (by omega), ← e 1 (by omega), ← i_post, ← i1_post, ← i2_post]
    cases b2 <;> simp_all

end Protocol.Ip
