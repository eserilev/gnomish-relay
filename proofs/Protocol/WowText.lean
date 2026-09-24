import Protocol.Ascii
import Protocol.Spec.WowText

/-! # WoW chat text (S10) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.WowText

/-- What `chat_safe` writes for one byte. -/
def safeByte (b : Spec.Byte) : List Spec.Byte := if b = pipe then [pipe, pipe] else [b]

theorem is_pipe_iff (b : U8) : b.bv = pipe ↔ b.val = 124 := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simpa [pipe, ch] using this
  · intro h; apply BitVec.eq_of_toNat_eq; simp [pipe, ch, h]

@[step]
theorem push_safe_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 2 ≤ Usize.max) :
    wow_text.push_safe out b ⦃ r =>
      bytes r.val = bytes out.val ++ safeByte b.bv ∧ r.val.length ≤ out.val.length + 2 ⦄ := by
  unfold wow_text.push_safe
  split
  · rename_i hb
    have hp : b.bv = pipe := by rw [is_pipe_iff]; simp [hb]
    step*
    all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
    refine ⟨?_, by simp [*]⟩
    simp [r_post, out1_post, bytes, safeByte, hp, pipe, ch]
  · rename_i hb
    have hp : ¬ b.bv = pipe := by rw [is_pipe_iff]; scalar_tac
    step*
    refine ⟨?_, by simp [*]⟩
    simp [r_post, bytes, safeByte, hp]

def SafeInv (src : Slice U8) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ src.val.length ∧
  bytes st.1.val = (bytes (src.val.take st.2.val)).flatMap safeByte ∧
  st.1.val.length ≤ 2 * st.2.val

theorem chat_safe_loop_spec (src : Slice U8) (out : alloc.vec.Vec U8) (i : Usize)
    (hmax : src.val.length ≤ 2 ^ 20) (hinv : SafeInv src (out, i)) :
    wow_text.chat_safe_loop src out i ⦃ r =>
      bytes r.val = (bytes src.val).flatMap safeByte ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold wow_text.chat_safe_loop
  apply loop.spec_decr_nat (fun st => src.val.length - st.2.val) (SafeInv src) _ _ _ _ hinv
  rintro ⟨out, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold wow_text.chat_safe_loop.body
  step*
  · have hlt : i.val < src.val.length := by scalar_tac
    have ht : bytes (src.val.take (i.val + 1)) = bytes (src.val.take i.val) ++ [src.val[i.val].bv] := by
      unfold bytes
      rw [List.take_add_one, List.getElem?_eq_getElem hlt, List.map_append]
      rfl
    refine ⟨⟨by scalar_tac, ?_, by scalar_tac⟩, by scalar_tac⟩
    rw [out1_post1, hout, i3_post, ht, List.flatMap_append, i2_post]
    simp only [List.flatMap_cons, List.flatMap_nil, List.append_nil]
  · have hn : i.val = src.val.length := by scalar_tac
    rw [hn, List.take_length] at hout
    exact hout

/-- **S10**, first half: what `chat_safe` writes. -/
theorem chat_safe_bytes (t : Slice U8) (hmax : t.val.length ≤ 2 ^ 20) :
    wow_text.chat_safe t ⦃ v => bytes v.val = (bytes t.val).flatMap safeByte ⦄ := by
  unfold wow_text.chat_safe
  apply chat_safe_loop_spec t _ _ hmax
  simp [SafeInv, bytes]

theorem wowPlain_safe (s : List Spec.Byte) : wowPlain (s.flatMap safeByte) = some s := by
  induction s with
  | nil => simp [wowPlain]
  | cons b s ih =>
    by_cases hb : b = pipe
    · subst hb
      simp [safeByte, wowPlain, ih]
    · simp only [List.flatMap_cons, safeByte, hb, if_false, List.singleton_append]
      rw [wowPlain.eq_def]
      simp [hb, ih]

/-- **S10.** -/
theorem chat_safe_spec (t : Slice U8) (hmax : t.val.length ≤ 2 ^ 20) :
    wow_text.chat_safe t ⦃ v => wowPlain (bytes v.val) = some (bytes t.val) ⦄ := by
  apply WP.spec_mono (chat_safe_bytes t hmax)
  intro v hv
  rw [hv, wowPlain_safe]

end Protocol.WowText
