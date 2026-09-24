import Protocol.Ascii
import Protocol.Spec.Record
import Protocol.Decimal
import Protocol.Popup

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

/-! ## One record -/

/-- A range splits around one of its bytes. -/
theorem range_split {α : Type} (l : List α) (a p b : Nat) (h1 : a ≤ p) (h2 : p < b) (hb : b ≤ l.length) :
    (l.drop a).take (b - a) =
      (l.drop a).take (p - a) ++ l[p]'(by omega) :: (l.drop (p + 1)).take (b - (p + 1)) := by
  rw [show b - a = (p - a) + (1 + (b - (p + 1))) by omega, List.take_add, List.drop_drop,
    show a + (p - a) = p by omega, List.take_add, List.drop_drop]
  rw [List.drop_eq_getElem_cons (show p < l.length by omega)]
  simp [show p + 1 = 1 + p by omega]

/-- The first match is unique. -/
theorem foundAt_unique (l : List U8) (s e p q : Nat) (t : U8) (hf : FoundAt l s e p t)
    (hq1 : s ≤ q) (hq2 : q < e) (hql : q < l.length) (hqt : l[q] = t)
    (hbefore : ∀ k (hk : k < l.length), s ≤ k → k < q → l[k] ≠ t) : p = q := by
  obtain ⟨hsp, hpe, hne, hat⟩ := hf
  rcases Nat.lt_trichotomy p q with h | h | h
  · exact absurd (hat (by omega) (by omega)) (hbefore p (by omega) hsp h)
  · exact h
  · exact absurd hqt (hne q hql hq1 h)

/-- No `target` lies before the one found. -/
theorem foundAt_not_mem (l : List U8) (s e p : Nat) (t : U8) (hf : FoundAt l s e p t) (he : e ≤ l.length) :
    t ∉ (l.drop s).take (p - s) := by
  obtain ⟨hsp, hpe, hne, -⟩ := hf
  intro hmem
  obtain ⟨k, hk, hkv⟩ := List.getElem_of_mem hmem
  simp only [List.getElem_take, List.getElem_drop] at hkv
  simp only [List.length_take, List.length_drop] at hk
  exact hne (s + k) (by omega) (by omega) (by omega) hkv

@[simp, scalar_tac_simps, grind =, agrind =]
theorem us_val : record.US.val = 31 := by unfold record.US; rfl

theorem us_bv : record.US.bv = US := by unfold record.US; rfl

attribute [step] is_valid_id_spec

theorem not_mem_bytes (l : List U8) (x : U8) (h : x ∉ l) : x.bv ∉ bytes l := by
  intro hm
  simp only [bytes, List.mem_map] at hm
  obtain ⟨y, hy, hyx⟩ := hm
  have : y = x := (UScalar.eq_equiv_bv_eq y x).mpr hyx
  exact h (this ▸ hy)

/-- The bytes of a record range. -/
def region (src : Slice U8) (start stop : Nat) : List U8 := (src.val.drop start).take (stop - start)

theorem parse_record_sound (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) (hnors : RS ∉ bytes (region src start.val stop.val)) :
    record.parse_record src start stop ⦃ r => match r with
      | .Ok rc => wellFormed rc ∧ recordBytes rc = bytes (region src start.val stop.val)
      | .Err _ => True ⦄ := by
  unfold record.parse_record
  step*
  -- Each separator is inside the range.
  have n1 : ¬ u1 = stop := by assumption
  have n2 : ¬ u2 = stop := by assumption
  have n3 : ¬ u3 = stop := by assumption
  have n4 : ¬ u4 = stop := by assumption
  have n5 : ¬ u5 = stop := by assumption
  have n6 : ¬ u6 = stop := by assumption
  have nu2 := foundAt_not_mem _ _ _ _ _ u2_post hstop
  have nu4 := foundAt_not_mem _ _ _ _ _ u4_post hstop
  have nu5 := foundAt_not_mem _ _ _ _ _ u5_post hstop
  have nu6 := foundAt_not_mem _ _ _ _ _ u6_post hstop
  have hb : b = true := by assumption
  have hb1 : b1 = true := by assumption
  have ho : o = some id := by assumption
  obtain ⟨a1, e1, -, at1⟩ := u1_post
  obtain ⟨a2, e2, -, at2⟩ := u2_post
  obtain ⟨a3, e3, -, at3⟩ := u3_post
  obtain ⟨a4, e4, -, at4⟩ := u4_post
  obtain ⟨a5, e5, -, at5⟩ := u5_post
  obtain ⟨a6, e6, -, at6⟩ := u6_post
  have l1 : u1.val < stop.val := by scalar_tac
  have l2 : u2.val < stop.val := by scalar_tac
  have l3 : u3.val < stop.val := by scalar_tac
  have l4 : u4.val < stop.val := by scalar_tac
  have l5 : u5.val < stop.val := by scalar_tac
  have l6 : u6.val < stop.val := by scalar_tac
  rw [i_post] at chat_post a2; rw [i1_post] at o_post a3
  rw [i2_post] at v_post a4 nu4; rw [i3_post] at v1_post a5 nu5
  rw [i4_post] at v2_post a6 nu6; rw [i5_post] at v3_post
  rw [i_post] at nu2
  -- The range is the fields with a US between each two.
  have hregion : region src start.val stop.val =
      token.val ++ record.US :: (chat.val ++ record.US ::
        ((src.val.drop (u2.val + 1)).take (u3.val - (u2.val + 1)) ++ record.US :: (v.val ++ record.US ::
          (v1.val ++ record.US :: (v2.val ++ record.US :: v3.val))))) := by
    unfold region
    rw [range_split _ _ u1.val _ a1 l1 hstop, at1 (by omega) l1, ← token_post,
      range_split _ _ u2.val _ a2 l2 hstop, at2 (by omega) l2, ← chat_post,
      range_split _ _ u3.val _ a3 l3 hstop, at3 (by omega) l3,
      range_split _ _ u4.val _ a4 l4 hstop, at4 (by omega) l4, ← v_post,
      range_split _ _ u5.val _ a5 l5 hstop, at5 (by omega) l5, ← v1_post,
      range_split _ _ u6.val _ a6 l6 hstop, at6 (by omega) l6, ← v2_post, ← v3_post]
  have hid : decimal id.val = bytes ((src.val.drop (u2.val + 1)).take (u3.val - (u2.val + 1))) :=
    o_post.1 id ho
  have hrb : recordBytes (record.Record.mk token chat id v v1 v2 v3) =
      bytes (region src start.val stop.val) := by
    rw [hregion]
    simp only [recordBytes, hid, bytes, List.map_append, List.map_cons, us_bv, List.append_assoc,
      List.cons_append, List.nil_append]
  refine ⟨?_, hrb⟩
  -- Well-formed: valid ids, and no field bleeds into the next.
  have hsub : ∀ l : List U8, (∀ x ∈ l, x ∈ region src start.val stop.val) → RS ∉ bytes l := by
    intro l hl hm
    simp only [bytes, List.mem_map] at hm hnors
    obtain ⟨y, hy, hyr⟩ := hm
    exact hnors ⟨y, hl y hy, hyr⟩
  rw [hregion] at hsub
  refine ⟨(b_post.mp hb), (b1_post.mp hb1), ⟨hsub v.val (by intro x hx; simp [hx]), ?_⟩,
    ⟨hsub v1.val (by intro x hx; simp [hx]), ?_⟩, ⟨hsub v2.val (by intro x hx; simp [hx]), ?_⟩,
    hsub v3.val (by intro x hx; simp [hx])⟩
  · rw [← us_bv, v_post]; exact not_mem_bytes _ _ nu4
  · rw [← us_bv, v1_post]; exact not_mem_bytes _ _ nu5
  · rw [← us_bv, v2_post]; exact not_mem_bytes _ _ nu6

/-! ## A well-formed record parses back -/

theorem bytes_injective : Function.Injective bytes :=
  List.map_injective_iff.mpr fun x y h => (UScalar.eq_equiv_bv_eq x y).mpr h

/-- If a range is `A ++ t :: R`, then `t` is at `s + |A|` and the rest is `R`. -/
theorem region_cons (l : List U8) (s e : Nat) (A R : List U8) (t : U8) (he : e ≤ l.length)
    (h : (l.drop s).take (e - s) = A ++ t :: R) :
    s + A.length < e ∧ (∀ hq : s + A.length < l.length, l[s + A.length] = t) ∧
      (∀ k (hk : k < A.length) (hl : s + k < l.length), l[s + k] = A[k]) ∧
      (l.drop (s + A.length + 1)).take (e - (s + A.length + 1)) = R := by
  have hlen := congrArg List.length h
  simp only [List.length_take, List.length_drop, List.length_append, List.length_cons] at hlen
  refine ⟨by omega, fun hq => ?_, fun k hk hl => ?_, ?_⟩
  · have := congrArg (·[A.length]?) h
    simp only [List.getElem?_take, List.getElem?_drop, List.getElem?_append_right (le_refl _),
      Nat.sub_self, List.getElem?_cons_zero] at this
    rw [if_pos (by omega), List.getElem?_eq_getElem hq] at this
    simpa using this
  · have := congrArg (·[k]?) h
    simp only [List.getElem?_take, List.getElem?_drop, List.getElem?_append_left hk] at this
    rw [if_pos (by omega), List.getElem?_eq_getElem hl, List.getElem?_eq_getElem hk] at this
    simpa using this
  · have := congrArg (List.drop (A.length + 1)) h
    have hd : (A ++ t :: R).drop (A.length + 1) = R := by simp [List.drop_append]
    rw [hd, List.drop_take, List.drop_drop] at this
    rw [show s + (A.length + 1) = s + A.length + 1 by omega,
      show e - s - (A.length + 1) = e - (s + A.length + 1) by omega] at this
    exact this

/-- `find_byte` stops right after a prefix without the target. -/
theorem find_first (l : List U8) (s e p : Nat) (A R : List U8) (t : U8) (he : e ≤ l.length)
    (hA : t ∉ A) (h : (l.drop s).take (e - s) = A ++ t :: R) (hf : FoundAt l s e p t) :
    p = s + A.length := by
  obtain ⟨hlt, hat, hbefore, -⟩ := region_cons l s e A R t he h
  apply foundAt_unique l s e p (s + A.length) t hf (by omega) hlt (by omega) (hat (by omega))
  intro k hk h1 h2 hkt
  have := hbefore (k - s) (by omega) (by rw [Nat.add_sub_cancel' h1]; exact hk)
  simp only [Nat.add_sub_cancel' h1] at this
  exact hA (by rw [← hkt, this]; exact List.getElem_mem _)

theorem region_prefix (l : List U8) (s e : Nat) (A R : List U8) (t : U8)
    (h : (l.drop s).take (e - s) = A ++ t :: R) : (l.drop s).take A.length = A := by
  have hlen := congrArg List.length h
  simp only [List.length_take, List.length_drop, List.length_append, List.length_cons] at hlen
  have := congrArg (List.take A.length) h
  rw [List.take_take, show min A.length (e - s) = A.length by omega] at this
  simpa using this

theorem us_not_idByte : ¬ idByte US := by
  simp [idByte, US, ch, BitVec.le_def]

theorem us_not_mem_of_validId (l : List U8) (h : validId (bytes l)) : record.US ∉ l := by
  intro hm
  have := h.2.2 record.US.bv (by simp only [bytes, List.mem_map]; exact ⟨_, hm, rfl⟩)
  rw [us_bv] at this
  exact us_not_idByte this

theorem us_not_mem_of_clean (l : List U8) (h : cleanField (bytes l)) : record.US ∉ l := by
  intro hm
  apply h.2
  rw [← us_bv]; simp only [bytes, List.mem_map]; exact ⟨_, hm, rfl⟩

/-- The U8 digits of `decimal n`. -/
def decimalU8 (n : Nat) : List U8 := (decimal n).map fun b => (⟨b⟩ : U8)

theorem bytes_decimalU8 (n : Nat) : bytes (decimalU8 n) = decimal n := by
  simp [bytes, decimalU8]

theorem us_not_mem_decimalU8 (n : Nat) : record.US ∉ decimalU8 n := by
  intro hm
  have hmb : US ∈ decimal n := by
    rw [← bytes_decimalU8, ← us_bv]; simp only [bytes, List.mem_map]; exact ⟨_, hm, rfl⟩
  have := Protocol.Popup.decimal_printable n US hmb
  revert this; decide

-- The proof is long, so it needs more than the default time budget.
set_option maxHeartbeats 2000000 in
theorem parse_record_complete (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) (rc : record.Record) (hwf : wellFormed rc)
    (hreg : bytes (region src start.val stop.val) = recordBytes rc) :
    record.parse_record src start stop ⦃ r => r = .Ok rc ⦄ := by
  obtain ⟨vt, vc, cw, cf, cn, -⟩ := hwf
  have nt := us_not_mem_of_validId _ vt
  have nc := us_not_mem_of_validId _ vc
  have nd := us_not_mem_decimalU8 rc.id.val
  have nw := us_not_mem_of_clean _ cw
  have nf := us_not_mem_of_clean _ cf
  have nn := us_not_mem_of_clean _ cn
  have hR : region src start.val stop.val =
      rc.token.val ++ record.US :: (rc.chat.val ++ record.US :: (decimalU8 rc.id.val ++ record.US ::
        (rc.cwd.val ++ record.US :: (rc.flags.val ++ record.US :: (rc.«name».val ++ record.US ::
          rc.text.val))))) := by
    apply bytes_injective
    rw [hreg]
    simp [recordBytes, bytes, decimalU8, us_bv]
  -- Where each field starts, one after the other.
  unfold region at hR
  obtain ⟨p1, -, -, r1⟩ := region_cons src.val _ _ _ _ _ hstop hR
  obtain ⟨p2, -, -, r2⟩ := region_cons src.val _ _ _ _ _ hstop r1
  obtain ⟨p3, -, -, r3⟩ := region_cons src.val _ _ _ _ _ hstop r2
  obtain ⟨p4, -, -, r4⟩ := region_cons src.val _ _ _ _ _ hstop r3
  obtain ⟨p5, -, -, r5⟩ := region_cons src.val _ _ _ _ _ hstop r4
  obtain ⟨p6, -, -, r6⟩ := region_cons src.val _ _ _ _ _ hstop r5
  unfold record.parse_record
  step*
  -- Each separator is where the record says.
  all_goals try have e1 := find_first _ _ _ _ _ _ _ hstop nt hR u1_post
  all_goals try have e2 := find_first _ _ _ _ _ _ _ hstop nc r1 (by rw [i_post, e1] at u2_post; exact u2_post)
  all_goals try have e3 := find_first _ _ _ _ _ _ _ hstop nd r2 (by rw [i1_post, e2] at u3_post; exact u3_post)
  all_goals try have e4 := find_first _ _ _ _ _ _ _ hstop nw r3 (by rw [i2_post, e3] at u4_post; exact u4_post)
  all_goals try have e5 := find_first _ _ _ _ _ _ _ hstop nf r4 (by rw [i3_post, e4] at u5_post; exact u5_post)
  all_goals try have e6 := find_first _ _ _ _ _ _ _ hstop nn r5 (by rw [i4_post, e5] at u6_post; exact u6_post)
  -- No separator is missing.
  iterate 6 (· exfalso; scalar_tac)
  -- Each copied field is the original field.
  all_goals
    have hT : token.val = rc.token.val := by
      rw [token_post, show u1.val - start.val = rc.token.val.length by omega]
      exact region_prefix _ _ _ _ _ _ hR
  all_goals try (
    have hC : chat.val = rc.chat.val := by
      rw [chat_post, i_post, e1, show u2.val - (start.val + rc.token.val.length + 1) =
        rc.chat.val.length by omega]
      exact region_prefix _ _ _ _ _ _ r1)
  all_goals try (
    have hD : (src.val.drop i1.val).take (u3.val - i1.val) = decimalU8 rc.id.val := by
      rw [i1_post, e2, show u3.val - (start.val + rc.token.val.length + 1 + rc.chat.val.length + 1) =
        (decimalU8 rc.id.val).length by omega]
      exact region_prefix _ _ _ _ _ _ r2)
  · -- The id reads back.
    exfalso
    have ho : o = none := by assumption
    have hlt : rc.id.val < 2 ^ 32 := by have := rc.id.hBounds; simpa using this
    obtain ⟨n, hn, -⟩ := o_post.2 rc.id.val (by rw [hD, bytes_decimalU8]) hlt
    rw [ho] at hn; simp at hn
  · -- Every field comes back.
    have hlt : rc.id.val < 2 ^ 32 := by have := rc.id.hBounds; simpa using this
    have ho : o = some id := by assumption
    obtain ⟨n, hn, hnv⟩ := o_post.2 rc.id.val (by rw [hD, bytes_decimalU8]) hlt
    rw [ho] at hn; simp only [Option.some.injEq] at hn; subst hn
    have hW : v.val = rc.cwd.val := by
      rw [v_post, i2_post, e3]
      rw [show u4.val - (start.val + rc.token.val.length + 1 + rc.chat.val.length + 1 +
        (decimalU8 rc.id.val).length + 1) = rc.cwd.val.length by omega]
      exact region_prefix _ _ _ _ _ _ r3
    have hF : v1.val = rc.flags.val := by
      rw [v1_post, i3_post, e4]
      rw [show u5.val - (start.val + rc.token.val.length + 1 + rc.chat.val.length + 1 +
        (decimalU8 rc.id.val).length + 1 + rc.cwd.val.length + 1) = rc.flags.val.length by omega]
      exact region_prefix _ _ _ _ _ _ r4
    have hN : v2.val = rc.«name».val := by
      rw [v2_post, i4_post, e5]
      rw [show u6.val - (start.val + rc.token.val.length + 1 + rc.chat.val.length + 1 +
        (decimalU8 rc.id.val).length + 1 + rc.cwd.val.length + 1 + rc.flags.val.length + 1) =
        rc.«name».val.length by omega]
      exact region_prefix _ _ _ _ _ _ r5
    have hX : v3.val = rc.text.val := by
      rw [v3_post, i5_post, e6]; exact r6
    congr 1
    obtain ⟨t, c, d, w, f, nm, x⟩ := rc
    simp only at hT hC hW hF hN hX hnv
    simp only [record.Record.mk.injEq]
    exact ⟨Subtype.ext hT, Subtype.ext hC, UScalar.eq_of_val_eq hnv, Subtype.ext hW, Subtype.ext hF,
      Subtype.ext hN, Subtype.ext hX⟩
  · -- The chat id is valid.
    exfalso
    have hb1 : ¬ b1 = true := by assumption
    exact hb1 (b1_post.mpr (by rw [show chat.deref.val = chat.val from rfl, hC]; exact vc))
  · -- The token is valid.
    exfalso
    have hb : ¬ b = true := by assumption
    exact hb (b_post.mpr (by rw [show token.deref.val = token.val from rfl, hT]; exact vt))

/-! ## All records (S3, S4) -/

@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_records_val : record.MAX_RECORDS.val = 16 := by unfold record.MAX_RECORDS; rfl

theorem rs_bv : record.RS.bv = RS := by unfold record.RS; rfl

theorem intercalate_snoc {α : Type} (sep y : List α) (xs : List (List α)) (h : xs ≠ []) :
    sep.intercalate (xs ++ [y]) = sep.intercalate xs ++ sep ++ y := by
  induction xs with
  | nil => exact absurd rfl h
  | cons x t ih =>
    by_cases ht : t = []
    · subst ht; simp [List.intercalate_cons_of_ne_nil]
    · rw [List.cons_append, List.intercalate_cons_of_ne_nil (by simp),
        List.intercalate_cons_of_ne_nil ht, ih ht]
      simp

theorem recordsBytes_snoc (rs : List record.Record) (r : record.Record) (h : rs ≠ []) :
    recordsBytes (rs ++ [r]) = recordsBytes rs ++ RS :: recordBytes r := by
  unfold recordsBytes
  rw [List.map_append, List.map_singleton, intercalate_snoc _ _ _ (by simpa using h)]
  simp

theorem recordsBytes_single (r : record.Record) : recordsBytes [r] = recordBytes r := by
  simp [recordsBytes]


theorem take_split2 {α : Type} (l : List α) (s e : Nat) (h : s ≤ e) :
    l.take e = l.take s ++ (l.drop s).take (e - s) := by
  rw [← List.take_add, Nat.add_sub_cancel' h]

theorem take_split3 {α : Type} (l : List α) (s e : Nat) (h1 : s ≤ e) (h2 : e < l.length) :
    l.take (e + 1) = l.take s ++ (l.drop s).take (e - s) ++ [l[e]] := by
  rw [← take_split2 l s e h1, List.take_add_one, List.getElem?_eq_getElem h2]
  rfl

def RecordsInv (payload : Slice U8) (st : alloc.vec.Vec record.Record × Usize × Bool) : Prop :=
  st.1.val.length ≤ maxRecords ∧ (∀ r ∈ st.1.val, wellFormed r) ∧
  (st.2.2 = true → st.2.1.val ≤ payload.val.length ∧
    ((st.1.val = [] ∧ st.2.1.val = 0) ∨
     (st.1.val ≠ [] ∧ bytes (payload.val.take st.2.1.val) = recordsBytes st.1.val ++ [RS]))) ∧
  (st.2.2 = false → st.1.val ≠ [] ∧ recordsBytes st.1.val = bytes payload.val)

def RecordsPost (payload : Slice U8) (res : core.result.Result (alloc.vec.Vec record.Record) record.RecordError) :
    Prop :=
  match res with
  | .Ok out => 1 ≤ out.val.length ∧ out.val.length ≤ maxRecords ∧
      (∀ r ∈ out.val, wellFormed r) ∧ recordsBytes out.val = bytes payload.val
  | .Err _ => True

attribute [step] parse_record_sound

theorem parse_records_loop_sound (payload : Slice U8) (records : alloc.vec.Vec record.Record)
    (start : Usize) (more : Bool) (hinv : RecordsInv payload (records, start, more)) :
    record.parse_records_loop payload records start more ⦃ res => RecordsPost payload res ⦄ := by
  unfold record.parse_records_loop
  apply loop.spec_decr_nat
    (fun st => if st.2.2 then payload.val.length + 1 - st.2.1.val else 0) (RecordsInv payload)
    _ _ _ _ hinv
  rintro ⟨records, start, more⟩ ⟨hlen, hwf, hmore, hdone⟩
  simp only at hlen hwf hmore hdone
  unfold record.parse_records_loop.body
  step*
  · simp [RecordsPost]
  · -- The range up to the first RS has no RS.
    have := foundAt_not_mem _ _ _ _ _ end_post (le_refl _)
    rw [← rs_bv]
    exact not_mem_bytes _ _ this
  · unfold maxRecords at hlen; scalar_tac
  · -- The last record: the payload ends here.
    have hr : r = .Ok r1 := by assumption
    rw [hr] at r_post; obtain ⟨hwf1, hrb⟩ := r_post
    have hm : more = true := by assumption
    obtain ⟨hs, hcase⟩ := hmore hm
    have hend : «end».val = payload.val.length := by scalar_tac
    have hlt : records.val.length < 16 := by unfold maxRecords at hlen; scalar_tac
    refine ⟨⟨by rw [records1_post]; simp [maxRecords]; omega, ?_, fun h => absurd h (by simp), fun _ => ⟨?_, ?_⟩⟩, ?_⟩
    · intro x hx; rw [records1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hwf x hx
      · exact hwf1
    · simp [records1_post]
    · rw [records1_post]
      unfold region at hrb
      rw [hend] at hrb
      rcases hcase with ⟨h0, hst⟩ | ⟨hne, htake⟩
      · rw [h0, List.nil_append, recordsBytes_single, hrb, hst]; simp
      · rw [recordsBytes_snoc _ _ hne, hrb, ← List.take_append_drop start.val payload.val,
          show bytes (payload.val.take start.val ++ payload.val.drop start.val) =
            bytes (payload.val.take start.val) ++ bytes (payload.val.drop start.val) by simp [bytes], htake]
        rw [List.take_of_length_le (by simp)]
        simp
    · simp; omega
  · -- Another record follows after this RS.
    have hr : r = .Ok r1 := by assumption
    rw [hr] at r_post; obtain ⟨hwf1, hrb⟩ := r_post
    have hm : more = true := by assumption
    obtain ⟨hs, hcase⟩ := hmore hm
    obtain ⟨hse, hel, -, hat⟩ := end_post
    have hne' : «end».val ≠ payload.val.length := by scalar_tac
    have hlt : records.val.length < 16 := by unfold maxRecords at hlen; scalar_tac
    have hend : «end».val < payload.val.length := by scalar_tac
    refine ⟨⟨by rw [records1_post]; simp [maxRecords]; omega, ?_, fun _ => ⟨by scalar_tac, Or.inr ⟨by simp [records1_post], ?_⟩⟩, fun h => absurd h (by simp)⟩, by simp only [if_true]; rw [start1_post]; exact (by omega : payload.val.length + 1 - («end».val + 1) < payload.val.length + 1 - start.val)⟩
    · intro x hx; rw [records1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hwf x hx
      · exact hwf1
    · rw [records1_post, start1_post, take_split3 _ _ _ hse hend, hat hend hend]
      unfold region at hrb
      rcases hcase with ⟨h0, hst⟩ | ⟨hne, htake⟩
      · rw [h0, List.nil_append, recordsBytes_single, hrb, hst]
        simp [bytes, rs_bv]
      · rw [recordsBytes_snoc _ _ hne, hrb]
        simp only [bytes, List.map_append, List.map_cons, List.map_nil] at htake ⊢
        rw [htake, rs_bv]
        simp
  · simp [RecordsPost]
  · -- The loop is done.
    have hm : more = false := by cases more <;> simp_all
    obtain ⟨hne, hbytes⟩ := hdone hm
    exact ⟨by have := List.length_pos_iff.mpr hne; omega, hlen, hwf, hbytes⟩

/-- **S3 + S4 + S13, parser.** -/
theorem parse_records_sound (payload : Slice U8) :
    record.parse_records payload ⦃ res => match res with
      | .Ok out => 1 ≤ out.val.length ∧ out.val.length ≤ maxRecords ∧
          (∀ r ∈ out.val, wellFormed r) ∧ recordsBytes out.val = bytes payload.val
      | .Err _ => True ⦄ := by
  unfold record.parse_records
  step*
  apply WP.spec_mono (parse_records_loop_sound payload _ 0#usize true (by simp [RecordsInv, maxRecords]))
  intro res hres
  exact hres

/-! ## The serializer (C3) -/

@[simp] theorem deref_val {α : Type} (v : alloc.vec.Vec α) : (alloc.vec.Vec.deref v).val = v.val := rfl

theorem push_record_spec (out : alloc.vec.Vec U8) (r : record.Record)
    (hroom : out.val.length + (recordBytes r).length + 10 ≤ Usize.max) :
    record.push_record out r ⦃ o => bytes o.val = bytes out.val ++ recordBytes r ⦄ := by
  have hR : (recordBytes r).length = r.token.val.length + 1 + r.chat.val.length + 1 +
      (decimal r.id.val).length + 1 + r.cwd.val.length + 1 + r.flags.val.length + 1 +
      r.«name».val.length + 1 + r.text.val.length := by
    simp [recordBytes, bytes]; omega
  unfold record.push_record
  step*
  -- The length after each step.
  all_goals try have l2 : out2.val.length = out1.val.length + 1 := by rw [out2_post]; simp
  all_goals try have l4 : out4.val.length = out3.val.length + 1 := by rw [out4_post]; simp
  all_goals try (
    have l5 : out5.val.length = out4.val.length + (decimal r.id.val).length := by
      have := congrArg List.length out5_post1; simp [bytes] at this; omega)
  all_goals try have l6 : out6.val.length = out5.val.length + 1 := by rw [out6_post]; simp
  all_goals try have l8 : out8.val.length = out7.val.length + 1 := by rw [out8_post]; simp
  all_goals try have l10 : out10.val.length = out9.val.length + 1 := by rw [out10_post]; simp
  all_goals try have l12 : out12.val.length = out11.val.length + 1 := by rw [out12_post]; simp
  all_goals try (simp only [deref_val] at *; omega)
  rw [o_post1, out12_post, out11_post1, out10_post, out9_post1, out8_post, out7_post1, out6_post]
  simp only [Protocol.Popup.bytes_append]
  rw [out5_post1, out4_post, Protocol.Popup.bytes_append, out3_post1, Protocol.Popup.bytes_append,
    out2_post, Protocol.Popup.bytes_append, out1_post1, Protocol.Popup.bytes_append]
  simp [recordBytes, bytes, us_bv]

attribute [step] push_record_spec

@[step]
theorem push_separator_spec (out : alloc.vec.Vec U8) (i : Usize) (hroom : out.val.length + 1 ≤ Usize.max) :
    record.push_separator out i ⦃ o =>
      bytes o.val = bytes out.val ++ (if i.val > 0 then [RS] else []) ∧
      o.val.length ≤ out.val.length + 1 ⦄ := by
  unfold record.push_separator
  split
  · step*
    refine ⟨?_, by simp [o_post]⟩
    rw [o_post, Protocol.Popup.bytes_append, if_pos (by scalar_tac)]
    simp [bytes, rs_bv]
  · step*

theorem recordsBytes_take_succ (rs : List record.Record) (k : Nat) (hk : k < rs.length) :
    recordsBytes (rs.take (k + 1)) =
      recordsBytes (rs.take k) ++ (if k > 0 then [RS] else []) ++ recordBytes rs[k] := by
  rw [List.take_add_one, List.getElem?_eq_getElem hk, Option.toList_some]
  by_cases h0 : k = 0
  · subst h0; simp [recordsBytes]
  · have hne : rs.take k ≠ [] := by
      rw [ne_eq, List.take_eq_nil_iff]
      rintro (h | h)
      · exact h0 h
      · simp [h] at hk
    rw [recordsBytes_snoc _ _ hne, if_pos (by omega)]
    simp

theorem recordsBytes_take_length_le (rs : List record.Record) (k : Nat) :
    (recordsBytes (rs.take k)).length ≤ (recordsBytes rs).length := by
  suffices ∀ d, (recordsBytes (rs.take (rs.length - d))).length ≤ (recordsBytes rs).length by
    by_cases hk : k ≤ rs.length
    · have := this (rs.length - k); rwa [show rs.length - (rs.length - k) = k by omega] at this
    · rw [List.take_of_length_le (by omega)]
  intro d
  induction d with
  | zero => simp
  | succ d ih =>
    by_cases hd : d < rs.length
    · have := recordsBytes_take_succ rs (rs.length - (d + 1)) (by omega)
      rw [show rs.length - (d + 1) + 1 = rs.length - d by omega] at this
      have := congrArg List.length this
      simp only [List.length_append] at this
      omega
    · rw [show rs.length - (d + 1) = 0 by omega]; simp [recordsBytes]

def SerializeInv (rs : Slice record.Record) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ rs.val.length ∧ bytes st.1.val = recordsBytes (rs.val.take st.2.val)

theorem serialize_records_loop_spec (rs : Slice record.Record) (out : alloc.vec.Vec U8) (i : Usize)
    (hmax : (recordsBytes rs.val).length ≤ 2 ^ 20) (hinv : SerializeInv rs (out, i)) :
    record.serialize_records_loop rs out i ⦃ v => bytes v.val = recordsBytes rs.val ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold record.serialize_records_loop
  apply loop.spec_decr_nat (fun st => rs.val.length - st.2.val) (SerializeInv rs) _ _ _ _ hinv
  rintro ⟨out, i⟩ ⟨hi, hout⟩
  simp only at hi hout
  have hlen : out.val.length = (recordsBytes (rs.val.take i.val)).length := by
    have := congrArg List.length hout; simpa [bytes] using this
  have hle := recordsBytes_take_length_le rs.val i.val
  unfold record.serialize_records_loop.body
  step*
  · -- Room for the next record.
    have hlt : i.val < rs.val.length := by scalar_tac
    have hsucc := congrArg List.length (recordsBytes_take_succ rs.val i.val hlt)
    have hle1 := recordsBytes_take_length_le rs.val (i.val + 1)
    simp only [List.length_append] at hsucc
    rw [r_post]
    split at hsucc <;> simp at hsucc <;> omega
  · -- One more record: the invariant holds for i + 1.
    have hlt : i.val < rs.val.length := by scalar_tac
    refine ⟨⟨by scalar_tac, ?_⟩, by scalar_tac⟩
    rw [out2_post, out1_post1, hout, i2_post, recordsBytes_take_succ _ _ hlt, r_post]
  · -- Past the end.
    have : i.val = rs.val.length := by scalar_tac
    rw [hout, this, List.take_length]

/-- **C3, serializer.** -/
theorem serialize_records_spec (rs : Slice record.Record) (hmax : (recordsBytes rs.val).length ≤ 2 ^ 20) :
    record.serialize_records rs ⦃ v => bytes v.val = recordsBytes rs.val ⦄ := by
  unfold record.serialize_records
  apply serialize_records_loop_spec rs _ _ hmax
  simp [SerializeInv, bytes, recordsBytes]

/-! ## Well-formed records parse back (C3) -/

theorem rs_not_idByte : ¬ idByte RS := by
  simp [idByte, RS, ch, BitVec.le_def]

theorem rs_not_mem_recordBytes (r : record.Record) (hwf : wellFormed r) : RS ∉ recordBytes r := by
  obtain ⟨vt, vc, ⟨cw, -⟩, ⟨cf, -⟩, ⟨cn, -⟩, ct⟩ := hwf
  have hus : RS ≠ US := by decide
  have hdec : RS ∉ decimal r.id.val := by
    intro h
    have := Protocol.Popup.decimal_printable r.id.val RS h
    revert this; decide
  have hid : ∀ l, validId l → RS ∉ l := fun l hv h => rs_not_idByte (hv.2.2 RS h)
  simp only [recordBytes, List.mem_append, List.mem_singleton, not_or]
  exact ⟨⟨⟨⟨⟨⟨⟨⟨⟨⟨⟨⟨hid _ vt, hus⟩, hid _ vc⟩, hus⟩, hdec⟩, hus⟩, cw⟩, hus⟩, cf⟩, hus⟩, cn⟩, hus⟩, ct⟩

theorem recordsBytes_cons (r : record.Record) (tail : List record.Record) :
    recordsBytes (r :: tail) = recordBytes r ++ (if tail = [] then [] else RS :: recordsBytes tail) := by
  unfold recordsBytes
  split
  · rename_i h; subst h; simp
  · rename_i h
    rw [List.map_cons, List.intercalate_cons_of_ne_nil (by simpa using h)]
    simp

def ParseInv (payload : Slice U8) (rs : List record.Record)
    (st : alloc.vec.Vec record.Record × Usize × Bool) : Prop :=
  ∃ k, st.1.val = rs.take k ∧ k ≤ rs.length ∧
    (st.2.2 = true → k < rs.length ∧ st.2.1.val ≤ payload.val.length ∧
      bytes (payload.val.drop st.2.1.val) = recordsBytes (rs.drop k)) ∧
    (st.2.2 = false → k = rs.length)

theorem parse_records_loop_complete (payload : Slice U8) (rs : List record.Record)
    (hlen : rs.length ≤ 16) (hwf : ∀ r ∈ rs, wellFormed r)
    (records : alloc.vec.Vec record.Record) (start : Usize) (more : Bool)
    (hinv : ParseInv payload rs (records, start, more)) :
    record.parse_records_loop payload records start more ⦃ res => ∃ out, res = .Ok out ∧ out.val = rs ⦄ := by
  unfold record.parse_records_loop
  apply loop.spec_decr_nat
    (fun st => if st.2.2 then payload.val.length + 1 - st.2.1.val else 0) (ParseInv payload rs)
    _ _ _ _ hinv
  rintro ⟨records, start, more⟩ ⟨k, hrec, hk, hmore, hdone⟩
  simp only at hrec hk hmore hdone
  unfold record.parse_records_loop.body
  dsimp only
  split
  · rename_i hm
    obtain ⟨hk', hs, hrest⟩ := hmore hm
    rw [List.drop_eq_getElem_cons hk', recordsBytes_cons] at hrest
    obtain ⟨A, Z, hAZ, hA, hZ⟩ := List.map_eq_append_iff.mp hrest
    have hwfk := hwf rs[k] (List.getElem_mem hk')
    have hRSA : record.RS ∉ A := by
      intro h
      apply rs_not_mem_recordBytes _ hwfk
      rw [← hA, ← rs_bv]
      exact List.mem_map_of_mem h
    have hrlen : records.val.length = k := by rw [hrec, List.length_take]; omega
    step as ⟨«end», hend⟩
    split
    · -- Not too many: only k < 16 records so far.
      exfalso
      rename_i hmax
      have := congrArg UScalar.val hmax
      simp [hrlen] at this
      omega
    have hfull : (payload.val.drop start.val).take (payload.len.val - start.val) = payload.val.drop start.val :=
      List.take_of_length_le (by simp)
    by_cases htail : rs.drop (k + 1) = []
    · -- The last record: the payload ends after it.
      rw [if_pos htail] at hZ
      have hZ0 : Z = [] := List.map_eq_nil_iff.mp hZ
      rw [hZ0, List.append_nil] at hAZ
      have hend_eq : «end».val = payload.val.length := by
        obtain ⟨h1, h2, -, hat⟩ := hend
        by_contra hne
        have hlt : «end».val < payload.val.length := by simp at h2; omega
        have hx := hat hlt (by simpa using hlt)
        apply hRSA
        have : (payload.val.drop start.val)[«end».val - start.val]'(by simp; omega) = record.RS := by
          simp [Nat.add_sub_cancel' h1, hx]
        rw [← this]
        simp only [hAZ]
        exact List.getElem_mem _
      have hreg : bytes (region payload start.val «end».val) = recordBytes rs[k] := by
        unfold region
        rw [hend_eq, List.take_of_length_le (by simp), hAZ, ← hA]; rfl
      step with parse_record_complete payload start «end» (by simp [hend_eq]; omega)
        (by simp [hend_eq]) rs[k] hwfk hreg
      step*
      have hk1 : k + 1 = rs.length := by
        have := List.length_drop (l := rs) (i := k + 1); rw [htail] at this; simp at this; omega
      refine ⟨⟨k + 1, ?_, by omega, fun h => absurd h (by simp), fun _ => hk1⟩, by simp; omega⟩
      have hr1 : r = .Ok r1 := by assumption
      rw [r_post] at hr1; cases hr1
      rw [records1_post, hrec, List.take_add_one, List.getElem?_eq_getElem hk']
      rfl
    · -- More records follow after an RS.
      rw [if_neg htail] at hZ
      obtain ⟨x, Z', hZx, hx, hZ'⟩ := List.map_eq_cons_iff.mp hZ
      have hxrs : x = record.RS := (UScalar.eq_equiv_bv_eq x record.RS).mpr (hx.trans rs_bv.symm)
      rw [hZx, hxrs] at hAZ
      have hform : (payload.val.drop start.val).take (payload.len.val - start.val) = A ++ record.RS :: Z' := by
        rw [hfull, hAZ]
      have hend_eq := find_first payload.val start.val payload.len.val «end».val A Z' record.RS
        (by simp) hRSA hform hend
      obtain ⟨hlt, -, -, hrest'⟩ := region_cons payload.val _ _ _ _ _ (by simp) hform
      have hreg : bytes (region payload start.val «end».val) = recordBytes rs[k] := by
        unfold region
        rw [hend_eq, Nat.add_sub_cancel_left, region_prefix _ _ _ _ _ _ hform, ← hA]; rfl
      step with parse_record_complete payload start «end» (by omega) (by simp at hlt; omega)
        rs[k] hwfk hreg
      step*
      have hr1 : r = .Ok r1 := by assumption
      rw [r_post] at hr1; cases hr1
      have hk1 : k + 1 < rs.length := by
        by_contra h
        exact htail (List.drop_eq_nil_of_le (by omega))
      have hs1 : start1.val = start.val + A.length + 1 := by rw [start1_post, hend_eq]
      have hlt' : start.val + A.length < payload.val.length := by simp at hlt; exact hlt
      have hdrop1 : payload.val.drop start1.val = Z' := by
        rw [hs1, ← hrest']
        exact (List.take_of_length_le (by simp)).symm
      refine ⟨⟨k + 1, ?_, by omega, fun _ => ⟨hk1, by rw [hs1]; exact hlt', ?_⟩, fun h => absurd h (by simp)⟩, ?_⟩
      · rw [records1_post, hrec, List.take_add_one, List.getElem?_eq_getElem hk']
        rfl
      · rw [hdrop1, bytes, hZ']
      · simp only [if_true]
        exact (by omega : payload.val.length + 1 - start1.val < payload.val.length + 1 - start.val)
  · -- The loop is done.
    have hk2 := hdone (by simpa using ‹¬ more = true›)
    step*
    exact ⟨records, rfl, by rw [hrec, hk2, List.take_length]⟩

theorem recordsBytes_ne_nil (rs : List record.Record) (h : rs ≠ []) : recordsBytes rs ≠ [] := by
  obtain ⟨r, t, rfl⟩ := List.exists_cons_of_ne_nil h
  rw [recordsBytes_cons]
  simp [recordBytes]

/-- **C3, parser.** -/
theorem parse_records_complete (rs : List record.Record) (payload : Slice U8)
    (h1 : 1 ≤ rs.length) (h16 : rs.length ≤ maxRecords) (hwf : ∀ r ∈ rs, wellFormed r)
    (hp : bytes payload.val = recordsBytes rs) :
    record.parse_records payload ⦃ res => ∃ out, res = .Ok out ∧ out.val = rs ⦄ := by
  unfold maxRecords at h16
  unfold record.parse_records
  step*
  · exfalso
    apply recordsBytes_ne_nil rs (by intro h; simp [h] at h1)
    rw [← hp]
    have : payload.val.length = 0 := by scalar_tac
    simp [bytes, List.length_eq_zero_iff.mp this]
  · apply parse_records_loop_complete payload rs h16 hwf
    exact ⟨0, by simp, by omega, fun _ => ⟨by omega, by simp, by simpa using hp⟩, fun h => absurd h (by simp)⟩

end Protocol.Record
