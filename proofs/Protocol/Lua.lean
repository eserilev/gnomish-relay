import Protocol.Ascii
import Protocol.Spec.Lua

/-! # Lua string literals (S8) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Lua

theorem plain_iff (b : U8) :
    (ch ' ' ≤ b.bv ∧ b.bv ≤ ch '~' ∧ b.bv ≠ ch '"' ∧ b.bv ≠ ch '\\') ↔
      (32 ≤ b.val ∧ b.val ≤ 126 ∧ b.val ≠ 34 ∧ b.val ≠ 92) := by
  simp only [ch, BitVec.le_def, ne_eq, ← BitVec.toNat_inj]
  simp

@[step]
theorem is_plain_spec (b : U8) :
    lua.is_plain b ⦃ r => (r = true ↔ (32 ≤ b.val ∧ b.val ≤ 126 ∧ b.val ≠ 34 ∧ b.val ≠ 92)) ⦄ := by
  unfold lua.is_plain
  step*

theorem digit_bv (x : U8) (n : Nat) (h : n < 10) (hx : x.val = 48 + n) :
    x.bv = ch '0' + BitVec.ofNat 8 n := by
  rw [U8_bv_eq_ofNat, hx]
  apply BitVec.eq_of_toNat_eq
  simp [ch]

@[step]
theorem push_escape_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 4 ≤ Usize.max) :
    lua.push_escape out b ⦃ r =>
      bytes r.val = bytes out.val ++ (ch '\\' :: decimal3 b.bv) ∧ r.val.length = out.val.length + 4 ⦄ := by
  unfold lua.push_escape
  step*
  all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
  have hb := b.hBounds
  have d1 := digit_bv i1 (b.val / 100) (by simp at hb; omega) (by simp [i1_post, i_post])
  have d2 := digit_bv i4 (b.val / 10 % 10) (by omega) (by simp [i4_post, i3_post, i2_post])
  have d3 := digit_bv i6 (b.val % 10) (by omega) (by simp [i6_post, i5_post])
  refine ⟨?_, by simp [*]⟩
  simp only [r_post, out3_post, out2_post, out1_post, bytes, List.map_append, List.map_cons,
    List.map_nil, List.append_assoc, List.cons_append, List.nil_append, decimal3, d1, d2, d3,
    U8.bv_toNat]
  rfl

theorem luaEscapeByte_length_le (b : Spec.Byte) : (luaEscapeByte b).length ≤ 4 := by
  unfold luaEscapeByte decimal3; split <;> simp

@[step]
theorem push_escaped_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 4 ≤ Usize.max) :
    lua.push_escaped out b ⦃ r =>
      bytes r.val = bytes out.val ++ luaEscapeByte b.bv ∧ r.val.length ≤ out.val.length + 4 ⦄ := by
  unfold lua.push_escaped
  step*
  · have hp : ch ' ' ≤ b.bv ∧ b.bv ≤ ch '~' ∧ b.bv ≠ ch '"' ∧ b.bv ≠ ch '\\' := by
      rw [plain_iff]; simp_all
    refine ⟨?_, by simp [*]⟩
    simp [r_post, bytes, luaEscapeByte, hp]
  · have hp : ¬ (ch ' ' ≤ b.bv ∧ b.bv ≤ ch '~' ∧ b.bv ≠ ch '"' ∧ b.bv ≠ ch '\\') := by
      rw [plain_iff]; simp_all
    refine ⟨?_, by simp [*]⟩
    rw [r_post1]
    simp only [luaEscapeByte, if_neg hp]

def LuaInv (src : Slice U8) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ src.val.length ∧
  bytes st.1.val = ch '"' :: (bytes (src.val.take st.2.val)).flatMap luaEscapeByte ∧
  st.1.val.length ≤ 1 + 4 * st.2.val

theorem lua_string_loop_spec (src : Slice U8) (out : alloc.vec.Vec U8) (i : Usize)
    (hmax : src.val.length ≤ 2 ^ 20) (hinv : LuaInv src (out, i)) :
    lua.lua_string_loop src out i ⦃ r =>
      bytes r.val = ch '"' :: (bytes src.val).flatMap luaEscapeByte ∧
      r.val.length ≤ 1 + 4 * src.val.length ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold lua.lua_string_loop
  apply loop.spec_decr_nat (fun st => src.val.length - st.2.val) (LuaInv src) _ _ _ _ hinv
  rintro ⟨out, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold lua.lua_string_loop.body
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

/-- **S8, writer.** -/
theorem lua_string_spec (s : Slice U8) (hmax : s.val.length ≤ 2 ^ 20) :
    lua.lua_string s ⦃ v => bytes v.val = luaLiteral (bytes s.val) ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold lua.lua_string
  step as ⟨out, out_post⟩
  step with lua_string_loop_spec s out 0#usize hmax (by simp [LuaInv, out_post, bytes, ch])
    as ⟨out1, h1, h2⟩
  step*
  have hq : bytes (out1.val ++ [34#u8]) = bytes out1.val ++ [ch '"'] := by simp [bytes]; rfl
  rw [v_post, hq, h1]
  simp [luaLiteral]

/-! ## Lua 5.1 reads the literal back -/

def digitByte (n : Nat) : Spec.Byte := ch '0' + BitVec.ofNat 8 n

theorem digitByte_facts (n : Nat) (h : n < 10) :
    isDigit (digitByte n) = true ∧ isNewline (digitByte n) = false ∧ (digitByte n).toNat - 48 = n ∧
    digitByte n ≠ ch 'a' ∧ digitByte n ≠ ch 'b' ∧ digitByte n ≠ ch 'f' ∧ digitByte n ≠ ch 'n' ∧
    digitByte n ≠ ch 'r' ∧ digitByte n ≠ ch 't' ∧ digitByte n ≠ ch 'v' ∧
    digitByte n ≠ ch '"' ∧ digitByte n ≠ ch '\\' := by
  interval_cases n <;> decide

theorem readDigits_three (a b c : Nat) (ha : a < 10) (hb : b < 10) (hc : c < 10)
    (rest : List Spec.Byte) :
    readDigits (digitByte a :: digitByte b :: digitByte c :: rest) 0 0 = (100 * a + 10 * b + c, rest) := by
  obtain ⟨da, -, va, -⟩ := digitByte_facts a ha
  obtain ⟨db, -, vb, -⟩ := digitByte_facts b hb
  obtain ⟨dc, -, vc, -⟩ := digitByte_facts c hc
  simp only [readDigits, da, db, dc, va, vb, vc]
  cases rest <;> simp [readDigits] <;> omega

theorem luaReadBody_quote (rest : List Spec.Byte) :
    luaReadBody (ch '"') (ch '"' :: rest) = some ([], rest) := by
  rw [luaReadBody.eq_def]; simp

theorem luaReadBody_escapeByte (b : Spec.Byte) (rest : List Spec.Byte) :
    luaReadBody (ch '"') (luaEscapeByte b ++ rest) = consOut b (luaReadBody (ch '"') rest) := by
  unfold luaEscapeByte
  split
  · -- A plain byte stands for itself.
    rename_i hp
    obtain ⟨h1, h2, h3, h4⟩ := hp
    have hnl : isNewline b = false := by
      simp only [isNewline, ch, BitVec.le_def] at h1 ⊢
      simp only [decide_eq_false_iff_not, not_or]
      constructor <;> (intro h; subst h; simp at h1)
    rw [List.singleton_append]
    conv_lhs => rw [luaReadBody.eq_def]
    simp [h3, hnl, h4]
  · -- An escape: a backslash and three digits.
    have hb := b.isLt
    have e1 := digitByte_facts (b.toNat / 100) (by omega)
    have e2 := digitByte_facts (b.toNat / 10 % 10) (by omega)
    have e3 := digitByte_facts (b.toNat % 10) (by omega)
    have hdigits := readDigits_three (b.toNat / 100) (b.toNat / 10 % 10) (b.toNat % 10)
      (by omega) (by omega) (by omega) rest
    unfold decimal3
    simp only [digitByte] at e1 e2 e3 hdigits
    rw [List.cons_append]
    conv_lhs => rw [luaReadBody.eq_def]
    have hbs : ¬ ch '\\' = ch '"' := by decide
    have hbn : isNewline (ch '\\') = false := by decide
    simp only [hbs, if_false, hbn, Bool.false_eq_true, if_true, List.cons_append]
    obtain ⟨d1, n1, -, a1, b1, f1, nn1, r1, t1, v1, -, -⟩ := e1
    simp only [a1, b1, f1, nn1, r1, t1, v1, n1, d1, if_false, Bool.false_eq_true, if_true,
      List.nil_append, hdigits]
    have hv : 100 * (b.toNat / 100) + 10 * (b.toNat / 10 % 10) + b.toNat % 10 = b.toNat := by omega
    rw [hv]
    simp [show ¬ b.toNat > 255 by omega]

theorem luaReadBody_escaped (s rest : List Spec.Byte) :
    luaReadBody (ch '"') (s.flatMap luaEscapeByte ++ ch '"' :: rest) = some (s, rest) := by
  induction s with
  | nil => exact luaReadBody_quote rest
  | cons b s ih =>
    rw [List.flatMap_cons, List.append_assoc, luaReadBody_escapeByte, ih]
    rfl

/-- **S8, reader.** -/
theorem lua_reads_back (s rest : List Spec.Byte) :
    luaReadString (luaLiteral s ++ rest) = some (s, rest) := by
  simp only [luaLiteral, List.cons_append, luaReadString, if_true, List.append_assoc,
    List.singleton_append]
  exact luaReadBody_escaped s rest

end Protocol.Lua
