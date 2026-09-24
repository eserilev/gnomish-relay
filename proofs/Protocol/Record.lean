import Protocol.Ascii
import Protocol.Spec.Record
import Protocol.Decimal

/-! # Records (S13, C3, S3, S4) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.Decimal

namespace Protocol.Record

@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_id_len_val : record.MAX_ID_LEN.val = 32 := by unfold record.MAX_ID_LEN; rfl

theorem idByte_iff (b : U8) :
    idByte b.bv ↔ (97 ≤ b.val ∧ b.val ≤ 122) ∨ (48 ≤ b.val ∧ b.val ≤ 57) ∨ b.val = 95 ∨ b.val = 45 := by
  simp only [idByte, ch, BitVec.le_def, ← BitVec.toNat_inj]
  simp

@[step]
theorem is_id_byte_spec (b : U8) : record.is_id_byte b ⦃ r => (r = true ↔ idByte b.bv) ⦄ := by
  unfold record.is_id_byte
  step*
  all_goals rw [idByte_iff]
  all_goals scalar_tac

theorem is_valid_id_loop_spec (src : Slice U8) (i : Usize) (hi : i.val ≤ src.val.length)
    (hinv : ∀ j (hj : j < i.val), idByte (src.val[j]'(by omega)).bv) :
    record.is_valid_id_loop src i ⦃ r => (r = true ↔ ∀ b ∈ bytes src.val, idByte b) ⦄ := by
  unfold record.is_valid_id_loop
  apply loop.spec_decr_nat (fun i => src.val.length - i.val)
    (fun i => ∃ hi : i.val ≤ src.val.length, ∀ j (hj : j < i.val), idByte (src.val[j]'(by omega)).bv)
    _ _ _ _ ⟨hi, hinv⟩
  rintro i ⟨hi, hinv⟩
  unfold record.is_valid_id_loop.body
  step*
  · -- Byte i is an id byte: the invariant holds for i + 1.
    have hlt : i.val < src.val.length := by scalar_tac
    have hb : idByte (src.val[i.val]'hlt).bv := by rw [← i2_post]; simp_all
    refine ⟨⟨by scalar_tac, fun j hj => ?_⟩, by scalar_tac⟩
    by_cases hji : j < i.val
    · exact hinv j hji
    · have : j = i.val := by scalar_tac
      subst this
      exact hb
  · -- Byte i is not an id byte.
    simp only [Bool.false_eq_true, false_iff, not_forall]
    have hlt : i.val < src.val.length := by scalar_tac
    refine ⟨(src.val[i.val]'hlt).bv, by simp only [bytes, List.mem_map]; exact ⟨_, List.getElem_mem hlt, rfl⟩, ?_⟩
    rw [← i2_post]; simp_all
  · -- Every byte was an id byte.
    simp only [true_iff]
    intro b hb
    simp only [bytes, List.mem_map] at hb
    obtain ⟨x, hx, rfl⟩ := hb
    obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hx
    exact hinv j (by scalar_tac)

/-- **S13.** -/
theorem is_valid_id_spec (input : Slice U8) :
    record.is_valid_id input ⦃ r => (r = true ↔ validId (bytes input.val)) ⦄ := by
  unfold record.is_valid_id
  step*
  · simp [validId, bytes]; scalar_tac
  · simp [validId, bytes]; scalar_tac
  · apply WP.spec_mono (is_valid_id_loop_spec input 0#usize (by simp) (by simp))
    intro r hr
    rw [hr]
    simp only [validId, bytes, List.length_map]
    constructor
    · intro h; exact ⟨by scalar_tac, by scalar_tac, h⟩
    · intro h; exact h.2.2

/-! ## Finding separators and copying fields -/

/-- `find_byte` returns the first `target` in `[from, end)`, or `end`. -/
def FoundAt (src : List U8) (start stop p : Nat) (target : U8) : Prop :=
  start ≤ p ∧ p ≤ stop ∧ (∀ k (hk : k < src.length), start ≤ k → k < p → src[k] ≠ target) ∧
    (∀ hp : p < src.length, p < stop → src[p] = target)

@[step]
theorem find_byte_spec (src : Slice U8) (start stop : Usize) (target : U8)
    (hle : start.val ≤ stop.val) (hstop : stop.val ≤ src.val.length) :
    record.find_byte src start stop target ⦃ p => FoundAt src.val start.val stop.val p.val target ⦄ := by
  unfold record.find_byte record.find_byte_loop
  apply loop.spec_decr_nat (fun i => stop.val - i.val)
    (fun i => start.val ≤ i.val ∧ i.val ≤ stop.val ∧
      ∀ k (hk : k < src.val.length), start.val ≤ k → k < i.val → src.val[k] ≠ target)
    _ _ _ _ ⟨le_refl _, hle, fun k _ h1 h2 => absurd h2 (by omega)⟩
  rintro i ⟨hs, hi, hne⟩
  unfold record.find_byte_loop.body
  step*
  · -- Found it at i.
    refine ⟨hs, hi, hne, fun hp _ => ?_⟩
    rw [← i1_post]; assumption
  · -- Not at i: continue.
    refine ⟨by scalar_tac, by scalar_tac, fun k hk h1 h2 => ?_, by scalar_tac⟩
    by_cases hki : k < i.val
    · exact hne k hk h1 hki
    · have : k = i.val := by scalar_tac
      subst this
      rw [← i1_post]; assumption
  · -- Reached the end.
    have : i.val = stop.val := by scalar_tac
    refine ⟨by omega, le_refl _, fun k hk h1 h2 => hne k hk h1 (by omega), fun hp h => absurd h (by omega)⟩

@[step]
theorem copy_field_spec (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) :
    record.copy_field src start stop ⦃ v => v.val = (src.val.drop start.val).take (stop.val - start.val) ⦄ := by
  unfold record.copy_field
  step*

/-! ## Decimal ids -/

theorem take_drop_succ' {α : Type} (l : List α) (s i : Nat) (hs : s ≤ i) (hi : i < l.length) :
    (l.drop s).take (i + 1 - s) = (l.drop s).take (i - s) ++ [l[i]] := by
  rw [show i + 1 - s = (i - s) + 1 by omega, List.take_add_one,
    List.getElem?_eq_getElem (by simp; omega)]
  simp [Nat.add_sub_cancel' hs]

theorem take_split {α : Type} (l : List α) (s i e : Nat) (h1 : s ≤ i) (h2 : i < e) :
    (l.drop s).take (e - s) = (l.drop s).take (i + 1 - s) ++ (l.drop (i + 1)).take (e - (i + 1)) := by
  rw [show e - s = (i + 1 - s) + (e - (i + 1)) by omega, List.take_add, List.drop_drop,
    show s + (i + 1 - s) = i + 1 by omega]

def DigitsInv (src : Slice U8) (start : Nat) (st : U64 × Usize) : Prop :=
  start ≤ st.2.val ∧
  digitsVal (bytes ((src.val.drop start).take (st.2.val - start))) = some st.1.val

@[step]
theorem digits_value_spec (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) (hten : stop.val - start.val ≤ 10) :
    record.digits_value src start stop ⦃ r =>
      r.map (·.val) = digitsVal (bytes ((src.val.drop start.val).take (stop.val - start.val))) ⦄ := by
  unfold record.digits_value record.digits_value_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => st.2.val ≤ stop.val ∧ DigitsInv src start.val st) _ _ _ _
    ⟨hle, by simp [DigitsInv, bytes, digitsVal_nil]⟩
  rintro ⟨value, i⟩ ⟨hi, hs, hv⟩
  simp only at hi hs hv
  have hbound := digitsVal_lt _ _ hv
  have hplen : ((src.val.drop start.val).take (i.val - start.val)).length = i.val - start.val := by
    simp; omega
  rw [bytes, List.length_map, hplen] at hbound
  unfold record.digits_value_loop.body
  step*
  all_goals try (
    have hlt : i.val < src.val.length := by scalar_tac
    have hsplit := take_split src.val start.val i.val stop.val hs (by scalar_tac)
    have hpre := take_drop_succ' src.val start.val i.val hs hlt)
  · -- A byte below '0'.
    simp only [Option.map_none]
    rw [hsplit, hpre, bytes, List.map_append, List.map_append]
    symm; apply digitsVal_none_append
    rw [List.map_singleton, digitsVal_append_one, ← bytes, hv]
    simp [isDigitB, ← b_post]; scalar_tac
  · -- A byte above '9'.
    simp only [Option.map_none]
    rw [hsplit, hpre, bytes, List.map_append, List.map_append]
    symm; apply digitsVal_none_append
    rw [List.map_singleton, digitsVal_append_one, ← bytes, hv]
    simp [isDigitB, ← b_post]; scalar_tac
  · have : 10 ^ (i.val - start.val) ≤ 10 ^ 9 := Nat.pow_le_pow_right (by norm_num) (by scalar_tac)
    scalar_tac
  · have : 10 ^ (i.val - start.val) ≤ 10 ^ 9 := Nat.pow_le_pow_right (by norm_num) (by scalar_tac)
    scalar_tac
  · -- A digit: the invariant holds for i + 1.
    refine ⟨by scalar_tac, ⟨by scalar_tac, ?_⟩, by scalar_tac⟩
    rw [i4_post, hpre, bytes, List.map_append, List.map_singleton, digitsVal_append_one, ← bytes, hv]
    have hb57 : b.val ≤ 57 := by scalar_tac
    have hi3 : i3.val = b.val - 48 := by simp [i3_post, UScalar.cast_val_eq, i2_post1]; omega
    simp only [Option.bind_some, isDigitB, ← b_post, U8.bv_toNat, i2_post2, hb57, value1_post,
      i1_post, hi3]
    simp [Nat.mul_comm]
  · -- Past the end.
    have : i.val = stop.val := by scalar_tac
    rw [this] at hv
    simp [hv]

def DecimalSpec (f : List Spec.Byte) (r : Option U32) : Prop :=
  (∀ n, r = some n → decimal n.val = f) ∧
  (∀ m, f = decimal m → m < 2 ^ 32 → ∃ n, r = some n ∧ n.val = m)

theorem decimal_u32_length_le (m : Nat) (h : m < 2 ^ 32) : (decimal m).length ≤ 10 :=
  decimal_length_le m 10 (by omega) (by omega)

@[step]
theorem parse_decimal_spec (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) :
    record.parse_decimal src start stop ⦃ r =>
      DecimalSpec (bytes ((src.val.drop start.val).take (stop.val - start.val))) r ⦄ := by
  have hflen : (bytes ((src.val.drop start.val).take (stop.val - start.val))).length =
      stop.val - start.val := by simp [bytes]; omega
  have hu32 : (UScalar.cast UScalarTy.U64 core.num.U32.MAX).val = 4294967295 := by
    simp [UScalar.cast_val_eq]; rfl
  unfold record.parse_decimal
  step*
  -- Every rejection is right: no decimal below 2^32 looks like this field.
  · refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    exfalso
    have h1 := decimal_ne_nil m; rw [← hm] at h1; apply h1
    rw [← List.length_eq_zero_iff, hflen]; scalar_tac
  · refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    have h1 := decimal_u32_length_le m hlt; rw [← hm, hflen] at h1; scalar_tac
  · refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    rename_i hi48
    have h1 := decimal_head m (by rw [← hm, hflen]; scalar_tac)
    rw [← hm] at h1; exfalso; apply h1
    rw [bytes, List.head?_map, List.head?_take, if_neg (by scalar_tac), List.head?_drop,
      List.getElem?_eq_getElem (by scalar_tac)]
    simp only [Option.map_some, Option.some.injEq]
    rw [← i_post, hi48]; rfl
  · refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    rename_i ho
    have h1 := digitsVal_decimal m; rw [← hm] at h1; rw [h1, ho] at o_post
    simp at o_post
  · refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    rename_i ho hgt
    have h1 := digitsVal_decimal m; rw [← hm] at h1; rw [h1, ho] at o_post
    simp at o_post
    have hv : value.val > 4294967295 := by
      have : value.val > i1.val := hgt
      rwa [i1_post, hu32] at this
    scalar_tac
  · -- Accepted, more than one digit.
    have hi48 : ¬ i = 48#u8 := by assumption
    have ho : o = some value := by assumption
    have hgt : ¬ value > i1 := by assumption
    have hle' : value.val ≤ 4294967295 := by
      have : ¬ value.val > i1.val := hgt
      rw [i1_post, hu32] at this; omega
    have hi2 : i2.val = value.val := by simp [i2_post, UScalar.cast_val_eq]; omega
    rw [ho] at o_post; simp only [Option.map_some] at o_post
    refine ⟨fun n hn => ?_, fun m hm hlt => ⟨i2, rfl, ?_⟩⟩
    · simp only [Option.some.injEq] at hn; subst hn
      rw [hi2]
      apply decimal_digitsVal _ (by rw [← List.length_pos_iff, hflen]; scalar_tac) _ _ o_post.symm
      intro _
      rw [bytes, List.head?_map, List.head?_take, if_neg (by scalar_tac), List.head?_drop,
        List.getElem?_eq_getElem (by scalar_tac)]
      simp only [Option.map_some, ne_eq, Option.some.injEq]
      rw [← i_post]
      intro h48; apply hi48
      rw [UScalar.eq_equiv_bv_eq]; exact h48.trans rfl
    · rw [hm, digitsVal_decimal] at o_post
      simp only [Option.some.injEq] at o_post
      omega
  · -- One digit, not a digit.
    refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    have ho : o = none := by assumption
    have h1 := digitsVal_decimal m; rw [← hm] at h1; rw [h1, ho] at o_post
    simp at o_post
  · -- One digit, too large (cannot happen, but the code checks).
    refine ⟨fun n h => by simp at h, fun m hm hlt => ?_⟩
    have ho : o = some value := by assumption
    have hgt : value > i1 := by assumption
    have h1 := digitsVal_decimal m; rw [← hm] at h1; rw [h1, ho] at o_post
    simp at o_post
    have hv : value.val > 4294967295 := by
      have : value.val > i1.val := hgt
      rwa [i1_post, hu32] at this
    scalar_tac
  · -- Accepted, one digit.
    have ho : o = some value := by assumption
    have hgt : ¬ value > i1 := by assumption
    have hle' : value.val ≤ 4294967295 := by
      have : ¬ value.val > i1.val := hgt
      rw [i1_post, hu32] at this; omega
    have hi2 : i2.val = value.val := by simp [i2_post, UScalar.cast_val_eq]; omega
    rw [ho] at o_post; simp only [Option.map_some] at o_post
    refine ⟨fun n hn => ?_, fun m hm hlt => ⟨i2, rfl, ?_⟩⟩
    · simp only [Option.some.injEq] at hn; subst hn
      rw [hi2]
      apply decimal_digitsVal _ (by rw [← List.length_pos_iff, hflen]; scalar_tac) _ _ o_post.symm
      intro h; rw [hflen] at h; scalar_tac
    · rw [hm, digitsVal_decimal] at o_post
      simp only [Option.some.injEq] at o_post
      omega

end Protocol.Record
