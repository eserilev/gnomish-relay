import Protocol.Code.Funs
import Protocol.Spec.Bytes
import Mathlib.Tactic.IntervalCases

/-! # The ASCII writers -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Ascii

theorem ch_digitChar (d : Nat) (h : d < 10) : ch (Nat.digitChar d) = BitVec.ofNat 8 (48 + d) := by
  interval_cases d <;> rfl

/-- The last digit comes off the end, as in `push_decimal`. -/
theorem decimal_eq (n : Nat) :
    decimal n = (if n < 10 then [] else decimal (n / 10)) ++ [BitVec.ofNat 8 (48 + n % 10)] := by
  unfold decimal
  rw [Nat.toDigits_eq_if (by omega)]
  split
  · simp [ch_digitChar n (by omega), Nat.mod_eq_of_lt (by omega : n < 10)]
  · simp [ch_digitChar (n % 10) (Nat.mod_lt _ (by omega))]

theorem decimal_length_le (n k : Nat) (hk : 1 ≤ k) (h : n < 10 ^ k) : (decimal n).length ≤ k := by
  induction k generalizing n with
  | zero => omega
  | succ k ih =>
    rw [decimal_eq]
    split
    · simp
    · have hk' : 1 ≤ k := by
        rcases k with _ | k
        · simp at h; omega
        · omega
      have : n / 10 < 10 ^ k := by rw [Nat.pow_succ] at h; omega
      have := ih (n / 10) hk' this
      simp; omega

theorem decimal_u32_length (n : U32) : (decimal n.val).length ≤ 10 :=
  decimal_length_le n.val 10 (by omega) (by have := n.hBounds; simp at this; omega)

theorem U8_bv_eq_ofNat (x : U8) : x.bv = BitVec.ofNat 8 x.val := by
  apply BitVec.eq_of_toNat_eq
  simp

/-- The digit byte that `push_decimal` writes last. -/
theorem last_digit_bv (n i : U32) (i1 i2 : U8) (hi : i.val = n.val % 10)
    (hi1 : i1 = UScalar.cast UScalarTy.U8 i) (hi2 : i2.val = 48 + i1.val) :
    i2.bv = BitVec.ofNat 8 (48 + n.val % 10) := by
  rw [U8_bv_eq_ofNat, hi2, hi1]
  have : i.val < 10 := by omega
  simp [UScalar.cast_val_eq, hi, Nat.mod_eq_of_lt (show n.val % 10 < 256 by omega)]

theorem push_bytes_loop_spec (out0 : alloc.vec.Vec U8) (src : Slice U8)
    (out : alloc.vec.Vec U8) (i : Usize)
    (hroom : out0.val.length + src.val.length ≤ Usize.max)
    (hi : i.val ≤ src.val.length) (hout : out.val = out0.val ++ src.val.take i.val) :
    ascii.push_bytes_loop out src i ⦃ r => r.val = out0.val ++ src.val ⦄ := by
  unfold ascii.push_bytes_loop
  apply loop.spec_decr_nat (fun st => src.val.length - st.2.val)
    (fun st => st.2.val ≤ src.val.length ∧ st.1.val = out0.val ++ src.val.take st.2.val)
    _ _ _ _ ⟨hi, hout⟩
  rintro ⟨out, i⟩ ⟨hi, hout⟩
  simp only at hi hout
  unfold ascii.push_bytes_loop.body
  step*
  · rw [hout]; simp; scalar_tac
  · have hlt : i.val < src.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [out1_post, hout, i3_post, List.take_add_one, List.getElem?_eq_getElem hlt, i2_post]
    simp
  · have : i.val = src.val.length := by scalar_tac
    rw [hout, this, List.take_length]

@[step]
theorem push_bytes_spec (out : alloc.vec.Vec U8) (src : Slice U8)
    (hroom : out.val.length + src.val.length ≤ Usize.max) :
    ascii.push_bytes out src ⦃ r =>
      r.val = out.val ++ src.val ∧ r.val.length = out.val.length + src.val.length ⦄ := by
  unfold ascii.push_bytes
  apply WP.spec_mono (push_bytes_loop_spec out src out 0#usize hroom (by simp) (by simp))
  intro r hr
  simp [hr]

@[step]
theorem push_decimal_spec (out : alloc.vec.Vec U8) (n : U32)
    (hroom : out.val.length + 10 ≤ Usize.max) :
    ascii.push_decimal out n ⦃ r =>
      bytes r.val = bytes out.val ++ decimal n.val ∧ r.val.length ≤ out.val.length + 10 ⦄ := by
  rw [ascii.push_decimal]
  split
  · step as ⟨q, hq⟩
    step with push_decimal_spec out q hroom as ⟨out1, hout1, _⟩
    step*
    · -- Room for the last digit: the digits of q are at most 9.
      have hlen := congrArg List.length hout1
      simp only [bytes, List.length_map, List.length_append] at hlen
      have := decimal_length_le q.val 9 (by omega) (by have := n.hBounds; simp at this; omega)
      omega
    · have hbytes : bytes r.val = bytes out.val ++ decimal n.val := by
        rw [decimal_eq n.val, if_neg (by scalar_tac), ← hq, ← List.append_assoc, ← hout1, r_post]
        simp [bytes, last_digit_bv n i i1 i2 i_post i1_post i2_post]
      refine ⟨hbytes, ?_⟩
      have := congrArg List.length hbytes
      have := decimal_u32_length n
      simp only [bytes, List.length_map, List.length_append] at *
      omega
  · step*
    have hbytes : bytes r.val = bytes out.val ++ decimal n.val := by
      rw [decimal_eq n.val, if_pos (by scalar_tac), r_post]
      simp [bytes, last_digit_bv n i i1 i2 i_post i1_post i2_post]
    refine ⟨hbytes, ?_⟩
    have := congrArg List.length hbytes
    have := decimal_u32_length n
    simp only [bytes, List.length_map, List.length_append] at *
    omega
termination_by n.val
decreasing_by all_goals scalar_tac

def PushRangeInv (out0 : alloc.vec.Vec U8) (src : Slice U8) (start : Nat)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  start ≤ st.2.val ∧ st.1.val = out0.val ++ (src.val.drop start).take (st.2.val - start)

@[step]
theorem push_range_spec (out : alloc.vec.Vec U8) (src : Slice U8) (start stop : Usize)
    (hle : start.val ≤ stop.val) (hstop : stop.val ≤ src.val.length)
    (hroom : out.val.length + (stop.val - start.val) ≤ Usize.max) :
    ascii.push_range out src start stop ⦃ r =>
      r.val = out.val ++ (src.val.drop start.val).take (stop.val - start.val) ⦄ := by
  unfold ascii.push_range ascii.push_range_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => st.2.val ≤ stop.val ∧ PushRangeInv out src start.val st) _ _ _ _
    ⟨hle, by simp [PushRangeInv]⟩
  rintro ⟨cur, i⟩ ⟨hi, hs, hcur⟩
  simp only at hi hs hcur
  unfold ascii.push_range_loop.body
  step*
  · rw [hcur]; simp; scalar_tac
  · have hlt : i.val < src.val.length := by scalar_tac
    refine ⟨by scalar_tac, ⟨by scalar_tac, ?_⟩, by scalar_tac⟩
    rw [out1_post, hcur, i2_post, show i.val + 1 - start.val = (i.val - start.val) + 1 by omega,
      List.take_add_one, List.getElem?_eq_getElem (by simp; omega)]
    simp [Nat.add_sub_cancel' hs, i1_post]

theorem bytes_equal_loop_spec (a b : Slice U8) (i : Usize) (hlen : a.val.length = b.val.length)
    (hi : i.val ≤ a.val.length) (hpre : a.val.take i.val = b.val.take i.val) :
    ascii.bytes_equal_loop a b i ⦃ r => (r = true ↔ a.val = b.val) ⦄ := by
  unfold ascii.bytes_equal_loop
  apply loop.spec_decr_nat (fun i => a.val.length - i.val)
    (fun i => i.val ≤ a.val.length ∧ a.val.take i.val = b.val.take i.val) _ _ _ _ ⟨hi, hpre⟩
  rintro i ⟨hi, hpre⟩
  unfold ascii.bytes_equal_loop.body
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
    ascii.bytes_equal a b ⦃ r => (r = true ↔ a.val = b.val) ⦄ := by
  unfold ascii.bytes_equal
  step*
  apply bytes_equal_loop_spec a b 0#usize (by scalar_tac) (by simp) (by simp)

@[step]
theorem copy_bytes_spec (src : Slice U8) : ascii.copy_bytes src ⦃ v => v.val = src.val ⦄ := by
  unfold ascii.copy_bytes
  step*
  simp [v_post]

end Protocol.Ascii
