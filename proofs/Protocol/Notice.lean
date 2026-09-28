import Protocol.Ascii
import Protocol.WowText
import Protocol.Spec.Notice

/-! # The text of a notification (S40) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Notice

@[simp] theorem toNat_bv (b : U8) : b.bv.toNat = b.val := by simp

/-- The length of the UTF-8 sequence that a lead byte starts, or 0. -/
def seqLen (v : Nat) : Nat :=
  if v < 0x80 then 1 else if v < 0xC2 then 0 else if v < 0xE0 then 2
  else if v < 0xF0 then 3 else if v < 0xF5 then 4 else 0

@[step]
theorem sequence_len_spec (lead : U8) :
    notice.sequence_len lead ⦃ n => n.val = seqLen lead.val ⦄ := by
  unfold notice.sequence_len seqLen
  step*

@[step]
theorem is_continuation_spec (b : U8) :
    notice.is_continuation b ⦃ r => (r = true ↔ isCont b.bv) ⦄ := by
  unfold notice.is_continuation isCont
  step*

/-- The byte at `j` is there and continues a sequence. -/
def contAt (text : List U8) (j : Nat) : Prop := ∃ h : j < text.length, isCont (text[j]'h).bv

@[step]
theorem continues_at_spec (text : Slice U8) (j : Usize) :
    notice.continues_at text j ⦃ r => (r = true ↔ contAt text.val j.val) ⦄ := by
  unfold notice.continues_at contAt
  step*
  · rw [r_post, i1_post]
    constructor
    · intro h; exact ⟨by scalar_tac, h⟩
    · rintro ⟨_, h⟩; exact h

@[step]
theorem whole_at_spec (text : Slice U8) (i n : Usize) (hi : i.val < text.val.length)
    (hlen : text.val.length ≤ 2 ^ 20) :
    notice.whole_at text i n ⦃ r =>
      (r = true ↔ 2 ≤ n.val ∧ contAt text.val (i.val + 1) ∧ (n.val < 3 ∨ contAt text.val (i.val + 2)) ∧
        (n.val < 4 ∨ contAt text.val (i.val + 3))) ⦄ := by
  unfold notice.whole_at
  step*

@[step]
theorem in_range_spec (b low high : U8) :
    notice.in_range b low high ⦃ r => (r = true ↔ low.val ≤ b.val ∧ b.val ≤ high.val) ⦄ := by
  unfold notice.in_range
  step*

def hiddenTwo (a b : Nat) : Prop := (a = 0xC2 ∧ b < 0xA0) ∨ (a = 0xD8 ∧ b = 0x9C)

def hiddenThree (a b c : Nat) : Prop :=
  (a = 0xE1 ∧ b = 0xA0 ∧ c = 0x8E) ∨
  (a = 0xE2 ∧ b = 0x80 ∧ 0x8B ≤ c ∧ c ≤ 0x8F) ∨
  (a = 0xE2 ∧ b = 0x80 ∧ 0xA8 ≤ c ∧ c ≤ 0xAE) ∨
  (a = 0xE2 ∧ b = 0x81 ∧ 0xA0 ≤ c ∧ c ≤ 0xAF) ∨
  (a = 0xEF ∧ b = 0xBB ∧ c = 0xBF)

def hiddenFour (a b c : Nat) : Prop := a = 0xF3 ∧ b = 0xA0 ∧ (c = 0x80 ∨ c = 0x81)

@[step]
theorem is_hidden_two_spec (a b : U8) :
    notice.is_hidden_two a b ⦃ r => (r = true ↔ hiddenTwo a.val b.val) ⦄ := by
  unfold notice.is_hidden_two hiddenTwo
  step*

@[step]
theorem is_hidden_punctuation_spec (b c : U8) :
    notice.is_hidden_punctuation b c ⦃ r =>
      (r = true ↔ (b.val = 0x80 ∧ 0x8B ≤ c.val ∧ c.val ≤ 0x8F) ∨ (b.val = 0x80 ∧ 0xA8 ≤ c.val ∧ c.val ≤ 0xAE) ∨
        (b.val = 0x81 ∧ 0xA0 ≤ c.val ∧ c.val ≤ 0xAF)) ⦄ := by
  unfold notice.is_hidden_punctuation
  step*

@[step]
theorem is_hidden_three_spec (a b c : U8) :
    notice.is_hidden_three a b c ⦃ r => (r = true ↔ hiddenThree a.val b.val c.val) ⦄ := by
  unfold notice.is_hidden_three hiddenThree
  step*
  rw [r_post]
  simp [show (226#u8 : U8).val = 226 from rfl]

@[step]
theorem is_hidden_four_spec (a b c : U8) :
    notice.is_hidden_four a b c ⦃ r => (r = true ↔ hiddenFour a.val b.val c.val) ⦄ := by
  unfold notice.is_hidden_four hiddenFour
  step*

theorem drop_two (l : List U8) (i : Nat) (h : i + 2 ≤ l.length) :
    l.drop i = l[i] :: l[i + 1] :: l.drop (i + 2) := by
  rw [List.drop_eq_getElem_cons (by omega), List.drop_eq_getElem_cons (by omega)]
  rfl

theorem piece2 (l : List U8) (i : Nat) (h : i + 2 ≤ l.length) :
    (l.drop i).take 2 = [l[i], l[i + 1]] := by
  rw [drop_two l i h]; rfl

theorem piece3 (l : List U8) (i : Nat) (h : i + 3 ≤ l.length) :
    (l.drop i).take 3 = [l[i], l[i + 1], l[i + 2]] := by
  rw [drop_two l i (by omega), List.drop_eq_getElem_cons (by omega)]; rfl

theorem piece4 (l : List U8) (i : Nat) (h : i + 4 ≤ l.length) :
    (l.drop i).take 4 = [l[i], l[i + 1], l[i + 2], l[i + 3]] := by
  rw [drop_two l i (by omega), drop_two l (i + 2) (by omega)]; rfl

/-- The bytes of the sequence of `n` bytes at `i`. -/
def pieceAt (text : List U8) (i n : Nat) : List Spec.Byte := bytes ((text.drop i).take n)

@[step]
theorem is_hidden_spec (text : Slice U8) (i n : Usize) (h2 : 2 ≤ n.val) (h4 : n.val ≤ 4)
    (hin : i.val + n.val ≤ text.val.length) (hlen : text.val.length ≤ 2 ^ 20) :
    notice.is_hidden text i n ⦃ r => (r = true ↔ hiddenChar (pieceAt text.val i.val n.val)) ⦄ := by
  unfold notice.is_hidden
  step*
  · have hn : n.val = 2 := by simp [*]
    rw [r_post, pieceAt, show ((2#usize : Usize)).val = 2 from rfl, piece2 _ _ (by omega)]
    simp [hiddenChar, hiddenTwo, bytes, *]
    exact Iff.rfl
  · have hn : n.val = 3 := by simp [*]
    rw [r_post, pieceAt, show ((3#usize : Usize)).val = 3 from rfl, piece3 _ _ (by omega)]
    simp [hiddenChar, hiddenThree, bytes, *]
    exact Iff.rfl
  · have hn : n.val = 4 := by scalar_tac
    rw [r_post, pieceAt, hn, piece4 _ _ (by omega)]
    simp [hiddenChar, hiddenFour, bytes, *]

@[step]
theorem ascii_width_spec (b : U8) :
    notice.ascii_width b ⦃ w => w.val = if b.val = 124 then 2 else 1 ⦄ := by
  unfold notice.ascii_width
  step*

/-- What `push_ascii` writes for one byte below 0x80. -/
def asciiPiece (b : Spec.Byte) : List Spec.Byte :=
  if b = pipe then [pipe, pipe] else if b.toNat < 0x20 ∨ b.toNat = 0x7F then [ch ' '] else [b]

theorem is_pipe_iff (b : U8) : b.bv = pipe ↔ b.val = 124 := Protocol.WowText.is_pipe_iff b

theorem bv_124_iff (b : U8) : b.bv = 124#8 ↔ b.val = 124 := by
  have h := is_pipe_iff b
  simpa [pipe, ch] using h

@[step]
theorem push_ascii_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 2 ≤ Usize.max) :
    notice.push_ascii out b ⦃ r =>
      bytes r.val = bytes out.val ++ asciiPiece b.bv ∧
      r.val.length = out.val.length + (if b.val = 124 then 2 else 1) ⦄ := by
  unfold notice.push_ascii asciiPiece
  have h124 := bv_124_iff b
  step*
  · scalar_tac
  · have hv : b.val = 124 := by simp [*]
    simp [bytes, *, pipe, ch]
  · have hv : ¬ b.val = 124 := by scalar_tac
    have hlt : b.val < 32 := by scalar_tac
    simp [bytes, *, pipe, ch]
  · have hv : ¬ b.val = 124 := by scalar_tac
    have h7 : b.val = 127 := by scalar_tac
    simp [bytes, *, pipe, ch]
  · have hv : ¬ b.val = 124 := by scalar_tac
    have hlt : ¬ (b.val < 32 ∨ b.val = 127) := by scalar_tac
    simp [bytes, *, pipe, ch]

/-! ## Pieces: what the loop writes for each character -/

/-- A doubled `|`, or one visible character that is not `|`. -/
def goodPiece (p : List Spec.Byte) : Prop := p = [pipe, pipe] ∨ (visibleChar p ∧ p ≠ [pipe])

def Good (v : List Spec.Byte) : Prop :=
  ∃ ps : List (List Spec.Byte), v = ps.flatten ∧ ∀ p ∈ ps, goodPiece p

theorem good_nil : Good [] := ⟨[], rfl, by simp⟩

theorem good_append (v p : List Spec.Byte) (hv : Good v) (hp : goodPiece p) : Good (v ++ p) := by
  obtain ⟨ps, rfl, hps⟩ := hv
  refine ⟨ps ++ [p], by simp, ?_⟩
  intro q hq
  rw [List.mem_append, List.mem_singleton] at hq
  rcases hq with hq | rfl
  · exact hps q hq
  · exact hp

theorem control_length (p : List Spec.Byte) (h : controlChar p) : p.length = 1 := by
  match p with
  | [] => simp [controlChar] at h
  | [_] => rfl
  | _ :: _ :: _ => simp [controlChar] at h

theorem asciiPiece_good (b : Spec.Byte) (h : b.toNat < 0x80) : goodPiece (asciiPiece b) := by
  unfold asciiPiece goodPiece
  split_ifs with h1 h2
  · left; rfl
  · right
    refine ⟨⟨by simp [wholeChar, ch], by simp [controlChar, ch], by simp [hiddenChar]⟩, ?_⟩
    simp [pipe, ch]
  · right
    refine ⟨⟨by simpa [wholeChar] using h, by simpa [controlChar] using h2, by simp [hiddenChar]⟩, ?_⟩
    simpa using h1

theorem multi_good (p : List Spec.Byte) (hw : wholeChar p) (hl : 2 ≤ p.length) (hh : ¬ hiddenChar p) :
    goodPiece p := by
  right
  refine ⟨⟨hw, fun hc => by have := control_length p hc; omega, hh⟩, ?_⟩
  intro hp; rw [hp] at hl; simp at hl

theorem seqLen_two (v : Nat) (h : seqLen v = 2) : 0xC2 ≤ v ∧ v < 0xE0 := by
  unfold seqLen at h; split_ifs at h <;> omega

theorem seqLen_three (v : Nat) (h : seqLen v = 3) : 0xE0 ≤ v ∧ v < 0xF0 := by
  unfold seqLen at h; split_ifs at h <;> omega

theorem seqLen_four (v : Nat) (h : seqLen v = 4) : 0xF0 ≤ v ∧ v < 0xF5 := by
  unfold seqLen at h; split_ifs at h <;> omega

theorem seqLen_le (v : Nat) : seqLen v ≤ 4 := by
  unfold seqLen; split_ifs <;> omega

theorem contAt_cont (text : List U8) (j : Nat) (h : contAt text j) (hj : j < text.length) :
    isCont (text[j]).bv := by
  obtain ⟨_, hc⟩ := h; exact hc

theorem piece_whole (text : List U8) (i n : Nat) (hi : i < text.length) (hn : seqLen (text[i]).val = n)
    (h2 : 2 ≤ n) (hc1 : contAt text (i + 1)) (hc2 : n < 3 ∨ contAt text (i + 2))
    (hc3 : n < 4 ∨ contAt text (i + 3)) :
    i + n ≤ text.length ∧ wholeChar (pieceAt text i n) := by
  have h4 := seqLen_le (text[i]).val
  obtain ⟨h1, hb1⟩ := hc1
  rcases (by omega : n = 2 ∨ n = 3 ∨ n = 4) with rfl | rfl | rfl
  · refine ⟨by omega, ?_⟩
    rw [pieceAt, piece2 _ _ (by omega)]
    have := seqLen_two _ hn
    simp only [bytes, List.map_cons, List.map_nil, wholeChar, toNat_bv]
    exact ⟨this.1, this.2, hb1⟩
  · obtain ⟨h2', hb2⟩ := hc2.resolve_left (by omega)
    refine ⟨by omega, ?_⟩
    rw [pieceAt, piece3 _ _ (by omega)]
    have := seqLen_three _ hn
    simp only [bytes, List.map_cons, List.map_nil, wholeChar, toNat_bv]
    exact ⟨this.1, this.2, hb1, hb2⟩
  · obtain ⟨h2', hb2⟩ := hc2.resolve_left (by omega)
    obtain ⟨h3', hb3⟩ := hc3.resolve_left (by omega)
    refine ⟨by omega, ?_⟩
    rw [pieceAt, piece4 _ _ (by omega)]
    have := seqLen_four _ hn
    simp only [bytes, List.map_cons, List.map_nil, wholeChar, toNat_bv]
    exact ⟨this.1, this.2, hb1, hb2, hb3⟩

theorem pieceAt_length (text : List U8) (i n : Nat) (h : i + n ≤ text.length) :
    (pieceAt text i n).length = n := by
  simp [pieceAt, bytes]; omega

theorem ascii_of_seqLen_one (v : Nat) (h : seqLen v = 1) : v < 0x80 := by
  unfold seqLen at h; split_ifs at h <;> omega

/-! ## The loop -/

def TextInv (text : Slice U8) (max : Nat) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ text.val.length ∧ st.1.val.length ≤ max ∧ st.1.val.length ≤ 2 * st.2.val ∧
    Good (bytes st.1.val)

theorem notice_text_loop_spec (text : Slice U8) (max : Usize) (out : alloc.vec.Vec U8) (i : Usize)
    (hlen : text.val.length ≤ 2 ^ 20) (hinv : TextInv text max.val (out, i)) :
    notice.notice_text_loop text max out i ⦃ r => r.val.length ≤ max.val ∧ Good (bytes r.val) ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold notice.notice_text_loop
  apply loop.spec_decr_nat (fun st => text.val.length - st.2.val) (TextInv text max.val) _ _ _ _ hinv
  rintro ⟨o, i⟩ ⟨hi, hmax, hdouble, hgood⟩
  simp only at hi hmax hdouble hgood
  unfold notice.notice_text_loop.body
  step*
  all_goals try (have hn4 : n.val ≤ 4 := by rw [n_post]; exact seqLen_le _)
  all_goals try (have hi4 : i4.val ≤ 2 := by rw [i4_post]; split <;> omega)
  all_goals try (
    have hb : b = true := by assumption
    obtain ⟨h2, hc1, hc2, hc3⟩ := b_post.mp hb
    obtain ⟨hin, hw⟩ := piece_whole text.val i.val n.val (by scalar_tac) (by simp [n_post, i2_post])
      h2 hc1 hc2 hc3)
  all_goals try (
    have hn1 : n = 1#usize := by assumption
    have hascii : i2.val < 0x80 := ascii_of_seqLen_one _ (by rw [← n_post]; simp [hn1]))
  all_goals first
    | scalar_tac
    | (refine ⟨⟨by scalar_tac, by scalar_tac, by scalar_tac, hgood⟩, by scalar_tac⟩)
    | (refine ⟨⟨by scalar_tac, by scalar_tac, by scalar_tac, ?_⟩, by scalar_tac⟩
       rw [out1_post1]
       exact good_append _ _ hgood (asciiPiece_good _ (by rw [toNat_bv]; exact hascii)))
    | skip
  have hhid : ¬ hiddenChar (pieceAt text.val i.val n.val) := by rw [← b1_post]; assumption
  have hlen1 : out1.val.length ≤ 2 * i5.val := by simp [out1_post]; scalar_tac
  refine ⟨⟨by scalar_tac, by simp [out1_post]; scalar_tac, hlen1, ?_⟩, by scalar_tac⟩
  have hp : goodPiece (pieceAt text.val i.val n.val) :=
    multi_good _ hw (by rw [pieceAt_length _ _ _ hin]; exact h2) hhid
  rw [out1_post, show i5.val - i.val = n.val by scalar_tac, bytes, List.map_append]
  exact good_append _ _ hgood hp

/-! ## From pieces to the three properties -/

theorem pipe_visible : visibleChar [pipe] := by
  refine ⟨by simp [wholeChar, pipe, ch], by simp [controlChar, pipe, ch], by simp [hiddenChar]⟩

theorem good_safe (ps : List (List Spec.Byte)) (h : ∀ p ∈ ps, goodPiece p) : noticeSafe ps.flatten := by
  induction ps with
  | nil => exact ⟨[], rfl, by simp⟩
  | cons p rest ih =>
    obtain ⟨qs, hq, hqs⟩ := ih (fun q hq => h q (List.mem_cons_of_mem _ hq))
    rcases h p List.mem_cons_self with rfl | ⟨hv, _⟩
    · refine ⟨[pipe] :: [pipe] :: qs, by simp [hq], ?_⟩
      intro q hq'
      simp only [List.mem_cons] at hq'
      rcases hq' with rfl | rfl | hq'
      · exact pipe_visible
      · exact pipe_visible
      · exact hqs q hq'
    · refine ⟨p :: qs, by simp [hq], ?_⟩
      intro q hq'
      rcases List.mem_cons.mp hq' with rfl | hq'
      · exact hv
      · exact hqs q hq'

theorem safe_ends (v : List Spec.Byte) (h : noticeSafe v) : endsOnChar v := by
  obtain ⟨ps, rfl, hps⟩ := h
  exact ⟨ps, rfl, fun p hp => (hps p hp).1⟩

theorem wowPlain_append (xs rest : List Spec.Byte) (h : ∀ b ∈ xs, b ≠ pipe) (plain : List Spec.Byte)
    (hr : wowPlain rest = some plain) : wowPlain (xs ++ rest) = some (xs ++ plain) := by
  induction xs with
  | nil => simpa using hr
  | cons b xs ih =>
    have hb : b ≠ pipe := h b List.mem_cons_self
    have ih' := ih (fun c hc => h c (List.mem_cons_of_mem _ hc))
    rw [List.cons_append, wowPlain.eq_def]
    simp [hb, ih']

theorem no_pipe (p : List Spec.Byte) (hv : visibleChar p) (hne : p ≠ [pipe]) : ∀ b ∈ p, b ≠ pipe := by
  obtain ⟨hw, _, _⟩ := hv
  have hpipe : pipe.toNat = 124 := by simp [pipe, ch]
  intro b hb hbp
  rw [hbp] at hb
  match p, hw with
  | [a], _ => simp at hb; exact hne (by rw [hb])
  | [a, c], hw =>
    simp only [List.mem_cons, List.not_mem_nil, or_false] at hb
    obtain ⟨h1, _, h3, _⟩ := hw
    rcases hb with rfl | rfl <;> simp_all
  | [a, c, d], hw =>
    simp only [List.mem_cons, List.not_mem_nil, or_false] at hb
    obtain ⟨h1, _, ⟨h3, _⟩, ⟨h4, _⟩⟩ := hw
    rcases hb with rfl | rfl | rfl <;> omega
  | [a, c, d, e], hw =>
    simp only [List.mem_cons, List.not_mem_nil, or_false] at hb
    obtain ⟨h1, _, ⟨h3, _⟩, ⟨h4, _⟩, ⟨h5, _⟩⟩ := hw
    rcases hb with rfl | rfl | rfl | rfl <;> omega

theorem good_doubled (ps : List (List Spec.Byte)) (h : ∀ p ∈ ps, goodPiece p) : pipesDoubled ps.flatten := by
  induction ps with
  | nil => exact ⟨[], by simp [wowPlain]⟩
  | cons p rest ih =>
    obtain ⟨plain, hplain⟩ := ih (fun q hq => h q (List.mem_cons_of_mem _ hq))
    rcases h p List.mem_cons_self with rfl | ⟨hv, hne⟩
    · refine ⟨pipe :: plain, ?_⟩
      simp [wowPlain, hplain]
    · exact ⟨p ++ plain, by simpa using wowPlain_append p _ (no_pipe p hv hne) plain hplain⟩

/-- **S40.** -/
theorem notice_text_spec (t : Slice U8) (max : Usize) (hlen : t.val.length ≤ 2 ^ 20) :
    notice.notice_text t max ⦃ v =>
      v.val.length ≤ max.val ∧ noticeSafe (bytes v.val) ∧ pipesDoubled (bytes v.val) ∧
        endsOnChar (bytes v.val) ⦄ := by
  unfold notice.notice_text
  apply WP.spec_mono (notice_text_loop_spec t max _ _ hlen ⟨by simp, by simp, by simp, by simpa [bytes] using good_nil⟩)
  rintro v ⟨hmax, ps, hps, hgood⟩
  rw [hps]
  exact ⟨hmax, good_safe ps hgood, good_doubled ps hgood, safe_ends _ (good_safe ps hgood)⟩

end Protocol.Notice
