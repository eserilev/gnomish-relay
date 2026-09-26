import Protocol.Search
import Protocol.Spec.Sbpl

/-! # Seatbelt string literals (S32) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Sbpl

theorem quote_ch : ch '"' = (34#u8 : U8).bv := by decide
theorem backslash_ch : ch '\\' = (92#u8 : U8).bv := by decide

theorem backslash_bv : sbpl.BACKSLASH.bv = ch '\\' := by unfold sbpl.BACKSLASH; decide

theorem quote_bv : sbpl.QUOTE.bv = ch '"' := by unfold sbpl.QUOTE; decide

@[step]
theorem needs_backslash_spec (b : U8) :
    sbpl.needs_backslash b ⦃ r => (r = true ↔ (b.bv = ch '"' ∨ b.bv = ch '\\')) ⦄ := by
  unfold sbpl.needs_backslash
  have hq : (b = sbpl.QUOTE ↔ b.bv = ch '"') := by
    unfold sbpl.QUOTE; rw [UScalar.eq_equiv_bv_eq, quote_ch]
  have hs : (b = sbpl.BACKSLASH ↔ b.bv = ch '\\') := by
    unfold sbpl.BACKSLASH; rw [UScalar.eq_equiv_bv_eq, backslash_ch]
  split
  · simp_all
  · simp_all

@[step]
theorem push_escaped_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 2 ≤ Usize.max) :
    sbpl.push_escaped out b ⦃ r =>
      bytes r.val = bytes out.val ++ sbplEscapeByte b.bv ∧ r.val.length ≤ out.val.length + 2 ⦄ := by
  unfold sbpl.push_escaped
  step as ⟨b1, hb1⟩
  by_cases h : b1 = true
  · have hq := hb1.mp h
    simp only [h, if_true]
    step*
    all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
    refine ⟨?_, by simp [*]⟩
    simp [*, bytes, sbplEscapeByte, backslash_bv]
  · have hq : ¬ (b.bv = ch '"' ∨ b.bv = ch '\\') := fun hq => h (hb1.mpr hq)
    simp only [Bool.not_eq_true] at h
    simp only [h, Bool.false_eq_true, if_false]
    step*
    refine ⟨?_, by simp [*]⟩
    simp [r_post, bytes, sbplEscapeByte, hq]

def SbplInv (src : Slice U8) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ src.val.length ∧
  bytes st.1.val = ch '"' :: (bytes (src.val.take st.2.val)).flatMap sbplEscapeByte ∧
  st.1.val.length ≤ 1 + 2 * st.2.val

theorem quoted_loop_spec (src : Slice U8) (out : alloc.vec.Vec U8) (i : Usize)
    (hmax : src.val.length ≤ 2 ^ 20) (hinv : SbplInv src (out, i)) :
    sbpl.quoted_loop src out i ⦃ r =>
      bytes r.val = ch '"' :: (bytes src.val).flatMap sbplEscapeByte ∧
      r.val.length ≤ 1 + 2 * src.val.length ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold sbpl.quoted_loop
  apply loop.spec_decr_nat (fun st => src.val.length - st.2.val) (SbplInv src) _ _ _ _ hinv
  rintro ⟨out, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold sbpl.quoted_loop.body
  step*
  · have hlt : i.val < src.val.length := by scalar_tac
    have ht : bytes (src.val.take (i.val + 1)) = bytes (src.val.take i.val) ++ [src.val[i.val].bv] := by
      unfold bytes
      rw [List.take_add_one, List.getElem?_eq_getElem hlt, List.map_append]
      rfl
    refine ⟨⟨by scalar_tac, ?_, by scalar_tac⟩, by scalar_tac⟩
    rw [out1_post1, hout, i3_post, ht, List.flatMap_append, i2_post]
    simp only [List.cons_append, List.flatMap_cons, List.flatMap_nil, List.append_nil]
  · have hn : i.val = src.val.length := by scalar_tac
    rw [hn, List.take_length] at hout
    rw [hn] at hlen
    exact ⟨hout, hlen⟩

theorem quoted_spec (s : Slice U8) (hmax : s.val.length ≤ 2 ^ 20) :
    sbpl.quoted s ⦃ v => bytes v.val = sbplLiteral (bytes s.val) ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold sbpl.quoted
  step as ⟨out, out_post⟩
  step with quoted_loop_spec s out 0#usize hmax (by simp [SbplInv, out_post, bytes, quote_bv])
    as ⟨out1, h1, h2⟩
  step*
  have hq : bytes (out1.val ++ [sbpl.QUOTE]) = bytes out1.val ++ [ch '"'] := by
    simp [bytes, quote_bv]
  rw [v_post, hq, h1]
  simp [sbplLiteral]

theorem zero_mem_bytes (s : List U8) : (0#u8 ∈ s) ↔ ((0 : Spec.Byte) ∈ bytes s) := by
  simp only [bytes, List.mem_map]
  constructor
  · intro h; exact ⟨0#u8, h, rfl⟩
  · rintro ⟨x, hx, he⟩
    have : x = 0#u8 := (UScalar.eq_equiv_bv_eq x 0#u8).mpr (by simpa using he)
    rw [← this]; exact hx

/-- **S32, writer.** -/
theorem sbpl_string_spec (s : Slice U8) (hmax : s.val.length ≤ 2 ^ 20) :
    sbpl.sbpl_string s ⦃ v =>
      ((0 : Spec.Byte) ∈ bytes s.val → v = none) ∧
      ((0 : Spec.Byte) ∉ bytes s.val → ∃ lit, v = some lit ∧ bytes lit.val = sbplLiteral (bytes s.val)) ⦄ := by
  unfold sbpl.sbpl_string
  step*
  · have h0 : (0 : Spec.Byte) ∈ bytes s.val := (zero_mem_bytes _).mp (b_post.mp (by assumption))
    exact ⟨fun _ => by simp, fun h => absurd h0 h⟩
  · step with quoted_spec s hmax as ⟨v, hv⟩
    have h0 : (0 : Spec.Byte) ∉ bytes s.val := by
      rw [← zero_mem_bytes]; intro h; simp_all
    exact ⟨fun h => absurd h h0, fun _ => ⟨v, rfl, hv⟩⟩

/-! ## Seatbelt reads the literal back -/

theorem sbplReadBody_quote (rest : List Spec.Byte) :
    sbplReadBody (ch '"' :: rest) = some ([], rest) := by
  rw [sbplReadBody.eq_def]; simp [ch]

theorem sbplReadBody_escapeByte (b : Spec.Byte) (hb : b ≠ 0) (rest : List Spec.Byte) :
    sbplReadBody (sbplEscapeByte b ++ rest) = consOut b (sbplReadBody rest) := by
  unfold sbplEscapeByte
  split
  · rename_i hq
    rw [List.cons_append, List.cons_append, List.nil_append]
    conv_lhs => rw [sbplReadBody.eq_def]
    rcases hq with rfl | rfl <;> simp [ch, isOctalDigit]
  · rename_i hq
    rw [List.singleton_append]
    conv_lhs => rw [sbplReadBody.eq_def]
    simp only [not_or] at hq
    have hb' : ¬ b = 0#8 := hb
    simp [hb', hq.1, hq.2]

theorem sbplReadBody_escaped (s rest : List Spec.Byte) (h0 : (0 : Spec.Byte) ∉ s) :
    sbplReadBody (s.flatMap sbplEscapeByte ++ ch '"' :: rest) = some (s, rest) := by
  induction s with
  | nil => exact sbplReadBody_quote rest
  | cons b s ih =>
    simp only [List.mem_cons, not_or] at h0
    rw [List.flatMap_cons, List.append_assoc, sbplReadBody_escapeByte b (Ne.symm h0.1), ih h0.2]
    rfl

/-- **S32, reader.** -/
theorem sbpl_reads_back (s rest : List Spec.Byte) (h0 : (0 : Spec.Byte) ∉ s) :
    sbplReadString (sbplLiteral s ++ rest) = some (s, rest) := by
  simp only [sbplLiteral, List.cons_append, sbplReadString, if_true, List.append_assoc]
  exact sbplReadBody_escaped s rest h0

end Protocol.Sbpl
