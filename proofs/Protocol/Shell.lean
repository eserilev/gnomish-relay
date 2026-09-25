import Protocol.CommandRules

/-! # The shell splitter never panics (S27)

Every step of the lexer consumes at least one byte and adds at most two items to its
lists. So the lists stay below twice the input length, and no push overflows.
-/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.PathRules

namespace Protocol.Shell

/-- All items in the lists of the lexer, and the depth of `(`. -/
def load (lx : shell.Lexer) : Nat :=
  lx.word.val.length + lx.words.val.length + lx.simples.val.length + lx.redirects.val.length + lx.depth.val

@[simp, scalar_tac_simps]
theorem load_eq (lx : shell.Lexer) :
    load lx = lx.word.val.length + lx.words.val.length + lx.simples.val.length +
      lx.redirects.val.length + lx.depth.val := rfl

@[step]
theorem fail_spec (lx : shell.Lexer) : shell.fail lx ⦃ r => load r = load lx ⦄ := by
  unfold shell.fail; step*

@[step]
theorem add_byte_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 22) :
    shell.add_byte lx b ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.add_byte
  step*
  simp only [load_eq, v_post, List.length_append, List.length_singleton]
  omega

@[step]
theorem is_digit_spec (b : U8) : shell.is_digit b ⦃ _ => True ⦄ := by
  unfold shell.is_digit; step*

@[step]
theorem all_digits_spec (s : Slice U8) : shell.all_digits s ⦃ _ => True ⦄ := by
  unfold shell.all_digits shell.all_digits_loop
  apply loop.spec_decr_nat (fun st => s.val.length - st.2.val) (fun st => st.2.val ≤ s.val.length) _ _ _ _
    (by simp)
  rintro ⟨digits, i⟩ hi
  simp only at hi
  unfold shell.all_digits_loop.body
  step*

@[step]
theorem is_descriptor_spec (s : Slice U8) : shell.is_descriptor s ⦃ _ => True ⦄ := by
  unfold shell.is_descriptor; step*

@[simp, scalar_tac_simps]
theorem reserved_len : (Array.to_slice shell.RESERVED).val.length = 87 := by
  unfold shell.RESERVED; rfl

@[step]
theorem bad_command_name_spec (lx : shell.Lexer) : shell.bad_command_name lx ⦃ _ => True ⦄ := by
  unfold shell.bad_command_name
  step*
  split <;> step*

@[step]
theorem take_word_spec (lx : shell.Lexer) :
    shell.take_word lx ⦃ r => load r.1 + r.2.val.length = load lx ⦄ := by
  unfold shell.take_word
  step*

@[step]
theorem pending_eq_spec (a b : shell.Pending) :
    shell.Pending.Insts.CoreCmpPartialEqPending.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [shell.Pending.Insts.CoreCmpPartialEqPending.eq, shell.Pending.read_discriminant]

@[step]
theorem mode_eq_spec (a b : shell.Mode) :
    shell.Mode.Insts.CoreCmpPartialEqMode.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [shell.Mode.Insts.CoreCmpPartialEqMode.eq, shell.Mode.read_discriminant]

@[step]
theorem end_word_spec (lx : shell.Lexer) (h : load lx ≤ 2 ^ 22) :
    shell.end_word lx ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.end_word
  step*
  all_goals
    simp only [load_eq, v_post, List.length_append, List.length_singleton] at *
    omega

@[step]
theorem end_simple_spec (lx : shell.Lexer) (next : shell.Link) (h : load lx ≤ 2 ^ 22) :
    shell.end_simple lx next ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.end_simple
  step*
  · exact pending_eq_spec _ _
  · split
    · step*
      all_goals
        simp only [load_eq, x_post, List.length_append, List.length_singleton] at *
        simp only [List.length_nil]
        scalar_tac
    · step*

@[step]
theorem end_word_before_redirect_spec (lx : shell.Lexer) (h : load lx ≤ 2 ^ 22) :
    shell.end_word_before_redirect lx ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.end_word_before_redirect
  step*

@[step]
theorem open_redirect_spec (lx : shell.Lexer) (pending : shell.Pending) (h : load lx ≤ 2 ^ 22) :
    shell.open_redirect lx pending ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.open_redirect
  step*
  exact pending_eq_spec _ _

@[step]
theorem open_group_spec (lx : shell.Lexer) (h : load lx ≤ 2 ^ 21) :
    shell.open_group lx ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.open_group
  step*

@[step]
theorem close_group_spec (lx : shell.Lexer) (h : load lx ≤ 2 ^ 21) :
    shell.close_group lx ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.close_group
  step*

@[step]
theorem peek_spec (raw : Slice U8) (i : Usize) :
    shell.peek raw i ⦃ r => r.val ≠ 0 → i.val < raw.val.length ⦄ := by
  unfold shell.peek
  step*

/-- A step moves forward and stays inside the command. -/
def Moves (raw : Slice U8) (i j : Usize) : Prop := i.val < j.val ∧ j.val ≤ raw.val.length

@[step]
theorem ampersand_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.ampersand lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.ampersand
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem bar_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.bar lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.bar
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem greater_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.greater lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.greater
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem less_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.less lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.less
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem brace_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.brace lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.brace
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem glob_byte_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 22) :
    shell.glob_byte lx b ⦃ r => load r ≤ load lx + 1 ⦄ := by
  unfold shell.glob_byte
  step*

@[step]
theorem set_mode_spec (lx : shell.Lexer) (m : shell.Mode) :
    shell.set_mode lx m ⦃ r => load r = load lx ⦄ := by
  unfold shell.set_mode
  step*

@[step]
theorem start_quote_spec (lx : shell.Lexer) (m : shell.Mode) :
    shell.start_quote lx m ⦃ r => load r = load lx ⦄ := by
  unfold shell.start_quote
  step*

@[step]
theorem plain_single_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 21) :
    shell.plain_single lx b ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.plain_single
  step*

@[step]
theorem plain_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.plain lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.plain
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[step]
theorem single_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 21) :
    shell.single lx b ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.single
  step*

@[step]
theorem double_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 21) :
    shell.double lx b ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.double
  step*

@[step]
theorem double_escape_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 21) :
    shell.double_escape lx b ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.double_escape
  step*

@[step]
theorem escape_spec (lx : shell.Lexer) (b : U8) (h : load lx ≤ 2 ^ 21) :
    shell.escape lx b ⦃ r => load r ≤ load lx + 2 ⦄ := by
  unfold shell.escape
  step*

@[step]
theorem step_spec (lx : shell.Lexer) (raw : Slice U8) (i : Usize) (h : load lx ≤ 2 ^ 21)
    (hi : i.val < raw.val.length) :
    shell.step lx raw i ⦃ r => load r.1 ≤ load lx + 2 ∧ Moves raw i r.2 ⦄ := by
  unfold shell.step
  step*
  all_goals unfold Moves
  all_goals scalar_tac

@[simp, scalar_tac_simps]
theorem max_command_val : shell.MAX_COMMAND.val = 2 ^ 20 := by unfold shell.MAX_COMMAND; rfl

theorem run_loop_spec (raw : Slice U8) (lx : shell.Lexer) (hraw : raw.val.length ≤ 2 ^ 20)
    (h : load lx = 0) :
    shell.run_loop raw lx 0#usize ⦃ r =>
      load ⟨r.1, r.2.1, r.2.2.1, r.2.2.2.1, r.2.2.2.2.1, r.2.2.2.2.2.1, r.2.2.2.2.2.2.1,
        r.2.2.2.2.2.2.2.1, r.2.2.2.2.2.2.2.2.1, r.2.2.2.2.2.2.2.2.2.1, r.2.2.2.2.2.2.2.2.2.2⟩ ≤ 2 ^ 21 ⦄ := by
  unfold shell.run_loop
  apply loop.spec_decr_nat (fun st => raw.val.length - st.2.val)
    (fun st => st.2.val ≤ raw.val.length ∧ load st.1 ≤ 2 * st.2.val) _ _ _ _ ⟨by simp, by rw [h]; simp⟩
  rintro ⟨lx, i⟩ ⟨hi, hload⟩
  simp only at hi hload
  unfold shell.run_loop.body
  step*
  all_goals try unfold Moves at *
  all_goals scalar_tac

@[step]
theorem new_lexer_spec : shell.new_lexer ⦃ lx => load lx = 0 ⦄ := by
  unfold shell.new_lexer
  step*

@[step]
theorem run_spec (raw : Slice U8) (hraw : raw.val.length ≤ 2 ^ 20) :
    shell.run raw ⦃ r => load r ≤ 2 ^ 21 ⦄ := by
  unfold shell.run
  step*
  step with run_loop_spec raw lx hraw lx_post as ⟨res, hres⟩
  step*

@[step]
theorem finish_spec (lx : shell.Lexer) (h : load lx ≤ 2 ^ 21) : shell.finish lx ⦃ _ => True ⦄ := by
  unfold shell.finish
  step*
  exact mode_eq_spec _ _

/-! ## The quote state (S28) -/

theorem bv_eq_iff (b k : U8) (c : Char) (hk : k.bv = ch c) : b.bv = ch c ↔ b = k := by
  rw [← hk]; exact (UScalar.eq_equiv_bv_eq b k).symm

@[step]
theorem quote_step_spec (m : shell.Mode) (b : U8) :
    shell.quote_step m b ⦃ r => r = quoteStep m b.bv ⦄ := by
  unfold shell.quote_step
  have h39 := bv_eq_iff b 39#u8 '\'' (by decide)
  have h34 := bv_eq_iff b 34#u8 '"' (by decide)
  have h92 := bv_eq_iff b 92#u8 '\\' (by decide)
  induction m <;> step* <;> simp only [quoteStep, h39, h34, h92] <;> simp_all

@[step]
theorem opens_substitution_spec (raw : Slice U8) (i : Usize) (h : i.val < raw.val.length) :
    shell.opens_substitution raw i ⦃ r => (r = true ↔ opensAt (bytes raw.val) i.val) ⦄ := by
  unfold shell.opens_substitution
  have hi : (bytes raw.val)[i.val]? = some (raw.val[i.val].bv) := by simp [bytes, h]
  have h96 := bv_eq_iff (raw.val[i.val]) 96#u8 '`' (by decide)
  have h36 := bv_eq_iff (raw.val[i.val]) 36#u8 '$' (by decide)
  step*
  · simp only [true_iff, opensAt, hi, Option.some.injEq, h96]
    left; rw [← i1_post]; assumption
  · have hlt : i.val + 1 < raw.val.length := by scalar_tac
    have hj : (bytes raw.val)[i.val + 1]? = some (raw.val[i.val + 1]).bv := by
      simp only [bytes, List.getElem?_map, List.getElem?_eq_getElem hlt, Option.map_some]
    have h40 := bv_eq_iff (raw.val[i.val + 1]) 40#u8 '(' (by decide)
    have e : i4 = raw.val[i.val + 1] := by rw [i4_post]; simp [i2_post]
    simp only [opensAt, hi, hj, Option.some.injEq, h96, h36, h40, decide_eq_true_iff, e]
    simp_all
  · have hj : (bytes raw.val)[i.val + 1]? = none := by simp [bytes]; scalar_tac
    simp only [Bool.false_eq_true, false_iff, opensAt, hi, hj, Option.some.injEq, h96]
    simp_all
  · have hj : True := trivial
    simp only [Bool.false_eq_true, false_iff, opensAt, hi, Option.some.injEq, h96, h36]
    simp_all

/-- **S28, substitution.** `has_substitution` follows the quote state of the splitter. -/
theorem has_substitution_spec (raw : Slice U8) :
    shell.has_substitution raw ⦃ r => (r = true ↔ substitution (bytes raw.val)) ⦄ := by
  unfold shell.has_substitution shell.has_substitution_loop
  apply loop.spec_decr_nat (fun st => raw.val.length - st.2.2.val)
    (fun st => st.2.2.val ≤ raw.val.length ∧ st.1 = modeAt (bytes raw.val) st.2.2.val ∧
      (st.2.1 = true ↔ ∃ i < st.2.2.val, modeAt (bytes raw.val) i ≠ .Single ∧ opensAt (bytes raw.val) i))
    _ _ _ _ ⟨by simp, by simp [modeAt], by simp⟩
  rintro ⟨mode, found, i⟩ ⟨hi, hmode, hfound⟩
  simp only at hi hmode hfound
  unfold shell.has_substitution_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨j, hj, h⟩ := hfound.mp (by assumption)
    exact ⟨j, by simp [bytes]; omega, h⟩
  · exact mode_eq_spec _ _
  · have hlt : i.val < raw.val.length := by scalar_tac
    have hnot : ¬ ∃ j < i.val, modeAt (bytes raw.val) j ≠ .Single ∧ opensAt (bytes raw.val) j := by
      rw [← hfound]; assumption
    have hnext : modeAt (bytes raw.val) (i.val + 1) = quoteStep (modeAt (bytes raw.val) i.val) raw.val[i.val].bv := by
      unfold modeAt
      rw [List.take_add_one, List.foldl_append]
      simp [bytes, hlt]
    have hstep : ∀ found1 : Bool, (found1 = true ↔ modeAt (bytes raw.val) i.val ≠ .Single ∧
        opensAt (bytes raw.val) i.val) →
        (found1 = true ↔ ∃ j < i.val + 1, modeAt (bytes raw.val) j ≠ .Single ∧ opensAt (bytes raw.val) j) := by
      intro f hf
      rw [hf]
      constructor
      · intro h; exact ⟨i.val, by omega, h⟩
      · rintro ⟨j, hj, h⟩
        by_cases hji : j < i.val
        · exact absurd ⟨j, hji, h⟩ hnot
        · have : j = i.val := by omega
          subst this; exact h
    split
    · step*
      have hb := b_post.mp (by assumption)
      have hf : found1 = true ↔ modeAt (bytes raw.val) i.val ≠ .Single ∧ opensAt (bytes raw.val) i.val := by
        rw [found1_post, ← hmode]; simp [hb]
      refine ⟨by scalar_tac, ?_, ?_, by scalar_tac⟩
      · rw [mode1_post, i3_post, hnext, hmode, i2_post]
      · rw [i3_post]; exact hstep _ hf
    · step*
      have hb : mode = .Single := by
        have := (by assumption : ¬ b = true)
        rw [b_post] at this
        simpa using this
      have hf : false = true ↔ modeAt (bytes raw.val) i.val ≠ .Single ∧ opensAt (bytes raw.val) i.val := by
        rw [← hmode]; simp [hb]
      refine ⟨by scalar_tac, ?_, ?_, by scalar_tac⟩
      · rw [mode1_post, i3_post, hnext, hmode, i2_post]
      · rw [i3_post]; exact hstep _ hf
  · have : i.val = raw.val.length := by scalar_tac
    simp only [Bool.false_eq_true, false_iff, substitution, not_exists, not_and]
    intro j hj h1 h2
    have hn : ¬ ∃ k < i.val, modeAt (bytes raw.val) k ≠ .Single ∧ opensAt (bytes raw.val) k := by
      rw [← hfound]; assumption
    exact hn ⟨j, by simp [bytes] at hj; omega, h1, h2⟩

/-- **S27, splitter.** `split` returns for every input. -/
theorem split_spec (raw : Slice U8) : shell.split raw ⦃ _ => True ⦄ := by
  unfold shell.split
  step*

end Protocol.Shell

