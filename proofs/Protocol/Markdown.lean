import Protocol.Inline

/-! # Reply blocks (S22, S23, S24, S25)

The renderer writes one line of Markdown at a time. Each line appends a piece `w`, and
the spec of each writer says how `w` keeps the blocks well formed, and how long it is. -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.Inline

namespace Protocol.Markdown

@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_digits_val : markdown.MAX_DIGITS.val = 9 := by unfold markdown.MAX_DIGITS; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_level_val : markdown.MAX_LEVEL.val = 4 := by unfold markdown.MAX_LEVEL; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_heading_val : markdown.MAX_HEADING.val = 3 := by unfold markdown.MAX_HEADING; rfl
@[simp] theorem field_val : markdown.FIELD = 31#u8 := by unfold markdown.FIELD; rfl
@[simp] theorem heading_kind : markdown.HEADING = 104#u8 := by unfold markdown.HEADING; rfl
@[simp] theorem paragraph_kind : markdown.PARAGRAPH = 112#u8 := by unfold markdown.PARAGRAPH; rfl
@[simp] theorem item_kind : markdown.ITEM = 108#u8 := by unfold markdown.ITEM; rfl
@[simp] theorem quote_kind : markdown.QUOTE = 113#u8 := by unfold markdown.QUOTE; rfl
@[simp] theorem code_kind : markdown.CODE = 99#u8 := by unfold markdown.CODE; rfl
@[simp] theorem row_kind : markdown.ROW = 116#u8 := by unfold markdown.ROW; rfl
@[simp] theorem rule_kind : markdown.RULE = 114#u8 := by unfold markdown.RULE; rfl

/-! ## Scanning helpers -/

@[step]
theorem line_end_spec (md : Slice U8) (start : Usize) :
    markdown.line_end md start ⦃ r =>
      start.val ≤ r.val ∧ (start.val ≤ md.val.length → r.val ≤ md.val.length) ⦄ := by
  unfold markdown.line_end
  step*

@[step]
theorem skip_spaces_spec (md : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.skip_spaces md start stop ⦃ r => start.val ≤ r.val ∧ r.val ≤ stop.val ⦄ := by
  unfold markdown.skip_spaces markdown.skip_spaces_loop
  apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => start.val ≤ j.val ∧ j.val ≤ stop.val)
    _ _ _ _ ⟨le_refl _, hs⟩
  rintro j ⟨h1, h2⟩
  unfold markdown.skip_spaces_loop.body
  step*

@[step]
theorem trim_end_spec (md : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.trim_end md start stop ⦃ r => start.val ≤ r.val ∧ r.val ≤ stop.val ∧
      (∀ h : start.val < md.val.length, start.val < stop.val → 32 < (md.val[start.val]).val →
        start.val < r.val) ⦄ := by
  unfold markdown.trim_end markdown.trim_end_loop
  apply loop.spec_decr_nat (fun e => e.val)
    (fun e => start.val ≤ e.val ∧ e.val ≤ stop.val ∧
      ∀ k (hk : k < md.val.length), e.val ≤ k → k < stop.val → (md.val[k]).val ≤ 32)
    _ _ _ _ ⟨hs, le_refl _, fun k _ h1 h2 => absurd h2 (by omega)⟩
  rintro e ⟨h1, h2, h3⟩
  unfold markdown.trim_end_loop.body
  step*

@[step]
theorem run_len_spec (md : Slice U8) (i stop : Usize) (b : U8) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.run_len md i stop b ⦃ r => i.val + r.val ≤ stop.val ∧
      (∀ h : i.val < md.val.length, i.val < stop.val → md.val[i.val] = b → 1 ≤ r.val) ⦄ := by
  have hloop : markdown.run_len_loop md stop b i ⦃ j => i.val ≤ j.val ∧ j.val ≤ stop.val ∧
      (∀ h : i.val < md.val.length, i.val < stop.val → md.val[i.val] = b → i.val < j.val) ⦄ := by
    unfold markdown.run_len_loop
    apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => i.val ≤ j.val ∧ j.val ≤ stop.val)
      _ _ _ _ ⟨le_refl _, hi⟩
    rintro j ⟨hij, hj⟩
    unfold markdown.run_len_loop.body
    step*
  unfold markdown.run_len
  step with hloop as ⟨j, hj1, hj2, hj3⟩
  step*
  exact ⟨by omega, fun h h1 h2 => by have := hj3 h h1 h2; omega⟩

@[step]
theorem is_digit_spec (b : U8) : markdown.is_digit b ⦃ r => (r = true ↔ 48 ≤ b.val ∧ b.val ≤ 57) ⦄ := by
  unfold markdown.is_digit
  step*

@[step]
theorem min_usize_spec (a b : Usize) : markdown.min_usize a b ⦃ r => r.val = min a.val b.val ⦄ := by
  unfold markdown.min_usize
  step*

@[step]
theorem is_fence_spec (md : Slice U8) (i stop : Usize) (hstop : stop.val ≤ md.val.length) :
    markdown.is_fence md i stop ⦃ _ => True ⦄ := by
  unfold markdown.is_fence
  step*

@[step]
theorem closes_fence_spec (md : Slice U8) (i stop : Usize) (fence : U8)
    (hstop : stop.val ≤ md.val.length) :
    markdown.closes_fence md i stop fence ⦃ _ => True ⦄ := by
  unfold markdown.closes_fence
  step*

@[step]
theorem heading_level_spec (md : Slice U8) (i stop : Usize) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.heading_level md i stop ⦃ r => r.val ≤ 6 ∧ i.val + r.val ≤ stop.val ⦄ := by
  unfold markdown.heading_level
  step*

@[step]
theorem is_rule_loop0_spec (md : Slice U8) (stop i : Usize) (b : U8) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_rule_loop0 md stop b 0#i32 i ⦃ _ => True ⦄ := by
  unfold markdown.is_rule_loop0
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => 0 ≤ st.1.val ∧ st.1.val ≤ (st.2.val : Int) - i.val ∧ i.val ≤ st.2.val ∧ st.2.val ≤ stop.val)
    _ _ _ _ (by simp; omega)
  rintro ⟨count, j⟩ ⟨h1, h2, h3, h4⟩
  dsimp only at h1 h2 h3 h4
  unfold markdown.is_rule_loop0.body
  step*
  all_goals scalar_tac

@[step]
theorem is_rule_loop1_spec (md : Slice U8) (stop i : Usize) (b : U8) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_rule_loop1 md stop b 0#i32 i ⦃ _ => True ⦄ := by
  unfold markdown.is_rule_loop1
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => 0 ≤ st.1.val ∧ st.1.val ≤ (st.2.val : Int) - i.val ∧ i.val ≤ st.2.val ∧ st.2.val ≤ stop.val)
    _ _ _ _ (by simp; omega)
  rintro ⟨count, j⟩ ⟨h1, h2, h3, h4⟩
  dsimp only at h1 h2 h3 h4
  unfold markdown.is_rule_loop1.body
  step*
  all_goals scalar_tac

@[step]
theorem is_rule_loop2_spec (md : Slice U8) (stop i : Usize) (b : U8) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_rule_loop2 md stop b 0#i32 i ⦃ _ => True ⦄ := by
  unfold markdown.is_rule_loop2
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => 0 ≤ st.1.val ∧ st.1.val ≤ (st.2.val : Int) - i.val ∧ i.val ≤ st.2.val ∧ st.2.val ≤ stop.val)
    _ _ _ _ (by simp; omega)
  rintro ⟨count, j⟩ ⟨h1, h2, h3, h4⟩
  dsimp only at h1 h2 h3 h4
  unfold markdown.is_rule_loop2.body
  step*
  all_goals scalar_tac

@[step]
theorem is_rule_spec (md : Slice U8) (i stop : Usize) (hi : i.val < md.val.length) (his : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_rule md i stop ⦃ _ => True ⦄ := by
  unfold markdown.is_rule
  step*

def Digits (md : Slice U8) (a b : Nat) : Prop :=
  ∀ k (hk : k < md.val.length), a ≤ k → k < b → 48 ≤ (md.val[k]).val ∧ (md.val[k]).val ≤ 57

@[step]
theorem run_len_digits_spec (md : Slice U8) (a stop : Usize) (hi : a.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.run_len_digits md a stop ⦃ r => a.val + r.val ≤ stop.val ∧
      Digits md a.val (a.val + r.val) ⦄ := by
  have hloop : markdown.run_len_digits_loop md stop a ⦃ j => a.val ≤ j.val ∧ j.val ≤ stop.val ∧
      Digits md a.val j.val ⦄ := by
    unfold markdown.run_len_digits_loop
    apply loop.spec_decr_nat (fun j => stop.val - j.val)
      (fun j => a.val ≤ j.val ∧ j.val ≤ stop.val ∧ Digits md a.val j.val)
      _ _ _ _ ⟨le_refl _, hi, fun k _ h1 h2 => absurd h2 (by omega)⟩
    rintro j ⟨hij, hj, hd⟩
    unfold markdown.run_len_digits_loop.body
    step*
    refine ⟨by scalar_tac, by scalar_tac, fun k hk h1 h2 => ?_, by scalar_tac⟩
    by_cases hkj : k < j.val
    · exact hd k hk h1 hkj
    · have : k = j.val := by scalar_tac
      subst this
      rw [← i_post]
      exact b_post.mp ‹_›
  unfold markdown.run_len_digits
  step with hloop as ⟨j, hj1, hj2, hj3⟩
  step*

@[step]
theorem number_end_spec (md : Slice U8) (i stop : Usize) (hi : i.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.number_end md i stop ⦃ r => r.val = i.val ∨
      (i.val + 2 ≤ r.val ∧ r.val ≤ stop.val ∧ r.val - 1 - i.val ≤ 9 ∧ Digits md i.val (r.val - 1)) ⦄ := by
  unfold markdown.number_end
  step*
  all_goals
    right
    refine ⟨by scalar_tac, by scalar_tac, by scalar_tac, ?_⟩
    rw [show r.val - 1 = i.val + digits.val by scalar_tac]
    exact digits_post2

def IsDigitAt (md : Slice U8) (a : Nat) : Prop :=
  ∀ h : a < md.val.length, 48 ≤ (md.val[a]).val ∧ (md.val[a]).val ≤ 57

@[step]
theorem item_marker_end_spec (md : Slice U8) (a stop : Usize) (ha : a.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.item_marker_end md a stop ⦃ r => r.val = a.val ∨
      (a.val < r.val ∧ r.val ≤ stop.val ∧
        (IsDigitAt md a.val → r.val - 1 - a.val ≤ 9 ∧ Digits md a.val (r.val - 1))) ⦄ := by
  unfold markdown.item_marker_end
  step*
  simp only [ite_bind]
  step*
  all_goals first
    | (left; scalar_tac)
    | (right; refine ⟨by scalar_tac, by scalar_tac, fun _ => ⟨by scalar_tac,
        fun k _ h1 h2 => absurd h2 (by scalar_tac)⟩⟩)

@[step]
theorem push_digits_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a b : Usize)
    (hab : a.val ≤ b.val) (hb : b.val ≤ md.val.length)
    (hroom : out.val.length + (b.val - a.val) ≤ Usize.max) :
    markdown.push_digits out md a b ⦃ r =>
      r.val = out.val ++ (md.val.drop a.val).take (b.val - a.val) ∧
      r.val.length = out.val.length + (b.val - a.val) ⦄ := by
  unfold markdown.push_digits markdown.push_digits_loop
  apply loop.spec_decr_nat (fun st => b.val - st.2.val)
    (fun st => a.val ≤ st.2.val ∧ st.2.val ≤ b.val ∧
      st.1.val = out.val ++ (md.val.drop a.val).take (st.2.val - a.val)) _ _ _ _
    ⟨le_refl _, hab, by simp⟩
  rintro ⟨cur, j⟩ ⟨h1, h2, h3⟩
  dsimp only at h1 h2 h3
  unfold markdown.push_digits_loop.body
  step*
  · rw [h3]; simp; scalar_tac
  · refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
    have hlt : j.val < md.val.length := by scalar_tac
    rw [out1_post, h3, j1_post, show j.val + 1 - a.val = (j.val - a.val) + 1 by omega,
      List.take_add_one, List.getElem?_eq_getElem (by simp; omega)]
    simp [i_post, Nat.add_sub_cancel' h1]
  · have : j.val = b.val := by scalar_tac
    rw [this] at h3
    refine ⟨h3, ?_⟩
    rw [h3]; simp; omega

@[step]
theorem skip_quote_marks_spec (md : Slice U8) (a stop : Usize) (ha : a.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.skip_quote_marks md a stop ⦃ r => a.val ≤ r.val ∧ r.val ≤ stop.val ∧
      (∀ h : a.val < md.val.length, a.val < stop.val → md.val[a.val] = 62#u8 → a.val < r.val) ⦄ := by
  unfold markdown.skip_quote_marks markdown.skip_quote_marks_loop
  apply loop.spec_decr_nat (fun j => stop.val - j.val)
    (fun j => a.val ≤ j.val ∧ j.val ≤ stop.val) _ _ _ _ ⟨le_refl _, ha⟩
  rintro j ⟨h1, h2⟩
  unfold markdown.skip_quote_marks_loop.body
  step*

@[step]
theorem is_delimiter_row_loop_spec (md : Slice U8) (stop a : Usize) (ha : a.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_delimiter_row_loop md stop 0#i32 a ⦃ _ => True ⦄ := by
  unfold markdown.is_delimiter_row_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => 0 ≤ st.1.val ∧ st.1.val ≤ (st.2.val : Int) - a.val ∧ a.val ≤ st.2.val ∧
      st.2.val ≤ stop.val)
    _ _ _ _ (by simp; omega)
  rintro ⟨count, j⟩ ⟨h1, h2, h3, h4⟩
  dsimp only at h1 h2 h3 h4
  unfold markdown.is_delimiter_row_loop.body
  step_ite
  all_goals scalar_tac

@[step]
theorem is_delimiter_row_spec (md : Slice U8) (a stop : Usize) (ha : a.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.is_delimiter_row md a stop ⦃ _ => True ⦄ := by
  unfold markdown.is_delimiter_row
  step*

@[step]
theorem next_is_delimiter_spec (md : Slice U8) (next : Usize) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.next_is_delimiter md next ⦃ _ => True ⦄ := by
  unfold markdown.next_is_delimiter
  step*

@[step]
theorem cell_end_spec (md : Slice U8) (a stop : Usize) (ha : a.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) :
    markdown.cell_end md a stop ⦃ r => a.val ≤ r.val ∧ r.val ≤ stop.val ⦄ := by
  unfold markdown.cell_end markdown.cell_end_loop
  apply loop.spec_decr_nat (fun j => stop.val - j.val) (fun j => a.val ≤ j.val ∧ j.val ≤ stop.val)
    _ _ _ _ ⟨le_refl _, ha⟩
  rintro j ⟨h1, h2⟩
  unfold markdown.cell_end_loop.body
  step_ite

/-! ## Blocks -/

/-- `r` is `out` with a piece `w` after it, and `w` has the property `P`. -/
def Appended (out r : alloc.vec.Vec U8) (P : List Spec.Byte → Prop) : Prop :=
  ∃ w, bytes r.val = bytes out.val ++ w ∧ P w

def Blocks (s : List Spec.Byte) : Prop :=
  ∃ bs : List (List Spec.Byte), (∀ b ∈ bs, Block escapedText b) ∧ s = bs.flatten

/-- A block that ends in a SimpleHTML text, so a later line can add to that text. -/
def OpenForm (w : List Spec.Byte) : Prop :=
  ∃ pre t, w = pre ++ t ∧ (∀ t', escapedText true t' → Block escapedText (pre ++ t')) ∧
    escapedText true t

/-- A space and more text for the open block. -/
def Cont (w : List Spec.Byte) : Prop := ∃ u, w = ch ' ' :: u ∧ escapedText true u

/-- The blocks so far. After a paragraph, a list item, or a quote, the last block is open. -/
def Inv (o : markdown.Open) (s : List Spec.Byte) : Prop :=
  Blocks s ∧ (o ≠ .Nothing → ∃ s0 w, s = s0 ++ w ∧ Blocks s0 ∧ OpenForm w)

/-- What one line adds keeps the blocks well formed. -/
def LineOut (o o' : markdown.Open) (w : List Spec.Byte) : Prop := ∀ s, Inv o s → Inv o' (s ++ w)

theorem blocks_nil : Blocks [] := ⟨[], by simp, rfl⟩

theorem blocks_snoc {s w : List Spec.Byte} (hs : Blocks s) (hw : Block escapedText w) :
    Blocks (s ++ w) := by
  obtain ⟨bs, hbs, rfl⟩ := hs
  refine ⟨bs ++ [w], fun b hb => ?_, by simp⟩
  rcases List.mem_append.mp hb with h | h
  · exact hbs b h
  · simp at h; subst h; exact hw

theorem openForm_block {w : List Spec.Byte} (h : OpenForm w) : Block escapedText w := by
  obtain ⟨pre, t, rfl, hpre, ht⟩ := h
  exact hpre t ht

theorem inv_start : Inv .Nothing [] := ⟨blocks_nil, fun h => absurd rfl h⟩

theorem inv_blocks {o : markdown.Open} {s : List Spec.Byte} (h : Inv o s) : Blocks s := h.1

theorem lineOut_nil (o : markdown.Open) : LineOut o .Nothing [] :=
  fun _ hs => ⟨by simpa using hs.1, fun h => absurd rfl h⟩

theorem lineOut_block (o : markdown.Open) {w : List Spec.Byte} (hw : Block escapedText w) :
    LineOut o .Nothing w :=
  fun _ hs => ⟨blocks_snoc hs.1 hw, fun h => absurd rfl h⟩

theorem lineOut_open (o o' : markdown.Open) {w : List Spec.Byte} (hw : OpenForm w) :
    LineOut o o' w :=
  fun s hs => ⟨blocks_snoc hs.1 (openForm_block hw), fun _ => ⟨s, w, rfl, hs.1, hw⟩⟩

theorem escaped_space_append {t u : List Spec.Byte} (ht : escapedText true t)
    (hu : escapedText true u) : escapedText true (t ++ ch ' ' :: u) := by
  have hs : plainByte true (ch ' ') := by simp [plainByte, fieldByte, pipe, ch]
  exact text_append ht (Text.plain hs hu)

theorem lineOut_cont {o : markdown.Open} (ho : o ≠ .Nothing) {w : List Spec.Byte} (hw : Cont w) :
    LineOut o o w := by
  intro s hs
  obtain ⟨s0, v, rfl, hs0, pre, t, rfl, hpre, ht⟩ := hs.2 ho
  obtain ⟨u, rfl, hu⟩ := hw
  have hv : OpenForm (pre ++ (t ++ ch ' ' :: u)) :=
    ⟨pre, t ++ ch ' ' :: u, rfl, hpre, escaped_space_append ht hu⟩
  refine ⟨?_, fun _ => ⟨s0, pre ++ (t ++ ch ' ' :: u), by simp, hs0, hv⟩⟩
  have := blocks_snoc hs0 (openForm_block hv)
  simpa using this

/-! ## Writers -/

@[step]
theorem start_block_spec (out : alloc.vec.Vec U8) (kind : U8)
    (hroom : out.val.length + 2 ≤ Usize.max) :
    markdown.start_block out kind ⦃ r =>
      bytes r.val = bytes out.val ++ [nl, kind.bv] ∧ r.val.length = out.val.length + 2 ⦄ := by
  unfold markdown.start_block
  step*
  · simp [out1_post]; omega
  · refine ⟨?_, by simp [r_post, out1_post]⟩
    simp only [r_post, out1_post, bytes, List.map_append, List.map_cons, List.map_nil,
      List.append_assoc, List.cons_append, List.nil_append]
    rfl

@[step]
theorem push_level_spec (out : alloc.vec.Vec U8) (level : Usize) (hl : level.val ≤ 9)
    (hroom : out.val.length + 2 ≤ Usize.max) :
    markdown.push_level out level ⦃ r =>
      bytes r.val = bytes out.val ++ [us, BitVec.ofNat 8 (48 + level.val)] ∧
      r.val.length = out.val.length + 2 ⦄ := by
  unfold markdown.push_level
  step*
  · simp [out1_post]; omega
  · have hi : i.val = level.val := by
      rw [i_post]; simp only [UScalar.cast_val_eq]; scalar_tac
    refine ⟨?_, by simp [r_post, out1_post]⟩
    have hb : i1.bv = BitVec.ofNat 8 (48 + level.val) := by rw [U8_bv_eq_ofNat, i1_post, hi]
    simp only [r_post, out1_post, bytes, List.map_append, List.map_cons, List.map_nil,
      List.append_assoc, List.cons_append, List.nil_append, hb, field_val]
    rfl

/-- `US`, then an inline text of at most 12 bytes for each byte of Markdown. -/
def Field (html : Bool) (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  Appended out r (fun w => ∃ t, w = us :: t ∧ t.length ≤ 12 * k ∧ escapedText html t)

@[step]
theorem push_text_field_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a b : Usize)
    (escape : inline.Escape) (hab : a.val ≤ b.val) (hb : b.val ≤ md.val.length)
    (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (b.val - a.val) + 25 ≤ Usize.max) :
    markdown.push_text_field out md a b escape ⦃ r =>
      Field (isHtml escape) (b.val - a.val) out r ∧
      r.val.length ≤ out.val.length + 1 + 12 * (b.val - a.val) ⦄ := by
  unfold markdown.push_text_field
  step*
  · simp [out1_post]; omega
  · obtain ⟨t, ht, hl, he⟩ := r_post1
    have hlen : out1.val.length = out.val.length + 1 := by simp [out1_post]
    refine ⟨⟨us :: t, ?_, t, rfl, by omega, he⟩, by omega⟩
    rw [ht, out1_post]
    simp [bytes, us]

theorem code_line_loop_spec (out0 out : alloc.vec.Vec U8) (md : Slice U8) (a b j : Usize)
    (hab : a.val ≤ j.val) (hj : j.val ≤ b.val) (hb : b.val ≤ md.val.length)
    (hroom : out0.val.length + 5 * (b.val - a.val) ≤ Usize.max)
    (hout : Wrote false (5 * (j.val - a.val)) out0 out) :
    markdown.code_line_loop out md b j ⦃ r => Wrote false (5 * (b.val - a.val)) out0 r ⦄ := by
  unfold markdown.code_line_loop
  apply loop.spec_decr_nat (fun st => b.val - st.2.val)
    (fun st => a.val ≤ st.2.val ∧ st.2.val ≤ b.val ∧ Wrote false (5 * (st.2.val - a.val)) out0 st.1)
    _ _ _ _ ⟨hab, hj, hout⟩
  rintro ⟨cur, i⟩ ⟨hs, hi, w, hw, hl, hp⟩
  dsimp only at hs hi hw hl
  have hcur := len_of_bytes hw
  unfold markdown.code_line_loop.body
  step*
  · obtain ⟨v, hv, hvl, hvp⟩ := out1_post1
    refine ⟨by scalar_tac, by scalar_tac, ⟨w ++ v, by rw [hv, hw, List.append_assoc], ?_,
      hp.append hvp⟩, by scalar_tac⟩
    simp only [List.length_append]
    have : i2.val = i.val + 1 := by scalar_tac
    rw [this]
    omega
  · have : i.val = b.val := by scalar_tac
    rw [this] at hl
    exact ⟨w, hw, hl, hp⟩

/-- A block and its length. -/
def BlockOut (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  Appended out r (fun w => Block escapedText w ∧ w.length ≤ k)

@[step]
theorem code_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a b : Usize)
    (hab : a.val ≤ b.val) (hb : b.val ≤ md.val.length)
    (hroom : out.val.length + 3 + 5 * (b.val - a.val) ≤ Usize.max) :
    markdown.code_line out md a b ⦃ r =>
      BlockOut (3 + 5 * (b.val - a.val)) out r ∧
      r.val.length ≤ out.val.length + 3 + 5 * (b.val - a.val) ⦄ := by
  unfold markdown.code_line
  step*
  have h2 : out2.val.length = out.val.length + 3 := by simp [out2_post, out1_post2]
  apply WP.spec_mono (code_line_loop_spec out2 out2 md a b a (le_refl _) hab hb (by omega)
    ⟨[], by simp, by simp, Piece.nil _⟩)
  intro r ⟨t, ht, hl, hp⟩
  have hw : bytes r.val = bytes out.val ++ ([nl, ch 'c', us] ++ t) := by
    rw [ht, out2_post, bytes_append, out1_post1]
    simp only [code_kind, field_val, bytes, List.map_cons, List.map_nil, List.append_assoc,
      List.cons_append, List.nil_append]
    rfl
  have := len_of_bytes hw
  refine ⟨⟨_, hw, Block.code (hp false), by simp; omega⟩, by simp at this; omega⟩

/-- A block, or nothing at all. -/
def BlockOrNothing (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  Appended out r (fun w => (w = [] ∨ Block escapedText w) ∧ w.length ≤ k)

theorem blockOrNothing_block {k : Nat} {out r : alloc.vec.Vec U8} (h : BlockOut k out r) :
    BlockOrNothing k out r := by
  obtain ⟨w, hw, hb, hl⟩ := h
  exact ⟨w, hw, Or.inr hb, hl⟩

theorem lineOut_blockOrNothing (o : markdown.Open) {w : List Spec.Byte}
    (h : w = [] ∨ Block escapedText w) : LineOut o .Nothing w := by
  rcases h with rfl | h
  · exact lineOut_nil o
  · exact lineOut_block o h

theorem blockOut_len {k : Nat} {out r : alloc.vec.Vec U8} (h : BlockOut k out r) :
    r.val.length ≤ out.val.length + k := by
  obtain ⟨w, hw, _, hl⟩ := h
  have := len_of_bytes hw
  omega

theorem bytes_push (l : List U8) (b : U8) : bytes (l ++ [b]) = bytes l ++ [b.bv] := by
  simp [bytes]

@[step]
theorem heading_spec (out : alloc.vec.Vec U8) (md : Slice U8) (i stop n : Usize)
    (hn : 1 ≤ n.val) (hn6 : n.val ≤ 6) (hin : i.val + n.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - i.val) + 64 ≤ Usize.max) :
    markdown.heading out md i stop n ⦃ r =>
      BlockOut (5 + 12 * (stop.val - i.val - n.val)) out r ∧
      r.val.length ≤ out.val.length + 5 + 12 * (stop.val - i.val - n.val) ⦄ := by
  unfold markdown.heading
  step*
  · omega
  · obtain ⟨w, hw, t, rfl, htl, ht⟩ := r_post1
    have hi1 : 1 ≤ i1.val ∧ i1.val ≤ 3 := by simp [max_heading_val] at i1_post; omega
    have hlv : BitVec.ofNat 8 (48 + i1.val) = ch '1' ∨ BitVec.ofNat 8 (48 + i1.val) = ch '2' ∨
        BitVec.ofNat 8 (48 + i1.val) = ch '3' := by
      obtain ⟨h1, h3⟩ := hi1
      interval_cases h : i1.val <;> simp [ch]
    have hbytes : bytes r.val = bytes out.val ++
        ([nl, ch 'h', us, BitVec.ofNat 8 (48 + i1.val), us] ++ t) := by
      rw [hw, out2_post1, out1_post1]
      simp only [heading_kind, List.append_assoc, List.cons_append, List.nil_append]
      rfl
    have := len_of_bytes hbytes
    simp only [List.length_append, List.length_cons, List.length_nil] at this
    refine ⟨⟨_, hbytes, Block.heading hlv ht, ?_⟩, ?_⟩
    · simp only [List.length_append, List.length_cons, List.length_nil]; omega
    · omega

@[step]
theorem rule_spec (out : alloc.vec.Vec U8) (hroom : out.val.length + 2 ≤ Usize.max) :
    markdown.rule out ⦃ r => BlockOut 2 out r ∧ r.val.length ≤ out.val.length + 2 ⦄ := by
  unfold markdown.rule
  step*
  refine ⟨⟨[nl, ch 'r'], by rw [r_post1, rule_kind]; rfl, Block.rule, by simp⟩, by omega⟩

/-- What one line of Markdown adds: it keeps the blocks well formed, and it is at most 16
bytes for each byte of the line, or 16 bytes for an empty line. -/
def LinePost (o : markdown.Open) (a b : Nat) (out : alloc.vec.Vec U8)
    (res : markdown.State × alloc.vec.Vec U8) : Prop :=
  Appended out res.2 (fun w => LineOut o res.1.open w ∧
    (w.length ≤ 16 * (b - a) ∨ (a = b ∧ w.length ≤ 16))) ∧
  res.2.val.length ≤ out.val.length + 16 * (b - a) + 16

theorem linePost_block (o o' : markdown.Open) (mode : markdown.Mode) (a b k : Nat)
    (out r : alloc.vec.Vec U8) (ho' : o' = .Nothing) (h : BlockOut k out r)
    (hk : k ≤ 16 * (b - a) ∨ (a = b ∧ k ≤ 16)) :
    LinePost o a b out ({ mode := mode, «open» := o' }, r) := by
  subst ho'
  obtain ⟨w, hw, hb, hl⟩ := h
  have := len_of_bytes hw
  refine ⟨⟨w, hw, lineOut_block o hb, by omega⟩, by dsimp only; omega⟩

theorem linePost_same (o o' : markdown.Open) (mode : markdown.Mode) (a b : Nat)
    (out : alloc.vec.Vec U8) (ho' : o' = .Nothing) :
    LinePost o a b out ({ mode := mode, «open» := o' }, out) := by
  subst ho'
  exact ⟨⟨[], by simp, lineOut_nil o, by simp⟩, by dsimp only; omega⟩

@[step]
theorem fence_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a b : Usize) (fence : U8)
    (hab : a.val ≤ b.val) (hb : b.val ≤ md.val.length)
    (hroom : out.val.length + 16 * (b.val - a.val) + 64 ≤ Usize.max) :
    markdown.fence_line out md a b fence ⦃ res => ∀ o, LinePost o a.val b.val out res ⦄ := by
  unfold markdown.fence_line
  step*
  · exact fun o => linePost_same o _ _ _ _ _ rfl
  · exact fun o => linePost_block o _ _ _ _ _ _ _ rfl out1_post1 (by omega)

theorem digitByte_iff (x : U8) : digitByte x.bv ↔ 48 ≤ x.val ∧ x.val ≤ 57 := by
  simp only [digitByte, ch, BitVec.le_def]
  simp

theorem digits_mem (md : Slice U8) (a e : Nat) (h : Digits md a e) :
    ∀ x ∈ bytes ((md.val.drop a).take (e - a)), digitByte x := by
  intro x hx
  simp only [bytes, List.mem_map] at hx
  obtain ⟨y, hy, rfl⟩ := hx
  obtain ⟨k, hk, rfl⟩ := List.getElem_of_mem hy
  simp only [List.length_take, List.length_drop] at hk
  rw [List.getElem_take, List.getElem_drop, digitByte_iff]
  exact h (a + k) (by omega) (by omega) (by omega)

/-- A block whose last text a later line can grow, and its length. -/
def OpenOut (k : Nat) (out r : alloc.vec.Vec U8) : Prop :=
  Appended out r (fun w => OpenForm w ∧ w.length ≤ k)

@[step]
theorem item_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop indent m : Usize)
    (ham : a.val < m.val) (hm : m.val ≤ stop.val) (hstop : stop.val ≤ md.val.length)
    (hmd : md.val.length ≤ 2 ^ 20)
    (hdig : IsDigitAt md a.val → m.val - 1 - a.val ≤ 9 ∧ Digits md a.val (m.val - 1))
    (hroom : out.val.length + 16 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.item out md (a, stop) indent m ⦃ r =>
      OpenOut (6 + (m.val - a.val) + 12 * (stop.val - m.val)) out r ∧
      r.val.length ≤ out.val.length + 6 + (m.val - a.val) + 12 * (stop.val - m.val) ⦄ := by
  unfold markdown.item
  step_ite
  all_goals
    have h3 : out3.val.length = out.val.length + 5 := by
      rw [out3_post]; simp; omega
    have hlvl : ch '0' ≤ BitVec.ofNat 8 (48 + i2.val) ∧
        BitVec.ofNat 8 (48 + i2.val) ≤ ch '4' := by
      have : i2.val ≤ 4 := by simp [max_level_val] at i2_post; omega
      simp only [ch, BitVec.le_def, BitVec.toNat_ofNat]
      constructor <;> simp <;> omega
  · omega
  · simp [out4_post2]; omega
  · have hdig' : IsDigitAt md a.val := fun h => by rw [← i3_post]; exact b_post.mp ‹_›
    obtain ⟨hd9, hd⟩ := hdig hdig'
    obtain ⟨w, hw, t, rfl, htl, ht⟩ := r_post1
    have hbytes : bytes r.val = bytes out.val ++
        ([nl, ch 'l', us, BitVec.ofNat 8 (48 + i2.val), us] ++
          bytes ((md.val.drop a.val).take (x.val - a.val)) ++ [us] ++ t) := by
      rw [hw, out4_post1, bytes_append, out3_post, bytes_append, out2_post1, out1_post1]
      simp only [item_kind, field_val, bytes, List.map_cons, List.map_nil, List.append_assoc,
        List.cons_append, List.nil_append]
      rfl
    have hlen := len_of_bytes hbytes
    simp only [List.length_append, List.length_cons, List.length_nil, bytes, List.length_map,
      List.length_take, List.length_drop] at hlen
    refine ⟨⟨_, hbytes, ⟨_, t, rfl, fun t' ht' => Block.item hlvl ?_ ?_ ht', ht⟩, ?_⟩, ?_⟩
    · simp [bytes]; omega
    · exact digits_mem md a.val x.val (by rw [x_post1]; exact hd)
    · simp [bytes]; omega
    · omega
  · omega
  · obtain ⟨w, hw, t, rfl, htl, ht⟩ := r_post1
    have hbytes : bytes r.val = bytes out.val ++
        ([nl, ch 'l', us, BitVec.ofNat 8 (48 + i2.val), us] ++ [] ++ [us] ++ t) := by
      rw [hw, out3_post, bytes_append, out2_post1, out1_post1]
      simp only [item_kind, field_val, bytes, List.map_cons, List.map_nil, List.append_assoc,
        List.cons_append, List.nil_append, List.append_nil]
      rfl
    have hlen := len_of_bytes hbytes
    simp only [List.length_append, List.length_cons, List.length_nil] at hlen
    refine ⟨⟨_, hbytes, ⟨_, t, rfl, fun t' ht' => Block.item hlvl (by simp) (by simp) ht', ht⟩,
      ?_⟩, ?_⟩
    · simp; omega
    · omega

@[step]
theorem open_eq_spec (a b : markdown.Open) :
    markdown.Open.Insts.CoreCmpPartialEqOpen.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [markdown.Open.Insts.CoreCmpPartialEqOpen.eq, markdown.Open.read_discriminant]

@[step]
theorem continue_text_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop : Usize)
    (ha : a.val ≤ stop.val) (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 12 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.continue_text out md a stop ⦃ r =>
      Appended out r (fun w => Cont w ∧ w.length ≤ 1 + 12 * (stop.val - a.val)) ∧
      r.val.length ≤ out.val.length + 1 + 12 * (stop.val - a.val) ⦄ := by
  unfold markdown.continue_text
  step*
  all_goals have h1 : out1.val.length = out.val.length + 1 := by simp [out1_post]
  · omega
  · obtain ⟨t, ht, hl, he⟩ := r_post1
    have hw : bytes r.val = bytes out.val ++ (ch ' ' :: t) := by
      rw [ht, out1_post, bytes_append]
      simp only [bytes, List.map_cons, List.map_nil, List.append_assoc, List.cons_append,
        List.nil_append]
      rfl
    exact ⟨⟨_, hw, ⟨t, rfl, he⟩, by simp; omega⟩, by omega⟩

theorem field_open {out r : alloc.vec.Vec U8} {k : Nat} (kind : Spec.Byte) (b : alloc.vec.Vec U8)
    (hb : bytes b.val = bytes out.val ++ [nl, kind]) (hf : Field true k b r)
    (hkind : ∀ t, escapedText true t → Block escapedText ([nl, kind, us] ++ t)) :
    OpenOut (3 + 12 * k) out r := by
  obtain ⟨w, hw, t, rfl, hl, ht⟩ := hf
  refine ⟨[nl, kind, us] ++ t, ?_, ⟨[nl, kind, us], t, rfl, hkind, ht⟩, by simp; omega⟩
  rw [hw, hb]
  simp

theorem openOut_len {k : Nat} {out r : alloc.vec.Vec U8} (h : OpenOut k out r) :
    r.val.length ≤ out.val.length + k := by
  obtain ⟨w, hw, _, hl⟩ := h
  have := len_of_bytes hw
  omega

@[step]
theorem quote_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop : Usize) (o : markdown.Open)
    (ha : a.val < stop.val) (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hq : ∀ h : a.val < md.val.length, md.val[a.val] = 62#u8)
    (hroom : out.val.length + 16 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.quote out md a stop o ⦃ r =>
      Appended out r (fun w => LineOut o .Quote w ∧ w.length ≤ 3 + 12 * (stop.val - a.val - 1)) ∧
      r.val.length ≤ out.val.length + 3 + 12 * (stop.val - a.val - 1) ⦄ := by
  unfold markdown.quote
  step*
  all_goals have hat : a.val < text.val := text_post3 (by omega) ha (hq (by omega))
  · have ho : o = .Quote := b_post.mp ‹_›
    subst ho
    obtain ⟨w, hw, hc, hl⟩ := r_post1
    exact ⟨⟨w, hw, lineOut_cont (by simp) hc, by omega⟩, by omega⟩
  · have := field_open (out := out) (ch 'q') out1 (by rw [out1_post1, quote_kind]; rfl)
      (by simpa [isHtml] using r_post1) (fun t ht => Block.quote ht)
    obtain ⟨w, hw, hf, hl⟩ := this
    exact ⟨⟨w, hw, lineOut_open o .Quote hf, by omega⟩, by have := len_of_bytes hw; omega⟩

@[step]
theorem paragraph_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop : Usize) (o : markdown.Open)
    (ha : a.val ≤ stop.val) (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 16 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.paragraph out md a stop o ⦃ res =>
      Appended out res.2 (fun w => LineOut o res.1 w ∧ w.length ≤ 3 + 12 * (stop.val - a.val)) ∧
      res.2.val.length ≤ out.val.length + 3 + 12 * (stop.val - a.val) ⦄ := by
  unfold markdown.paragraph
  step*
  · have ho : o = .Paragraph := b_post.mp ‹_›
    subst ho
    obtain ⟨w, hw, hc, hl⟩ := out1_post1
    exact ⟨⟨w, hw, lineOut_cont (by simp) hc, by omega⟩, by omega⟩
  · have ho : o = .Item := b1_post.mp ‹_›
    subst ho
    obtain ⟨w, hw, hc, hl⟩ := out1_post1
    exact ⟨⟨w, hw, lineOut_cont (by simp) hc, by omega⟩, by omega⟩
  · have := field_open (out := out) (ch 'p') out1 (by rw [out1_post1, paragraph_kind]; rfl)
      (by simpa [isHtml] using out2_post1) (fun t ht => Block.paragraph ht)
    obtain ⟨w, hw, hf, hl⟩ := this
    exact ⟨⟨w, hw, lineOut_open o .Paragraph hf, by omega⟩, by have := len_of_bytes hw; omega⟩

theorem cells_snoc {c t : List Spec.Byte} (hc : Cells escapedText c) (ht : escapedText false t) :
    Cells escapedText (c ++ us :: t) := by
  induction hc with
  | nil => simpa using Cells.cons ht Cells.nil
  | cons h _ ih => simpa using Cells.cons h ih

/-- The cells of a row so far: at most 13 bytes for each byte of Markdown after the first `|`. -/
def RowInv (out0 : alloc.vec.Vec U8) (a : Nat) (cur : alloc.vec.Vec U8) (s : Nat) : Prop :=
  Appended out0 cur (fun c => Cells escapedText c ∧ c.length ≤ 13 * (s - a - 1))

theorem row_loop0_spec (out0 out : alloc.vec.Vec U8) (md : Slice U8) (a e s : Usize)
    (hs : a.val + 1 ≤ s.val) (hse : s.val ≤ e.val + 1) (he : e.val ≤ md.val.length)
    (hmd : md.val.length ≤ 2 ^ 20) (hfirst : s.val = a.val + 1 ∨ a.val + 2 ≤ e.val)
    (hroom : out0.val.length + 16 * (e.val - a.val) + 40 ≤ Usize.max)
    (hinv : RowInv out0 a.val out s.val) :
    markdown.row_loop0 out md e s ⦃ r => ∃ s', a.val + 1 ≤ s' ∧ s' ≤ e.val + 1 ∧
      (s' = a.val + 1 ∨ a.val + 2 ≤ e.val) ∧ RowInv out0 a.val r s' ⦄ := by
  unfold markdown.row_loop0
  apply loop.spec_decr_nat (fun st => e.val + 1 - st.2.val)
    (fun st => a.val + 1 ≤ st.2.val ∧ st.2.val ≤ e.val + 1 ∧
      (st.2.val = a.val + 1 ∨ a.val + 2 ≤ e.val) ∧ RowInv out0 a.val st.1 st.2.val)
    _ _ _ _ ⟨hs, hse, hfirst, hinv⟩
  rintro ⟨cur, j⟩ ⟨h1, h2, h3, c0, hc0, hcells, hlen⟩
  dsimp only at h1 h2 h3 hc0 hlen
  have hcur := len_of_bytes hc0
  unfold markdown.row_loop0.body
  step*
  · obtain ⟨w, hw, t, rfl, htl, ht⟩ := out1_post1
    have hc1 : c.val + 1 = s1.val := by scalar_tac
    have hje : j.val < e.val := by scalar_tac
    refine ⟨by omega, by omega, by omega, ⟨c0 ++ us :: t, by rw [hw, hc0, List.append_assoc],
      cells_snoc hcells (by simpa [isHtml] using ht), ?_⟩, by omega⟩
    simp only [List.length_append, List.length_cons]
    omega
  · exact ⟨j.val, h1, h2, h3, c0, hc0, hcells, hlen⟩

theorem row_loop1_spec (out0 out : alloc.vec.Vec U8) (md : Slice U8) (a e s : Usize)
    (hs : a.val + 1 ≤ s.val) (hse : s.val ≤ e.val + 1) (he : e.val ≤ md.val.length)
    (hmd : md.val.length ≤ 2 ^ 20) (hfirst : s.val = a.val + 1 ∨ a.val + 2 ≤ e.val)
    (hroom : out0.val.length + 16 * (e.val - a.val) + 40 ≤ Usize.max)
    (hinv : RowInv out0 a.val out s.val) :
    markdown.row_loop1 out md e s ⦃ r => ∃ s', a.val + 1 ≤ s' ∧ s' ≤ e.val + 1 ∧
      (s' = a.val + 1 ∨ a.val + 2 ≤ e.val) ∧ RowInv out0 a.val r s' ⦄ := by
  unfold markdown.row_loop1
  apply loop.spec_decr_nat (fun st => e.val + 1 - st.2.val)
    (fun st => a.val + 1 ≤ st.2.val ∧ st.2.val ≤ e.val + 1 ∧
      (st.2.val = a.val + 1 ∨ a.val + 2 ≤ e.val) ∧ RowInv out0 a.val st.1 st.2.val)
    _ _ _ _ ⟨hs, hse, hfirst, hinv⟩
  rintro ⟨cur, j⟩ ⟨h1, h2, h3, c0, hc0, hcells, hlen⟩
  dsimp only at h1 h2 h3 hc0 hlen
  have hcur := len_of_bytes hc0
  unfold markdown.row_loop1.body
  step*
  · obtain ⟨w, hw, t, rfl, htl, ht⟩ := out1_post1
    have hc1 : c.val + 1 = s1.val := by scalar_tac
    have hje : j.val < e.val := by scalar_tac
    refine ⟨by omega, by omega, by omega, ⟨c0 ++ us :: t, by rw [hw, hc0, List.append_assoc],
      cells_snoc hcells (by simpa [isHtml] using ht), ?_⟩, by omega⟩
    simp only [List.length_append, List.length_cons]
    omega
  · exact ⟨j.val, h1, h2, h3, c0, hc0, hcells, hlen⟩

theorem row_block (out out3 r : alloc.vec.Vec U8) (a e stop : Nat) (f : Spec.Byte) (s' : Nat)
    (hf : f = ch '0' ∨ f = ch '1')
    (h3 : bytes out3.val = bytes out.val ++ [nl, ch 't', us, f])
    (hs : a + 1 ≤ s') (hse : s' ≤ e + 1) (hfirst : s' = a + 1 ∨ a + 2 ≤ e)
    (hae : a + 1 ≤ e) (hes : e ≤ stop) (hinv : RowInv out3 a r s') :
    BlockOut (16 * (stop - a)) out r ∧ r.val.length ≤ out.val.length + 16 * (stop - a) := by
  obtain ⟨c, hc, hcells, hl⟩ := hinv
  have hw : bytes r.val = bytes out.val ++ ([nl, ch 't', us, f] ++ c) := by
    rw [hc, h3, List.append_assoc]
  have := len_of_bytes hw
  simp only [List.length_append, List.length_cons, List.length_nil] at this
  have hbound : 4 + c.length ≤ 16 * (stop - a) := by
    rcases hfirst with h | h
    · rw [h, show a + 1 - a - 1 = 0 by omega] at hl
      omega
    · omega
  exact ⟨⟨_, hw, Block.row hf hcells, by simp; omega⟩, by omega⟩

@[step]
theorem row_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop : Usize) (header : Bool)
    (ha : a.val < stop.val) (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hbar : ∀ h : a.val < md.val.length, md.val[a.val] = 124#u8)
    (hroom : out.val.length + 16 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.row out md a stop header ⦃ r =>
      BlockOut (16 * (stop.val - a.val)) out r ∧
      r.val.length ≤ out.val.length + 16 * (stop.val - a.val) ⦄ := by
  unfold markdown.row
  step*
  · simp [out2_post]; omega
  · have hae : a.val < e.val := e_post3 (by omega) ha (by rw [hbar]; decide)
    have h3 : out3.val.length = out.val.length + 4 := by
      rw [out3_post, out2_post]; simp; omega
    apply WP.spec_mono (row_loop0_spec out3 out3 md a e s (by omega) (by omega) (by omega) hmd
      (Or.inl s_post) (by omega) ⟨[], by simp, Cells.nil, by simp⟩)
    rintro r ⟨s', h1, h2, h3', hinv⟩
    refine row_block out out3 r a.val e.val stop.val (ch '1') s' (Or.inr rfl) ?_ h1 h2 h3'
      (by omega) (by omega) hinv
    rw [out3_post, bytes_append, out2_post, bytes_append, out1_post1]
    simp [bytes, row_kind]
    exact ⟨rfl, rfl, rfl⟩
  · simp [out2_post]; omega
  · have hae : a.val < e.val := e_post3 (by omega) ha (by rw [hbar]; decide)
    have h3 : out3.val.length = out.val.length + 4 := by
      rw [out3_post, out2_post]; simp; omega
    apply WP.spec_mono (row_loop1_spec out3 out3 md a e s (by omega) (by omega) (by omega) hmd
      (Or.inl s_post) (by omega) ⟨[], by simp, Cells.nil, by simp⟩)
    rintro r ⟨s', h1, h2, h3', hinv⟩
    refine row_block out out3 r a.val e.val stop.val (ch '0') s' (Or.inl rfl) ?_ h1 h2 h3'
      (by omega) (by omega) hinv
    rw [out3_post, bytes_append, out2_post, bytes_append, out1_post1]
    simp [bytes, row_kind]
    exact ⟨rfl, rfl, rfl⟩

/-! ## Lines -/

@[step]
theorem text_state_spec (o : markdown.Open) :
    markdown.text_state o ⦃ st => st = { mode := .Text, «open» := o } ⦄ := by
  unfold markdown.text_state
  step*

@[step]
theorem table_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (a stop next : Usize)
    (ha : a.val < stop.val) (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hbar : ∀ h : a.val < md.val.length, md.val[a.val] = 124#u8)
    (hroom : out.val.length + 16 * (stop.val - a.val) + 64 ≤ Usize.max) :
    markdown.table_line out md a stop next ⦃ r =>
      BlockOrNothing (16 * (stop.val - a.val)) out r ∧
      r.val.length ≤ out.val.length + 16 * (stop.val - a.val) ⦄ := by
  unfold markdown.table_line
  step*
  · exact ⟨⟨[], by simp, Or.inl rfl, by simp⟩, by omega⟩
  · exact ⟨blockOrNothing_block r_post1, r_post2⟩

theorem linePost_of (o o' : markdown.Open) (mode : markdown.Mode) (a b k : Nat)
    (out r : alloc.vec.Vec U8) (h : Appended out r (fun w => LineOut o o' w ∧ w.length ≤ k))
    (hk : k ≤ 16 * (b - a)) :
    LinePost o a b out ({ mode := mode, «open» := o' }, r) := by
  obtain ⟨w, hw, hl, hk'⟩ := h
  have := len_of_bytes hw
  exact ⟨⟨w, hw, hl, Or.inl (by omega)⟩, by dsimp only; omega⟩

@[step]
theorem block_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start a stop next : Usize)
    (o : markdown.Open) (hsa : start.val ≤ a.val) (ha : a.val < stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 16 * (stop.val - start.val) + 64 ≤ Usize.max) :
    markdown.block_line out md (start, a, stop) next o ⦃ res =>
      LinePost o start.val stop.val out res ⦄ := by
  unfold markdown.block_line
  step*
  · rcases mark_end_post with h | ⟨_, _, h⟩
    · exact absurd h (by scalar_tac)
    · exact h
  · subst s_post
    have hm : mark_end.val ≤ stop.val := by
      rcases mark_end_post with h | ⟨_, h, _⟩
      · scalar_tac
      · exact h
    have hma : a.val < mark_end.val := by scalar_tac
    obtain ⟨w, hw, hf, hl⟩ := out1_post1
    exact linePost_of _ _ _ _ _ _ _ _ ⟨w, hw, lineOut_open _ _ hf, hl⟩ (by omega)
  · subst s_post
    exact linePost_of _ _ _ _ _ _ _ _ out1_post1 (by omega)
  · subst s_post
    obtain ⟨w, hw, hb, hl⟩ := out1_post1
    exact linePost_of _ _ _ _ _ _ _ _ ⟨w, hw, lineOut_blockOrNothing _ hb, hl⟩ (by omega)
  · subst s_post
    exact linePost_of _ _ _ _ _ _ _ _ o_post1 (by omega)

@[step]
theorem text_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop next : Usize)
    (o : markdown.Open) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 16 * (stop.val - start.val) + 64 ≤ Usize.max) :
    markdown.text_line out md (start, stop) next o ⦃ res =>
      LinePost o start.val stop.val out res ⦄ := by
  unfold markdown.text_line
  step*
  · subst s_post; exact linePost_same _ _ _ _ _ _ rfl
  · exact linePost_same _ _ _ _ _ _ rfl
  · subst s_post
    have : i.val < stop.val := by scalar_tac
    have : 1 ≤ level.val := by scalar_tac
    exact linePost_block _ _ _ _ _ _ _ _ rfl out1_post1 (by omega)
  · subst s_post
    have : i.val < stop.val := by scalar_tac
    exact linePost_block _ _ _ _ _ _ _ _ rfl out1_post1 (by omega)

@[step]
theorem render_line_spec (out : alloc.vec.Vec U8) (md : Slice U8) (start stop : Usize)
    (state : markdown.State) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ md.val.length) (hmd : md.val.length ≤ 2 ^ 20)
    (hroom : out.val.length + 16 * (stop.val - start.val) + 64 ≤ Usize.max) :
    markdown.render_line out md start stop state ⦃ res =>
      LinePost state.open start.val stop.val out res ⦄ := by
  obtain ⟨mode, o⟩ := state
  unfold markdown.render_line
  induction mode <;> step*

/-! ## The whole reply -/

def MainInv (md : Slice U8) (st : alloc.vec.Vec U8 × markdown.State × Usize) : Prop :=
  st.2.2.val ≤ md.val.length + 1 ∧
  (∃ s, bytes st.1.val = marker ++ s ∧ Inv st.2.1.open s) ∧
  st.1.val.length + 1 ≤ 16 * min st.2.2.val md.val.length + 4

theorem render_markdown_loop_spec (md : Slice U8) (out : alloc.vec.Vec U8) (state : markdown.State)
    (start : Usize) (hmd : md.val.length ≤ 2 ^ 20) (hinv : MainInv md (out, state, start)) :
    markdown.render_markdown_loop md out state start ⦃ r =>
      (∃ s, bytes r.val = marker ++ s ∧ Blocks s) ∧
      r.val.length + 1 ≤ 16 * md.val.length + 4 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold markdown.render_markdown_loop
  apply loop.spec_decr_nat (fun st => md.val.length + 1 - st.2.2.val) (MainInv md) _ _ _ _ hinv
  rintro ⟨cur, st, i⟩ ⟨hi, ⟨s, hs, hinv⟩, hlen⟩
  dsimp only at hi hs hinv hlen
  unfold markdown.render_markdown_loop.body
  step*
  · have hil : i.val < md.val.length := by scalar_tac
    have he : «end».val ≤ md.val.length := end_post2 (by omega)
    obtain ⟨⟨w, hw, hlo, hwl⟩, _⟩ := state1_post
    dsimp only at hw hlo
    have hl := len_of_bytes hw
    have hs1 : start1.val = «end».val + 1 := by scalar_tac
    have hie : i.val ≤ «end».val := end_post1
    dsimp only [MainInv]
    refine ⟨⟨by omega, ⟨s ++ w, by rw [hw, hs, List.append_assoc], hlo s hinv⟩, ?_⟩, by omega⟩
    omega
  · exact ⟨⟨s, hs, hinv.1⟩, by have : md.val.length ≤ i.val := by scalar_tac
                               omega⟩

/-- The marker and whole blocks. -/
def Body (r : alloc.vec.Vec U8) : Prop := ∃ s, bytes r.val = marker ++ s ∧ Blocks s

@[step]
theorem render_markdown_loop_start_spec (md : Slice U8) (out : alloc.vec.Vec U8)
    (hmd : md.val.length ≤ 2 ^ 20) (hout : bytes out.val = marker) (hlen : out.val.length = 3) :
    markdown.render_markdown_loop md out { mode := .Text, «open» := .Nothing } 0#usize ⦃ r =>
      Body r ∧ r.val.length + 1 ≤ 16 * md.val.length + 4 ⦄ := by
  apply WP.spec_mono (render_markdown_loop_spec md out _ 0#usize hmd
    ⟨by simp, ⟨[], by simpa using hout, inv_start⟩, by simp [hlen]⟩)
  intro r ⟨⟨s, hs, hb⟩, hl⟩
  exact ⟨⟨s, hs, hb⟩, hl⟩

theorem marker_bytes : bytes (Array.to_slice markdown.MARKER).val = marker := by
  unfold markdown.MARKER; rfl

theorem rendered_of_blocks {s : List Spec.Byte} (h : Blocks s) :
    Rendered escapedText (marker ++ s ++ [nl]) := by
  obtain ⟨bs, hbs, rfl⟩ := h
  exact ⟨bs, hbs, rfl⟩

/-- **S22, S24, S25.** For every Markdown text of at most 1 MiB: no panic, the output is the
marker, blocks, and `\n`, every text reads as escaped tokens, and the size stays bounded. -/
theorem render_markdown_spec (md : Slice U8) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.render_markdown md ⦃ v =>
      Rendered escapedText (bytes v.val) ∧ v.val.length ≤ 16 * md.val.length + 4 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold markdown.render_markdown
  step*
  subst s_post state_post
  have hout : bytes out.val = marker := by rw [out_post1, List.nil_append, marker_bytes]
  have hlen : out.val.length = 3 := by simp [out_post2]
  step*
  obtain ⟨s, hs, hb⟩ := out1_post1
  refine ⟨?_, by simp [v_post]; omega⟩
  rw [v_post, bytes_append, hs]
  exact rendered_of_blocks hb

/-! ## S23 from S24: an escaped text holds only field bytes -/

theorem codes_field : ∀ c ∈ colorCodes, ∀ b ∈ c, fieldByte b := by
  simp only [colorCodes, List.mem_cons, List.mem_nil_iff, or_false, forall_eq_or_imp, forall_eq,
    boldCode, italicCode, boldItalicCode, codeCode, linkCode, fieldByte]
  decide

theorem entities_field : ∀ e ∈ entities, ∀ b ∈ e, fieldByte b := by
  simp only [entities, List.mem_cons, List.mem_nil_iff, or_false, forall_eq_or_imp, forall_eq,
    fieldByte]
  decide

theorem reset_field : ∀ b ∈ resetCode, fieldByte b := by
  simp only [resetCode, fieldByte]
  decide

theorem text_field {html o o' : Bool} {t : List Spec.Byte} (h : Text html o t o') :
    ∀ b ∈ t, fieldByte b := by
  induction h with
  | nil => simp
  | plain hb _ ih =>
    intro b hmem
    rcases List.mem_cons.mp hmem with rfl | h
    · exact hb.1
    · exact ih b h
  | pipes _ ih =>
    intro b hmem
    simp only [List.mem_cons] at hmem
    rcases hmem with rfl | rfl | h
    · simp [fieldByte, pipe]; decide
    · simp [fieldByte, pipe]; decide
    · exact ih b h
  | entity _ he _ ih =>
    intro b hmem
    rcases List.mem_append.mp hmem with h | h
    · exact entities_field _ he b h
    · exact ih b h
  | color hc _ ih =>
    intro b hmem
    rcases List.mem_append.mp hmem with h | h
    · exact codes_field _ hc b h
    · exact ih b h
  | reset _ ih =>
    intro b hmem
    rcases List.mem_append.mp hmem with h | h
    · exact reset_field b h
    · exact ih b h

theorem cells_mono {P Q : Bool → List Spec.Byte → Prop} (hpq : ∀ h t, P h t → Q h t)
    {c : List Spec.Byte} (hc : Cells P c) : Cells Q c := by
  induction hc with
  | nil => exact Cells.nil
  | cons ht _ ih => exact Cells.cons (hpq _ _ ht) ih

theorem block_mono {P Q : Bool → List Spec.Byte → Prop} (hpq : ∀ h t, P h t → Q h t)
    {w : List Spec.Byte} (hw : Block P w) : Block Q w := by
  cases hw with
  | heading hl ht => exact Block.heading hl (hpq _ _ ht)
  | paragraph ht => exact Block.paragraph (hpq _ _ ht)
  | item hl hd hdd ht => exact Block.item hl hd hdd (hpq _ _ ht)
  | quote ht => exact Block.quote (hpq _ _ ht)
  | code ht => exact Block.code (hpq _ _ ht)
  | row hf hc => exact Block.row hf (cells_mono hpq hc)
  | rule => exact Block.rule

/-- **S23** follows from S24. -/
theorem rendered_fields {out : List Spec.Byte} (h : Rendered escapedText out) :
    Rendered fieldText out := by
  obtain ⟨bs, hbs, rfl⟩ := h
  exact ⟨bs, fun b hb => block_mono (fun _ _ ht => text_field ht) (hbs b hb), rfl⟩

/-! ## The four statements -/

/-- **S22.** -/
theorem render_total (md : Slice U8) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.render_markdown md ⦃ _ => True ⦄ :=
  WP.spec_mono (render_markdown_spec md hmd) (fun _ _ => trivial)

/-- **S23.** -/
theorem render_shape (md : Slice U8) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.render_markdown md ⦃ v => Rendered fieldText (bytes v.val) ⦄ :=
  WP.spec_mono (render_markdown_spec md hmd) (fun _ h => rendered_fields h.1)

/-- **S24.** -/
theorem render_escape (md : Slice U8) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.render_markdown md ⦃ v => Rendered escapedText (bytes v.val) ⦄ :=
  WP.spec_mono (render_markdown_spec md hmd) (fun _ h => h.1)

/-- **S25.** -/
theorem render_bound (md : Slice U8) (hmd : md.val.length ≤ 2 ^ 20) :
    markdown.render_markdown md ⦃ v => v.val.length ≤ 16 * md.val.length + 4 ⦄ :=
  WP.spec_mono (render_markdown_spec md hmd) (fun _ h => h.2)

end Protocol.Markdown
