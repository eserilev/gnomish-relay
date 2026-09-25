import Protocol.Ascii
import Protocol.Spec.Markdown

/-! # Inline spans (S22, S24, S25)

Every writer here appends a piece `w` to `out`. Its spec says that `w` reads as a
`Text` in tokens, and how long `w` is. -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Inline

/-- `step*` stops at `let x ← if c then a else b`. This moves the rest into each branch. -/
theorem ite_bind {α β : Type} (c : Prop) [Decidable c] (a b : Std.Result α) (k : α → Std.Result β) :
    ((if c then a else b) >>= k) = if c then a >>= k else b >>= k := by
  split <;> rfl

theorem ite_std_bind {α β : Type} (c : Prop) [Decidable c] (a b : Std.Result α)
    (k : α → Std.Result β) :
    Std.bind (if c then a else b) k = if c then Std.bind a k else Std.bind b k := by
  split <;> rfl

/-- `step*`, and then again inside each `let x ← if ...` that it stops at. -/
macro "step_ite" : tactic =>
  `(tactic| (step*
             all_goals (try (simp only [ite_bind, bind_assoc]; step*))
             all_goals (try (simp only [ite_bind, bind_assoc]; step*))))

/-! ## The grammar of texts -/

theorem text_append {html : Bool} {o o1 o2 : Bool} {t u : List Spec.Byte}
    (ht : Text html o t o1) (hu : Text html o1 u o2) : Text html o (t ++ u) o2 := by
  induction ht with
  | nil => exact hu
  | plain hb _ ih => exact Text.plain hb (ih hu)
  | pipes _ ih => exact Text.pipes (ih hu)
  | entity hh he _ ih => rw [List.append_assoc]; exact Text.entity hh he (ih hu)
  | color hc _ ih => rw [List.append_assoc]; exact Text.color hc (ih hu)
  | reset _ ih => rw [List.append_assoc]; exact Text.reset (ih hu)

theorem Text.one {html : Bool} {o : Bool} {b : Spec.Byte} (hb : plainByte html b) :
    Text html o [b] o := Text.plain hb (Text.nil o)

def isHtml : inline.Escape → Bool
  | .Html => true
  | .Wow => false

/-- A piece that keeps the color state as it is: the text between marks. -/
def Piece (html : Bool) (w : List Spec.Byte) : Prop := ∀ o, Text html o w o

theorem Piece.nil (html : Bool) : Piece html [] := fun o => Text.nil o

theorem Piece.append {html : Bool} {w v : List Spec.Byte} (hw : Piece html w) (hv : Piece html v) :
    Piece html (w ++ v) := fun o => text_append (hw o) (hv o)

theorem plainByte_iff (html : Bool) (b : U8) :
    plainByte html b.bv ↔
      (32 ≤ b.val ∧ b.val ≠ 127 ∧ b.val ≠ 124 ∧
        (html = true → b.val ≠ 60 ∧ b.val ≠ 62 ∧ b.val ≠ 38)) := by
  simp only [plainByte, fieldByte, pipe, ch, BitVec.le_def, ne_eq, ← BitVec.toNat_inj]
  simp

theorem bytes_append (a b : List U8) : bytes (a ++ b) = bytes a ++ bytes b := by
  simp [bytes]

theorem len_of_bytes {a b : List U8} {x : List Spec.Byte} (h : bytes a = bytes b ++ x) :
    a.length = b.length + x.length := by
  have := congrArg List.length h
  simpa [bytes] using this

/-! ## Constants -/

theorem lt_bytes : bytes (Array.to_slice inline.LT).val = ascii "&lt;" := by
  unfold inline.LT; rfl
theorem gt_bytes : bytes (Array.to_slice inline.GT).val = ascii "&gt;" := by
  unfold inline.GT; rfl
theorem amp_bytes : bytes (Array.to_slice inline.AMP).val = ascii "&amp;" := by
  unfold inline.AMP; rfl
theorem tab_bytes : bytes (Array.to_slice inline.TAB).val = ascii "    " := by
  unfold inline.TAB; rfl
theorem reset_bytes : bytes (Array.to_slice inline.RESET).val = resetCode := by
  unfold inline.RESET; rfl
theorem bold_bytes : bytes (Array.to_slice inline.BOLD).val = boldCode := by
  unfold inline.BOLD; rfl
theorem italic_bytes : bytes (Array.to_slice inline.ITALIC).val = italicCode := by
  unfold inline.ITALIC; rfl
theorem bold_italic_bytes : bytes (Array.to_slice inline.BOLD_ITALIC).val = boldItalicCode := by
  unfold inline.BOLD_ITALIC; rfl
theorem code_bytes : bytes (Array.to_slice inline.CODE).val = codeCode := by
  unfold inline.CODE; rfl
theorem link_bytes : bytes (Array.to_slice inline.LINK).val = linkCode := by
  unfold inline.LINK; rfl

@[simp, scalar_tac_simps] theorem lt_length : (Array.to_slice inline.LT).val.length = 4 := by
  unfold inline.LT; rfl
@[simp, scalar_tac_simps] theorem gt_length : (Array.to_slice inline.GT).val.length = 4 := by
  unfold inline.GT; rfl
@[simp, scalar_tac_simps] theorem amp_length : (Array.to_slice inline.AMP).val.length = 5 := by
  unfold inline.AMP; rfl
@[simp, scalar_tac_simps] theorem tab_length : (Array.to_slice inline.TAB).val.length = 4 := by
  unfold inline.TAB; rfl
@[simp, scalar_tac_simps] theorem reset_length : (Array.to_slice inline.RESET).val.length = 2 := by
  unfold inline.RESET; rfl

theorem entity_piece (e : List Spec.Byte) (he : e ∈ entities) : Piece true e :=
  fun o => by simpa using Text.entity (t := []) rfl he (Text.nil o)

theorem spaces_piece (html : Bool) : Piece html (ascii "    ") := by
  have hs : plainByte html (ch ' ') := by simp [plainByte, fieldByte, pipe, ch]
  intro o
  exact Text.plain hs (Text.plain hs (Text.plain hs (Text.one hs)))

theorem pipes_piece (html : Bool) : Piece html [pipe, pipe] :=
  fun o => Text.pipes (Text.nil o)

/-! ## Bytes -/

@[step]
theorem is_control_spec (b : U8) : inline.is_control b ⦃ r => (r = true ↔ (b.val < 32 ∨ b.val = 127)) ⦄ := by
  unfold inline.is_control
  step*

/-- What a writer of one text byte promises: at most `k` bytes, all in one piece. -/
def Wrote (html : Bool) (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length ≤ k ∧ Piece html w

theorem wrote_push (html : Bool) (k : Nat) (out : alloc.vec.Vec U8) (b : U8) (r : alloc.vec.Vec U8)
    (hr : r.val = out.val ++ [b]) (hk : 1 ≤ k) (hb : plainByte html b.bv) : Wrote html k out r :=
  ⟨[b.bv], by simp [hr, bytes], by simpa using hk, fun _ => Text.one hb⟩

theorem wrote_bytes (html : Bool) (k : Nat) (out : alloc.vec.Vec U8) (s : Slice U8) (r : alloc.vec.Vec U8)
    (hr : r.val = out.val ++ s.val) (hk : s.val.length ≤ k) (hp : Piece html (bytes s.val)) :
    Wrote html k out r :=
  ⟨bytes s.val, by simp [hr, bytes], by simpa [bytes] using hk, hp⟩

theorem wl {html : Bool} {k : Nat} {out r : alloc.vec.Vec U8} (h : Wrote html k out r) :
    Wrote html k out r ∧ r.val.length ≤ out.val.length + k := by
  refine ⟨h, ?_⟩
  obtain ⟨w, hw, hl, _⟩ := h
  have := len_of_bytes hw
  omega

@[step]
theorem push_html_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 5 ≤ Usize.max)
    (hb : 32 ≤ b.val ∧ b.val ≠ 127 ∧ b.val ≠ 124) :
    inline.push_html out b ⦃ r => Wrote true 5 out r ∧ r.val.length ≤ out.val.length + 5 ⦄ := by
  unfold inline.push_html
  step*
  · subst s_post
    exact wl <| wrote_bytes _ _ _ _ _ r_post1 (by simp) (by
      rw [lt_bytes]; exact entity_piece _ (by simp [entities]))
  · subst s_post
    exact wl <| wrote_bytes _ _ _ _ _ r_post1 (by simp) (by
      rw [gt_bytes]; exact entity_piece _ (by simp [entities]))
  · subst s_post
    exact wl <| wrote_bytes _ _ _ _ _ r_post1 (by simp) (by
      rw [amp_bytes]; exact entity_piece _ (by simp [entities]))
  · exact wl <| wrote_push _ _ _ _ _ r_post (by omega) (by rw [plainByte_iff]; scalar_tac)

@[step]
theorem escape_eq_spec (a b : inline.Escape) :
    inline.Escape.Insts.CoreCmpPartialEqEscape.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;> simp [inline.Escape.Insts.CoreCmpPartialEqEscape.eq, inline.Escape.read_discriminant]

@[step]
theorem mark_eq_spec (a b : inline.Mark) :
    inline.Mark.Insts.CoreCmpPartialEqMark.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;> simp [inline.Mark.Insts.CoreCmpPartialEqMark.eq, inline.Mark.read_discriminant]

@[step]
theorem push_visible_spec (out : alloc.vec.Vec U8) (b : U8) (escape : inline.Escape)
    (hroom : out.val.length + 5 ≤ Usize.max) (hb : 32 ≤ b.val ∧ b.val ≠ 127 ∧ b.val ≠ 124) :
    inline.push_visible out b escape ⦃ r => Wrote (isHtml escape) 5 out r ∧ r.val.length ≤ out.val.length + 5 ⦄ := by
  unfold inline.push_visible
  step*
  · have : escape = .Html := b1_post.mp ‹_›
    subst this
    exact ⟨r_post1, r_post2⟩
  · have : isHtml escape = false := by
      induction escape
      · exact absurd (b1_post.mpr rfl) ‹_›
      · rfl
    exact wl <| wrote_push _ _ _ _ _ r_post (by omega) (by rw [plainByte_iff, this]; scalar_tac)

theorem wrote_mono {html : Bool} {k k' : Nat} {out r : alloc.vec.Vec U8} (h : Wrote html k out r)
    (hk : k ≤ k') : Wrote html k' out r := by
  obtain ⟨w, hw, hl, hp⟩ := h
  exact ⟨w, hw, by omega, hp⟩

@[step]
theorem push_text_byte_spec (out : alloc.vec.Vec U8) (b : U8) (escape : inline.Escape)
    (hroom : out.val.length + 5 ≤ Usize.max) :
    inline.push_text_byte out b escape ⦃ r => Wrote (isHtml escape) 5 out r ∧ r.val.length ≤ out.val.length + 5 ⦄ := by
  unfold inline.push_text_byte
  step*
  · simp [out1_post]; omega
  · refine wl ⟨[pipe, pipe], ?_, by simp, pipes_piece _⟩
    simp only [r_post, out1_post, bytes, List.map_append, List.map_cons, List.map_nil,
      List.append_assoc, List.cons_append, List.nil_append]
    rfl
  · exact wl <| wrote_push _ _ _ _ _ r_post (by omega) (by rw [plainByte_iff]; simp)
  · exact wl ⟨[], by simp, by simp, Piece.nil _⟩

@[step]
theorem push_code_byte_spec (out : alloc.vec.Vec U8) (b : U8)
    (hroom : out.val.length + 5 ≤ Usize.max) :
    inline.push_code_byte out b ⦃ r => Wrote false 5 out r ∧ r.val.length ≤ out.val.length + 5 ⦄ := by
  unfold inline.push_code_byte
  step*
  · subst s_post
    exact wl <| wrote_bytes _ _ _ _ _ r_post1 (by simp) (by rw [tab_bytes]; exact spaces_piece _)
  · exact ⟨by simpa [isHtml] using r_post1, r_post2⟩

theorem piece_of_wrote {html : Bool} {k : Nat} {out r : alloc.vec.Vec U8} (h : Wrote html k out r) :
    ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length ≤ k ∧ Piece html w := h

def RangeInv (html : Bool) (out0 : alloc.vec.Vec U8) (start stop : Nat)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  start ≤ st.2.val ∧ st.2.val ≤ max start stop ∧ Wrote html (5 * (st.2.val - start)) out0 st.1

@[step]
theorem push_text_range_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop : Usize)
    (escape : inline.Escape) (hstop : stop.val ≤ md.val.length)
    (hroom : out.val.length + 5 * (stop.val - start.val) ≤ Usize.max) :
    inline.push_text_range out md start stop escape ⦃ r =>
      Wrote (isHtml escape) (5 * (stop.val - start.val)) out r ∧
      r.val.length ≤ out.val.length + 5 * (stop.val - start.val) ⦄ := by
  unfold inline.push_text_range inline.push_text_range_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val) (RangeInv (isHtml escape) out start.val stop.val)
    _ _ _ _ ⟨le_refl _, by simp, ⟨[], by simp, by simp, Piece.nil _⟩⟩
  rintro ⟨cur, i⟩ ⟨hs, hi, w, hw, hl, hp⟩
  simp only at hs hi hw hl
  have hcur := len_of_bytes hw
  unfold inline.push_text_range_loop.body
  step*
  · obtain ⟨v, hv, hvl, hvp⟩ := out1_post1
    refine ⟨⟨by scalar_tac, by scalar_tac, w ++ v, by rw [hv, hw, List.append_assoc], ?_,
      hp.append hvp⟩, by scalar_tac⟩
    simp only [List.length_append]
    have : i2.val = i.val + 1 := by scalar_tac
    rw [this]
    omega
  · exact wl (wrote_mono ⟨w, hw, hl, hp⟩ (by scalar_tac))

def styleOpen (s : inline.Style) : Bool :=
  match s.bold, s.italic with
  | .Off, .Off => false
  | _, _ => true

/-- The `|r` that a style still owes at the end of a text. -/
def owed (s : inline.Style) : Nat := if styleOpen s then 2 else 0

theorem owed_le (s : inline.Style) : owed s ≤ 2 := by unfold owed; split <;> omega

theorem color_text (html : Bool) (c : List Spec.Byte) (hc : c ∈ colorCodes) :
    Text html false c true := by
  simpa using Text.color (html := html) (t := []) hc (Text.nil true)

theorem reset_text (html : Bool) : Text html true resetCode false := by
  simpa using Text.reset (html := html) (t := []) (Text.nil false)

def Opened (out r : alloc.vec.Vec U8) (s : inline.Style) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧
    w.length = (if styleOpen s then 10 else 0) ∧ ∀ html, Text html false w (styleOpen s)

def Closed (out r : alloc.vec.Vec U8) (s : inline.Style) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length = owed s ∧ ∀ html, Text html (styleOpen s) w false

theorem opened_len {out r : alloc.vec.Vec U8} {s : inline.Style} (h : Opened out r s) :
    Opened out r s ∧ r.val.length ≤ out.val.length + 10 := by
  refine ⟨h, ?_⟩
  obtain ⟨w, hw, hl, _⟩ := h
  have := len_of_bytes hw
  split at hl <;> omega

theorem closed_len {out r : alloc.vec.Vec U8} {s : inline.Style} (h : Closed out r s) :
    Closed out r s ∧ r.val.length ≤ out.val.length + 2 := by
  refine ⟨h, ?_⟩
  obtain ⟨w, hw, hl, _⟩ := h
  have := len_of_bytes hw
  have := owed_le s
  omega

@[step]
theorem open_color_spec (out : alloc.vec.Vec U8) (s : inline.Style)
    (hroom : out.val.length + 10 ≤ Usize.max) :
    inline.open_color out s ⦃ r => Opened out r s ∧ r.val.length ≤ out.val.length + 10 ⦄ := by
  obtain ⟨bold, italic⟩ := s
  unfold inline.open_color
  induction bold <;> induction italic <;> step* <;> apply opened_len
  · exact ⟨[], by simp, by simp [styleOpen], fun _ => Text.nil _⟩
  · subst s_post
    refine ⟨_, by rw [r_post1, bytes_append, italic_bytes], by rfl, fun html => ?_⟩
    simp only [styleOpen]
    exact color_text _ _ (by simp [colorCodes])
  · subst s_post
    refine ⟨_, by rw [r_post1, bytes_append, bold_bytes], by rfl, fun html => ?_⟩
    simp only [styleOpen]
    exact color_text _ _ (by simp [colorCodes])
  · subst s_post
    refine ⟨_, by rw [r_post1, bytes_append, bold_italic_bytes], by rfl, fun html => ?_⟩
    simp only [styleOpen]
    exact color_text _ _ (by simp [colorCodes])

@[step]
theorem close_color_spec (out : alloc.vec.Vec U8) (s : inline.Style)
    (hroom : out.val.length + 2 ≤ Usize.max) :
    inline.close_color out s ⦃ r => Closed out r s ∧ r.val.length ≤ out.val.length + 2 ⦄ := by
  obtain ⟨bold, italic⟩ := s
  unfold inline.close_color
  induction bold <;> induction italic <;> step* <;> apply closed_len
  · exact ⟨[], by simp, by simp [owed, styleOpen], fun _ => Text.nil _⟩
  all_goals
    subst s_post
    exact ⟨_, by rw [r_post1, bytes_append, reset_bytes], by rfl, fun html => reset_text html⟩

/-! ## Scanning -/

@[step] theorem is_space_spec (b : U8) : inline.is_space b ⦃ _ => True ⦄ := by
  unfold inline.is_space; step*
@[step] theorem is_alnum_spec (b : U8) : inline.is_alnum b ⦃ _ => True ⦄ := by
  unfold inline.is_alnum; step*
@[step] theorem is_punct_spec (b : U8) : inline.is_punct b ⦃ _ => True ⦄ := by
  unfold inline.is_punct; step*
@[step] theorem flip_spec (m : inline.Mark) : inline.flip m ⦃ _ => True ⦄ := by
  unfold inline.flip; induction m <;> step*
@[step] theorem toggled_spec (s : inline.Style) (n : Usize) : inline.toggled s n ⦃ _ => True ⦄ := by
  unfold inline.toggled; step*
@[step] theorem opens_spec (s : inline.Style) (n : Usize) : inline.opens s n ⦃ _ => True ⦄ := by
  unfold inline.opens; step*
@[step] theorem closes_spec (s : inline.Style) (n : Usize) : inline.closes s n ⦃ _ => True ⦄ := by
  unfold inline.closes; step*
@[step] theorem min3_spec (n : Usize) : inline.min3 n ⦃ r => r.val ≤ n.val ∧ r.val ≤ 3 ∧ (1 ≤ n.val → 1 ≤ r.val) ⦄ := by
  unfold inline.min3; step*

@[step]
theorem byte_before_spec (md : Slice U8) (start i : Usize) (hi : i.val ≤ md.val.length) :
    inline.byte_before md start i ⦃ _ => True ⦄ := by
  unfold inline.byte_before; step*

@[step]
theorem byte_at_spec (md : Slice U8) (stop i : Usize) (hstop : stop.val ≤ md.val.length) :
    inline.byte_at md stop i ⦃ _ => True ⦄ := by
  unfold inline.byte_at; step*

@[step]
theorem can_open_spec (md : Slice U8) (start stop i n : Usize) (hi : i.val < md.val.length)
    (hstop : stop.val ≤ md.val.length) (hn : i.val + n.val ≤ Usize.max) :
    inline.can_open md start stop i n ⦃ _ => True ⦄ := by
  unfold inline.can_open
  step*
  by_cases h : i2 = 95#u8 <;> simp only [h, if_true, if_false] <;> step*

@[step]
theorem can_close_spec (md : Slice U8) (start stop i n : Usize) (hi : i.val < md.val.length)
    (hstop : stop.val ≤ md.val.length) (hn : i.val + n.val ≤ Usize.max) :
    inline.can_close md start stop i n ⦃ _ => True ⦄ := by
  unfold inline.can_close
  step*
  by_cases h : i1 = 95#u8 <;> simp only [h, if_true, if_false] <;> step*

@[step]
theorem run_len_spec (md : Slice U8) (i stop : Usize) (b : U8) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    inline.run_len md i stop b ⦃ r => i.val + r.val ≤ stop.val ∧
      (∀ h : i.val < md.val.length, i.val < stop.val → md.val[i.val] = b → 1 ≤ r.val) ⦄ := by
  have hloop : inline.run_len_loop md stop b i ⦃ j => i.val ≤ j.val ∧ j.val ≤ stop.val ∧
      (∀ h : i.val < md.val.length, i.val < stop.val → md.val[i.val] = b → i.val < j.val) ⦄ := by
    unfold inline.run_len_loop
    apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => i.val ≤ j.val ∧ j.val ≤ stop.val)
      _ _ _ _ ⟨le_refl _, hi⟩
    rintro j ⟨hij, hj⟩
    unfold inline.run_len_loop.body
    step*
  unfold inline.run_len
  step with hloop as ⟨j, hj1, hj2, hj3⟩
  step*
  exact ⟨by omega, fun h h1 h2 => by have := hj3 h h1 h2; omega⟩

@[step]
theorem find_tick_run_spec (md : Slice U8) (start stop n : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    inline.find_tick_run md start stop n ⦃ r =>
      r.val = stop.val ∨ (start.val ≤ r.val ∧ r.val < stop.val ∧ r.val + n.val ≤ stop.val) ⦄ := by
  unfold inline.find_tick_run inline.find_tick_run_loop
  apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => start.val ≤ j.val ∧ j.val ≤ stop.val)
    _ _ _ _ ⟨le_refl _, hs⟩
  rintro j ⟨hsj, hj⟩
  unfold inline.find_tick_run_loop.body
  step*
  by_cases h : len = 0#usize <;> simp only [h, if_true, if_false] <;> step*
  scalar_tac

@[step]
theorem find_byte_spec (md : Slice U8) (start stop : Usize) (b : U8)
    (hstop : stop.val ≤ md.val.length) :
    inline.find_byte md start stop b ⦃ r =>
      start.val ≤ r.val ∧ (start.val ≤ stop.val → r.val ≤ stop.val) ⦄ := by
  unfold inline.find_byte inline.find_byte_loop
  apply loop.spec_decr_nat (fun j => stop.val - j.val)
    (fun j => start.val ≤ j.val ∧ (start.val ≤ stop.val → j.val ≤ stop.val))
    _ _ _ _ ⟨le_refl _, id⟩
  rintro j ⟨hsj, hj⟩
  unfold inline.find_byte_loop.body
  step*

@[step]
theorem has_closer_spec (md : Slice U8) (start stop i n : Usize) (hmd : md.val.length ≤ 2 ^ 20)
    (hi : i.val < md.val.length) (hn : i.val + n.val ≤ stop.val) (hstop : stop.val ≤ md.val.length) :
    inline.has_closer md start stop i n ⦃ _ => True ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold inline.has_closer inline.has_closer_loop
  step*
  apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => j.val ≤ stop.val)
    _ _ _ _ (by scalar_tac)
  intro j hj
  unfold inline.has_closer_loop.body
  step*

@[step]
theorem toggles_spec (md : Slice U8) (start stop i n : Usize) (s : inline.Style)
    (hmd : md.val.length ≤ 2 ^ 20) (hi : i.val < md.val.length) (hn : i.val + n.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    inline.toggles md start stop i n s ⦃ _ => True ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold inline.toggles
  step*

/-! ## Spans -/

/-- A writer that moves the style from `s` to `s'` while it reads `k` bytes of Markdown.
The `|r` that a style owes counts, so a later close stays inside the bound. -/
def Emitted (html : Bool) (s s' : inline.Style) (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length + owed s' ≤ owed s + 12 * k ∧
    Text html (styleOpen s) w (styleOpen s')


/-- A colored span: it closes the color of the style, and opens it again after. -/
def Kept (html : Bool) (s : inline.Style) (c : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length ≤ 24 + 5 * c ∧
    Text html (styleOpen s) w (styleOpen s)

@[step]
theorem push_colored_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a b : Usize)
    (color : Std.Array U8 10#usize) (s : inline.Style) (escape : inline.Escape)
    (hcolor : bytes (Array.to_slice color).val ∈ colorCodes) (hb : b.val ≤ md.val.length)
    (hroom : out.val.length + 24 + 5 * (b.val - a.val) ≤ Usize.max) :
    inline.push_colored out md (a, b) color s escape ⦃ r =>
      Kept (isHtml escape) s (b.val - a.val) out r ⦄ := by
  unfold inline.push_colored
  step*
  subst s_post s1_post
  obtain ⟨w1, h1, l1, t1⟩ := out1_post1
  obtain ⟨w3, h3, l3, p3⟩ := out3_post1
  obtain ⟨w5, h5, l5, t5⟩ := r_post1
  refine ⟨w1 ++ bytes color.to_slice.val ++ w3 ++ resetCode ++ w5, ?_, ?_, ?_⟩
  · rw [h5, out4_post1, bytes_append, h3, out2_post1, bytes_append, h1, reset_bytes]
    simp only [List.append_assoc]
  · have hc : (bytes color.to_slice.val).length = 10 := by simp [bytes]
    have := owed_le s
    have hr : resetCode.length = 2 := by rfl
    have h5' : w5.length ≤ 10 := by split at l5 <;> omega
    simp only [List.length_append, hc, hr]
    omega
  · have := text_append (text_append (text_append (text_append (t1 _)
      (color_text (isHtml escape) _ hcolor)) (p3 true)) (reset_text _)) (t5 _)
    simpa only [List.append_assoc] using this

theorem opened_owed (s : inline.Style) : (if styleOpen s then 10 else 0) + owed s ≤ 12 := by
  unfold owed; split <;> omega

theorem emitted_len {html : Bool} {s s' : inline.Style} {k : Nat} {out r : alloc.vec.Vec U8}
    (h : Emitted html s s' k out r) : r.val.length ≤ out.val.length + 2 + 12 * k := by
  obtain ⟨w, hw, hl, _⟩ := h
  have := len_of_bytes hw
  have := owed_le s
  omega

/-- A piece that keeps the style reads fewer bytes than it may write. -/
theorem emitted_of_piece {html : Bool} {s : inline.Style} {k k' : Nat} {out r : alloc.vec.Vec U8}
    (h : Wrote html k out r) (hk : k ≤ 12 * k') : Emitted html s s k' out r := by
  obtain ⟨w, hw, hl, hp⟩ := h
  exact ⟨w, hw, by omega, hp _⟩

@[step]
theorem emphasis_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop i : Usize)
    (s : inline.Style) (escape : inline.Escape) (hi : i.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 24 ≤ Usize.max) :
    inline.emphasis out md (start, stop) i s escape ⦃ res =>
      i.val < res.1.1.val ∧ res.1.1.val ≤ stop.val ∧
      Emitted (isHtml escape) s res.1.2 (res.1.1.val - i.val) out res.2 ⦄ := by
  unfold inline.emphasis
  step*
  all_goals have hn : 1 ≤ n.val := n_post3 (i2_post2 (by omega) hi i1_post.symm)
  · refine ⟨by omega, by omega, ?_⟩
    obtain ⟨w1, h1, l1, t1⟩ := out1_post1
    obtain ⟨w2, h2, l2, t2⟩ := out2_post1
    refine ⟨w1 ++ w2, by rw [h2, h1, List.append_assoc], ?_, text_append (t1 _) (t2 _)⟩
    have := opened_owed next
    simp only [List.length_append]
    omega
  · omega
  · exact ⟨by omega, by omega, emitted_of_piece out1_post1 (by omega)⟩

theorem emitted_colored {html : Bool} {s : inline.Style} {k c : Nat} {out r : alloc.vec.Vec U8}
    (h : Kept html s c out r)
    (hk : 24 + 5 * c ≤ 12 * k) : Emitted html s s k out r := by
  obtain ⟨w, hw, hl, ht⟩ := h
  exact ⟨w, hw, by omega, ht⟩

@[step]
theorem code_span_spec (out : alloc.vec.Vec U8) (md : Slice U8) (stop i : Usize)
    (s : inline.Style) (escape : inline.Escape) (hi : i.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (htick : ∀ h : i.val < md.val.length, md.val[i.val] = 96#u8)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 24 ≤ Usize.max) :
    inline.code_span out md stop i s escape ⦃ res =>
      i.val < res.1.val ∧ res.1.val ≤ stop.val ∧
      Emitted (isHtml escape) s s (res.1.val - i.val) out res.2 ⦄ := by
  unfold inline.code_span
  step*
  all_goals have hn : 1 ≤ n.val := n_post2 (by omega) hi (htick (by omega))
  · omega
  · exact ⟨by omega, by omega, emitted_of_piece out1_post1 (by omega)⟩
  · rw [code_bytes]; simp [colorCodes]
  · obtain hc | ⟨hc1, hc2, hc3⟩ := close_post
    · exact absurd (by scalar_tac) ‹¬close = stop›
    · exact ⟨by omega, by omega, emitted_colored out1_post (by omega)⟩

@[step]
theorem link_spec (out : alloc.vec.Vec U8) (md : Slice U8) (stop i : Usize)
    (s : inline.Style) (escape : inline.Escape) (hi : i.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 24 ≤ Usize.max) :
    inline.link out md stop i s escape ⦃ res =>
      i.val < res.1.val ∧ res.1.val ≤ stop.val ∧
      Emitted (isHtml escape) s s (res.1.val - i.val) out res.2 ⦄ := by
  unfold inline.link
  step*
  by_cases ht : target < stop <;> simp only [ht, if_true, if_false] <;> step*
  · exact ⟨by omega, by omega, emitted_of_piece out1_post1 (by omega)⟩
  · rw [link_bytes]; simp [colorCodes]
  · have : target.val < stop.val := by scalar_tac
    have : paren.val ≠ stop.val := fun h => ‹¬paren = stop› (by scalar_tac)
    exact ⟨by omega, by omega, emitted_colored out1_post (by omega)⟩
  · exact ⟨by omega, by omega, emitted_of_piece out1_post1 (by omega)⟩
  · exact ⟨by omega, by omega, emitted_of_piece out1_post1 (by omega)⟩

@[step]
theorem escaped_spec (out : alloc.vec.Vec U8) (md : Slice U8) (stop i : Usize)
    (escape : inline.Escape) (hi : i.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 24 ≤ Usize.max) :
    inline.escaped out md stop i escape ⦃ res =>
      i.val < res.1.val ∧ res.1.val ≤ stop.val ∧
      ∀ s, Emitted (isHtml escape) s s (res.1.val - i.val) out res.2 ⦄ := by
  unfold inline.escaped
  step*
  all_goals exact ⟨by scalar_tac, by scalar_tac, fun _ => emitted_of_piece out1_post1 (by scalar_tac)⟩

@[step]
theorem step_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop i : Usize)
    (s : inline.Style) (escape : inline.Escape) (hi : i.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 24 ≤ Usize.max) :
    inline.step out md (start, stop) i s escape ⦃ res =>
      i.val < res.1.1.val ∧ res.1.1.val ≤ stop.val ∧
      Emitted (isHtml escape) s res.1.2 (res.1.1.val - i.val) out res.2 ⦄ := by
  unfold inline.step
  step*
  exact ⟨by scalar_tac, by scalar_tac, emitted_of_piece out1_post1 (by scalar_tac)⟩

theorem emitted_trans {html : Bool} {s s1 s2 : inline.Style} {k1 k2 : Nat}
    {out r1 r2 : alloc.vec.Vec U8} (h1 : Emitted html s s1 k1 out r1)
    (h2 : Emitted html s1 s2 k2 r1 r2) : Emitted html s s2 (k1 + k2) out r2 := by
  obtain ⟨w1, hw1, hl1, ht1⟩ := h1
  obtain ⟨w2, hw2, hl2, ht2⟩ := h2
  refine ⟨w1 ++ w2, by rw [hw2, hw1, List.append_assoc], ?_, text_append ht1 ht2⟩
  simp only [List.length_append]
  omega

@[simp] theorem styleOpen_plain : styleOpen inline.PLAIN = false := by
  simp [styleOpen, inline.PLAIN]

@[simp] theorem owed_plain : owed inline.PLAIN = 0 := by
  simp [owed]

/-- An inline text: at most 12 bytes for each byte of Markdown, and no color open at
either end. -/
def Inlined (html : Bool) (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ w.length ≤ 12 * k ∧ escapedText html w

def InlineInv (html : Bool) (out0 : alloc.vec.Vec U8) (start stop : Nat)
    (st : alloc.vec.Vec U8 × inline.Style × Usize) : Prop :=
  start ≤ st.2.2.val ∧ st.2.2.val ≤ stop ∧
  Emitted html inline.PLAIN st.2.1 (st.2.2.val - start) out0 st.1

@[step]
theorem push_inline_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop : Usize)
    (escape : inline.Escape) (hle : start.val ≤ stop.val) (hstop : stop.val ≤ md.val.length)
    (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - start.val) + 24 ≤ Usize.max) :
    inline.push_inline out md start stop escape ⦃ r =>
      Inlined (isHtml escape) (stop.val - start.val) out r ∧
      r.val.length ≤ out.val.length + 12 * (stop.val - start.val) ⦄ := by
  unfold inline.push_inline
  have hloop : inline.push_inline_loop out md start stop escape inline.PLAIN start ⦃ res =>
      Emitted (isHtml escape) inline.PLAIN res.2 (stop.val - start.val) out res.1 ⦄ := by
    unfold inline.push_inline_loop
    apply loop.spec_decr_nat (fun st => stop.val - st.2.2.val)
      (InlineInv (isHtml escape) out start.val stop.val) _ _ _ _
      ⟨le_refl _, hle, [], by simp, by simp, Text.nil _⟩
    rintro ⟨cur, style, i⟩ ⟨hs, hi, hem⟩
    dsimp only at hs hi hem
    have hcur : cur.val.length ≤ out.val.length + 12 * (i.val - start.val) := by
      obtain ⟨w, hw, hl, _⟩ := hem
      have := len_of_bytes hw
      simp only [owed_plain] at hl
      omega
    unfold inline.push_inline_loop.body
    step*
    refine ⟨⟨?_, ?_, ?_⟩, ?_⟩ <;> try dsimp only
    · omega
    · omega
    · have := emitted_trans hem next_post3
      rwa [show i.val - start.val + (next.val - i.val) = next.val - start.val by omega] at this
    · omega
  step with hloop as ⟨out1, style⟩
  have hem : Emitted (isHtml escape) inline.PLAIN style (stop.val - start.val) out out1 := ‹_›
  have hlen := emitted_len hem
  step*
  obtain ⟨w1, hw1, hl1, ht1⟩ := hem
  obtain ⟨w2, hw2, hl2, ht2⟩ := r_post1
  simp only [owed_plain, styleOpen_plain] at hl1 ht1
  have hw : bytes r.val = bytes out.val ++ (w1 ++ w2) := by rw [hw2, hw1, List.append_assoc]
  refine ⟨⟨w1 ++ w2, hw, by simp only [List.length_append]; omega, text_append ht1 (ht2 _)⟩, ?_⟩
  have := len_of_bytes hw
  simp only [List.length_append] at this
  omega

end Protocol.Inline
