import Protocol.Ascii
import Protocol.Spec.Record

/-! # Records (S13, C3, S3, S4) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

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

end Protocol.Record
