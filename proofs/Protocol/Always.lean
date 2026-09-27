import Protocol.Action
import Protocol.Spec.Always

/-! # "Always allow" (S36 to S39)

`propose` makes the rule of one simple command, and `offer` the rules of one click.
-/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.CommandRules

namespace Protocol.Always

/-! ## The lists -/

theorem subcommand_bytes :
    bytes (Array.to_slice always.SUBCOMMAND_TOOLS).val = ascii subcommandNames := by
  unfold always.SUBCOMMAND_TOOLS; rfl
theorem no_rule_tools_bytes :
    bytes (Array.to_slice always.NO_RULE_TOOLS).val = ascii noRuleNames := by
  unfold always.NO_RULE_TOOLS; rfl
theorem no_rule_pairs_bytes :
    bytes (Array.to_slice always.NO_RULE_PAIRS).val = ascii noRulePairs := by
  unfold always.NO_RULE_PAIRS; rfl
theorem special_bytes : bytes (Array.to_slice always.SPECIAL).val = ascii specialChars := by
  unfold always.SPECIAL; rfl

@[simp, scalar_tac_simps]
theorem no_rule_pairs_len : (Array.to_slice always.NO_RULE_PAIRS).val.length = 101 := by
  unfold always.NO_RULE_PAIRS; rfl

/-! ## Plain words -/

theorem mem_bytes_iff (l : List U8) (b : U8) : b ∈ l ↔ b.bv ∈ bytes l := by
  simp only [bytes, List.mem_map]
  constructor
  · intro h; exact ⟨b, h, rfl⟩
  · rintro ⟨x, hx, he⟩
    have : x = b := by
      cases x; cases b; simp_all
    rw [← this]; exact hx

@[step]
theorem is_plain_byte_spec (b : U8) :
    always.is_plain_byte b ⦃ r => (r = true ↔ ruleByte b.bv) ⦄ := by
  unfold always.is_plain_byte
  step*
  all_goals simp only [ruleByte, UScalar.bv_toNat]
  · rw [mem_bytes_iff, s_post, special_bytes] at b1_post
    simp_all
  all_goals simp_all

@[step]
theorem all_plain_bytes_spec (word : Slice U8) :
    always.all_plain_bytes word ⦃ r => (r = true ↔ ∀ b ∈ word.val, ruleByte b.bv) ⦄ := by
  unfold always.all_plain_bytes always.all_plain_bytes_loop
  apply loop.spec_decr_nat (fun st => word.val.length - st.2.val)
    (fun st => st.2.val ≤ word.val.length ∧
      (st.1 = true ↔ ∀ b ∈ word.val.take st.2.val, ruleByte b.bv)) _ _ _ _
    ⟨by simp, by simp⟩
  rintro ⟨plain, i⟩ ⟨hi, hplain⟩
  simp only at hi hplain
  unfold always.all_plain_bytes_loop.body
  step*
  · have hlt : i.val < word.val.length := by scalar_tac
    have hall := hplain.mp (by assumption)
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt, plain1_post]
    simp only [Option.toList_some, List.mem_append, List.mem_singleton]
    constructor
    · rintro h b (hb | rfl)
      · exact hall b hb
      · rw [← i2_post]; exact h
    · intro h
      rw [i2_post]
      exact h _ (Or.inr rfl)
  · have : i.val = word.val.length := by scalar_tac
    rw [this, List.take_length] at hplain
    simp only [true_iff]
    exact hplain.mp (by assumption)
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have hn : ¬ plain = true := by assumption
    exact hn (hplain.mpr (fun b hb => h b (List.mem_of_mem_take hb)))

theorem bv_ne_iff (x : U8) (c : Char) (n : Nat) (hc : c.toNat = n) (hn : n < 256) :
    x.bv ≠ ch c ↔ x.val ≠ n := not_congr (Protocol.CommandRules.byte_bv_iff x c n hc hn)

@[simp, scalar_tac_simps]
theorem max_word_val : always.MAX_WORD.val = 64 := by unfold always.MAX_WORD; rfl

theorem plainWord_iff (l : List U8) :
    plainWord (bytes l) ↔ 1 ≤ l.length ∧ l.length ≤ 64 ∧
      (∀ x, l.head? = some x → x.val ≠ 45 ∧ x.val ≠ 43) ∧ ∀ b ∈ l, ruleByte b.bv := by
  cases l with
  | nil => simp [plainWord, bytes]
  | cons x xs =>
    have e1 := Protocol.CommandRules.byte_bv_iff x '-' 45 rfl (by omega)
    have e2 := Protocol.CommandRules.byte_bv_iff x '+' 43 rfl (by omega)
    simp only [plainWord, bytes, List.map_cons, List.head?_cons, ne_eq, Option.some.injEq,
      List.length_cons, List.length_map, List.forall_mem_cons, List.forall_mem_map, forall_eq']
    rw [e1, e2]
    tauto

@[step]
theorem is_plain_word_spec (word : Slice U8) :
    always.is_plain_word word ⦃ r => (r = true ↔ plainWord (bytes word.val)) ⦄ := by
  unfold always.is_plain_word
  step*
  all_goals rw [plainWord_iff]
  · obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos (by scalar_tac : 0 < word.val.length)
    have hlen : word.val.length ≤ 64 := by scalar_tac
    have h45 : i2.val ≠ 45 := by
      intro h; have : (i2 != 45#u8) = true := by assumption
      simp_all
    have h43 : i2.val ≠ 43 := by
      intro h; have : (i2 != 43#u8) = true := by assumption
      simp_all
    rw [r_post]
    simp only [hx, List.getElem_cons_zero] at i2_post hlen ⊢
    subst i2_post
    simp only [List.head?_cons, Option.some.injEq, forall_eq', List.length_cons]
    constructor
    · intro h; exact ⟨by omega, hlen, ⟨h45, h43⟩, h⟩
    · intro h; exact h.2.2.2
  all_goals simp only [Bool.false_eq_true, false_iff, not_and]
  · intro _ _ h _
    obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos (by scalar_tac : 0 < word.val.length)
    simp only [hx, List.getElem_cons_zero, List.head?_cons, Option.some.injEq, forall_eq'] at h i2_post
    subst i2_post
    have : ¬(i2 != 43#u8) = true := by assumption
    simp only [bne_iff_ne, ne_eq, not_not] at this
    exact h.2 (by rw [this]; rfl)
  · intro _ _ h _
    obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos (by scalar_tac : 0 < word.val.length)
    simp only [hx, List.getElem_cons_zero, List.head?_cons, Option.some.injEq, forall_eq'] at h i2_post
    subst i2_post
    have : ¬(i2 != 45#u8) = true := by assumption
    simp only [bne_iff_ne, ne_eq, not_not] at this
    exact h.1 (by rw [this]; rfl)
  · intro _ h; scalar_tac
  · intro h; scalar_tac

/-! ## Tools with no rule -/

theorem listed_length (names : String) (n : List Spec.Byte) (h : listed names n) :
    n.length + 2 ≤ (ascii names).length := by
  have := h.length_le
  simp at this
  omega

theorem no_rule_pairs_length : (ascii noRulePairs).length = 101 := by rfl

@[step]
theorem is_short_spec (nm : Slice U8) :
    always.is_short nm ⦃ r => (r = true ↔ nm.val.length < 101) ⦄ := by
  unfold always.is_short
  step*

@[step]
theorem pair_of_spec (nm second : Slice U8) (h1 : nm.val.length < 101)
    (h2 : second.val.length < 101) :
    always.pair_of nm second ⦃ v => bytes v.val = bytes nm.val ++ [ch ':'] ++ bytes second.val ⦄ := by
  unfold always.pair_of
  step*
  case hroom => scalar_tac
  all_goals simp_all [bytes]; rfl

theorem strs_get_one (l : List (alloc.vec.Vec U8)) (h : 1 < l.length) :
    (strs l)[1]? = some (bytes l[1].val) := by
  simp [strs, List.getElem?_eq_getElem h]

theorem strs_get_one_none (l : List (alloc.vec.Vec U8)) (h : ¬ 1 < l.length) :
    (strs l)[1]? = none := by
  simp [strs]; omega

theorem bytes_length (l : List U8) : (bytes l).length = l.length := by simp [bytes]

theorem pair_too_long (a b : List Spec.Byte) (h : 101 ≤ a.length ∨ 101 ≤ b.length) :
    ¬ listed noRulePairs (a ++ [ch ':'] ++ b) := by
  intro hl
  have := listed_length _ _ hl
  rw [no_rule_pairs_length] at this
  simp at this
  omega

@[step]
theorem is_no_rule_pair_spec (nm : Slice U8) (words : Slice (alloc.vec.Vec U8)) :
    always.is_no_rule_pair nm words ⦃ r => (r = true ↔ ∃ w, (strs words.val)[1]? = some w ∧
      listed noRulePairs (bytes nm.val ++ [ch ':'] ++ progName w)) ⦄ := by
  unfold always.is_no_rule_pair
  step*
  · rw [strs_get_one_none _ (by scalar_tac)]
    simp
  · rw [strs_get_one _ (by scalar_tac)]
    simp only [Option.some.injEq, exists_eq_left', alloc.vec.Vec.deref] at *
    rw [r_post, Protocol.CommandRules.listed_iff _ _ noRulePairs (by rw [s2_post]; exact no_rule_pairs_bytes),
      v1_post, second_post1, v_post]
  · have hs : ¬ (alloc.vec.Vec.deref second).val.length < 101 := by rw [← b1_post]; assumption
    simp only [alloc.vec.Vec.deref] at hs
    rw [strs_get_one _ (by scalar_tac)]
    simp only [Option.some.injEq, exists_eq_left', alloc.vec.Vec.deref, Bool.false_eq_true,
      false_iff] at *
    apply pair_too_long
    right
    rw [← v_post, ← second_post1, bytes_length]
    omega
  · have hs : ¬ nm.val.length < 101 := by rw [← b_post]; assumption
    simp only [Bool.false_eq_true, false_iff, not_exists, not_and]
    intro w _
    apply pair_too_long
    left
    rw [bytes_length]
    omega

@[step]
theorem is_no_rule_tool_spec (words : Slice (alloc.vec.Vec U8)) :
    always.is_no_rule_tool words ⦃ r => (r = true ↔ noRuleTool (strs words.val)) ⦄ := by
  unfold always.is_no_rule_tool
  step*
  · simp only [Bool.false_eq_true, false_iff, noRuleTool, not_exists, not_and]
    intro h hh
    rw [strs_head_nil _ (by scalar_tac)] at hh
    simp at hh
  all_goals
    simp only [noRuleTool, strs_head _ (by scalar_tac : 0 < words.val.length), Option.some.injEq,
      exists_eq_left', alloc.vec.Vec.deref] at *
  · simp only [true_iff]
    left
    rw [Protocol.CommandRules.listed_iff _ _ noRuleNames (by rw [s1_post]; exact no_rule_tools_bytes),
      name_post1, v_post] at b_post
    exact b_post.mp (by assumption)
  · have h0 : 0 < words.val.length := by scalar_tac
    have hn : ¬ listed noRuleNames (progName (bytes words.val[0].val)) := by
      rw [Protocol.CommandRules.listed_iff _ _ noRuleNames (by rw [s1_post]; exact no_rule_tools_bytes),
        name_post1, v_post] at b_post
      rw [← b_post]; assumption
    rw [r_post, name_post1, v_post]
    simp only [hn, false_or]

@[step]
theorem has_subcommands_spec (word : Slice U8) :
    always.has_subcommands word ⦃ r => (r = true ↔ listed subcommandNames (progName (bytes word.val))) ⦄ := by
  unfold always.has_subcommands
  step*
  rw [r_post, Protocol.CommandRules.listed_iff _ _ subcommandNames (by rw [s_post]; exact subcommand_bytes)]
  simp only [alloc.vec.Vec.deref] at *
  rw [v_post1]

/-! ## The rule of one simple command -/

theorem vec_eq_of_val {α : Type} (a b : alloc.vec.Vec α) (h : a.val = b.val) : a = b :=
  Subtype.ext h

@[step]
theorem first_words_spec (words : Slice (alloc.vec.Vec U8)) (n : Usize)
    (hn : n.val ≤ words.val.length) :
    always.first_words words n ⦃ r => r.val = words.val.take n.val ⦄ := by
  unfold always.first_words always.first_words_loop
  apply loop.spec_decr_nat (fun st => n.val - st.2.val)
    (fun st => st.2.val ≤ n.val ∧ st.1.val = words.val.take st.2.val) _ _ _ _
    ⟨by simp, by simp⟩
  rintro ⟨rule, i⟩ ⟨hi, hrule⟩
  simp only at hi hrule
  unfold always.first_words_loop.body
  step*
  · have hlt : i.val < words.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [rule1_post, hrule, i1_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [Option.toList_some, List.append_cancel_left_eq, List.cons.injEq, and_true]
    apply vec_eq_of_val
    simp only [alloc.vec.Vec.deref] at v1_post
    rw [v1_post, v_post]

theorem words_eq (s : shell.Simple) : words s = strs s.words.val := rfl

@[step]
theorem is_ruled_out_spec (simple : shell.Simple) :
    always.is_ruled_out simple ⦃ r => (r = true ↔ simple.words.val = [] ∨ desktopSimple simple ∨
      neverAlways (words simple) ∨ noRuleTool (words simple)) ⦄ := by
  unfold always.is_ruled_out
  step*
  all_goals simp only [alloc.vec.Vec.deref, words_eq] at *
  · simp only [true_iff]; left; apply List.eq_nil_of_length_eq_zero; scalar_tac
  all_goals
    have hne : simple.words.val ≠ [] := by
      intro h; have : simple.words.val.length = 0 := by rw [h]; rfl
      scalar_tac
    simp_all

/-- A simple command that gets no rule, before any look at its words. -/
def RuledOut (s : shell.Simple) : Prop :=
  s.words.val = [] ∨ desktopSimple s ∨ neverAlways (words s) ∨ noRuleTool (words s)

/-- The shape of S36, with the bound that makes the rule cover its command. -/
def Shape (s : shell.Simple) (r : alloc.vec.Vec (alloc.vec.Vec U8)) : Prop :=
  ∃ k, (k = 1 ∨ k = 2) ∧ k ≤ s.words.val.length ∧ r.val = s.words.val.take k ∧
    (∀ w ∈ r.val, plainWord (bytes w.val)) ∧ ∀ h ∈ r.val.head?, ¬ hasSlash (bytes h.val)

theorem mem_take_one {α : Type} (l : List α) (h : 0 < l.length) (w : α) (hw : w ∈ l.take 1) :
    w = l[0] := by
  obtain ⟨x, xs, rfl⟩ := List.exists_cons_of_length_pos h
  simpa using hw

theorem mem_take_two {α : Type} (l : List α) (h : 2 ≤ l.length) (w : α) (hw : w ∈ l.take 2) :
    w = l[0] ∨ w = l[1] := by
  match l, h with
  | x :: y :: _, _ => simpa using hw

theorem head_take {α : Type} (l : List α) (k : Nat) (hk : 1 ≤ k) (h : 0 < l.length) :
    (l.take k).head? = some l[0] := by
  obtain ⟨x, xs, rfl⟩ := List.exists_cons_of_length_pos h
  obtain ⟨j, rfl⟩ : ∃ j, k = j + 1 := ⟨k - 1, by omega⟩
  simp

theorem slash_bv : (47#u8 : U8).bv = ch '/' := by decide

theorem no_slash_iff (l : List U8) : ¬ hasSlash (bytes l) ↔ ¬ (47#u8 : U8) ∈ l := by
  simp only [hasSlash, bytes, List.mem_map, not_exists, not_and]
  constructor
  · intro h hm; exact h _ hm slash_bv
  · intro h x hx he
    apply h
    have : x = 47#u8 := (UScalar.eq_equiv_bv_eq x 47#u8).mpr (he.trans slash_bv.symm)
    rw [← this]; exact hx

theorem not_ruled_out_ne (s : shell.Simple) (h : ¬ RuledOut s) : 0 < s.words.val.length := by
  apply List.length_pos_of_ne_nil
  intro he; exact h (Or.inl he)

theorem shape_of (s : shell.Simple) (r : alloc.vec.Vec (alloc.vec.Vec U8)) (k : Nat)
    (hk : k = 1 ∨ k = 2) (hlen : k ≤ s.words.val.length) (hr : r.val = s.words.val.take k)
    (hplain : ∀ w ∈ s.words.val.take k, plainWord (bytes w.val))
    (hslash : ¬ (47#u8 : U8) ∈ (s.words.val[0]'(by rcases hk with rfl | rfl <;> omega)).val) :
    Shape s r := by
  refine ⟨k, hk, hlen, hr, by rw [hr]; exact hplain, ?_⟩
  intro h hh
  rw [hr, head_take _ k (by omega) (by omega)] at hh
  simp only [Option.mem_def, Option.some.injEq] at hh
  rw [no_slash_iff, ← hh]
  exact hslash

@[step]
theorem propose_spec (s : shell.Simple) :
    always.propose s ⦃ o => (RuledOut s → o = none) ∧ ∀ r, o = some r → Shape s r ⦄ := by
  unfold always.propose
  step as ⟨b, b_post⟩
  split
  · simp
  have hout : ¬ RuledOut s := by rw [RuledOut, ← b_post]; assumption
  have hne := not_ruled_out_ne s hout
  step*
  all_goals simp only [alloc.vec.Vec.deref] at *
  all_goals first | scalar_tac | (simp; done) | skip
  · refine ⟨fun h => absurd h hout, ?_⟩
    rintro r ⟨⟩
    apply shape_of s v1 2 (Or.inr rfl) (by scalar_tac) v1_post
    case hplain =>
      intro w hw
      rcases mem_take_two _ (by scalar_tac) w hw with rfl | rfl
      · rw [← head_post]; exact b2_post.mp ‹_›
      · have h4 := b4_post.mp ‹_›
        rw [v_post] at h4
        exact h4
    case hslash => rw [← head_post, ← b1_post]; assumption
  · refine ⟨fun h => absurd h hout, ?_⟩
    rintro r ⟨⟩
    apply shape_of s v1 1 (Or.inl rfl) (by scalar_tac) v1_post
    case hplain =>
      intro w hw
      rw [mem_take_one _ hne w hw, ← head_post]
      exact b2_post.mp ‹_›
    case hslash => rw [← head_post, ← b1_post]; assumption

/-! ## The rules of one click -/

theorem shape_matches (s : shell.Simple) (r : alloc.vec.Vec (alloc.vec.Vec U8)) (h : Shape s r) :
    ruleMatches (strs r.val) (words s) := by
  obtain ⟨k, hk, hlen, hr, _, _⟩ := h
  refine ⟨?_, ?_⟩
  · rw [hr]
    intro h
    have := congrArg List.length h
    simp only [strs, List.length_map, List.length_take, List.length_nil] at this
    rcases hk with rfl | rfl <;> omega
  · rw [hr, words_eq, strs, strs, List.map_take]
    exact List.take_prefix _ _

@[step]
theorem is_covered_by_spec (words : Slice (alloc.vec.Vec U8)) (policy : action.Policy)
    (rules new : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    always.is_covered_by words policy rules new ⦃ _ => True ⦄ := by
  unfold always.is_covered_by
  step*

@[step]
theorem add_rule_spec (new : alloc.vec.Vec (alloc.vec.Vec (alloc.vec.Vec U8))) (simple : shell.Simple)
    (policy : action.Policy) (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8)))
    (hlen : new.val.length < Usize.max) :
    always.add_rule new simple policy rules ⦃ p => p.1.val.length ≤ new.val.length + 1 ∧
      ∀ r ∈ p.1.val, r ∈ new.val ∨ ruleMatches (strs r.val) (words simple) ⦄ := by
  unfold always.add_rule
  step*
  · refine ⟨by simp only [new1_post, List.length_append, List.length_singleton]; omega, ?_⟩
    intro r hr
    rw [new1_post] at hr
    simp only [List.mem_append, List.mem_singleton] at hr
    rcases hr with hr | rfl
    · exact Or.inl hr
    · exact Or.inr (shape_matches simple _ (o_post2 _ (by assumption)))

/-- Each rule covers a simple command of the list. -/
def Covers (simples : List shell.Simple) (new : List (alloc.vec.Vec (alloc.vec.Vec U8))) : Prop :=
  ∀ r ∈ new, ∃ s ∈ simples, ruleMatches (strs r.val) (words s)

@[step]
theorem new_rules_spec (simples : Slice shell.Simple) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    always.new_rules simples policy rules ⦃ o => ∀ rs, o = some rs → Covers simples.val rs.val ⦄ := by
  unfold always.new_rules
  have hloop : always.new_rules_loop simples policy rules (alloc.vec.Vec.new _) true 0#usize
      ⦃ p => Covers simples.val p.1.val ⦄ := by
    unfold always.new_rules_loop
    apply loop.spec_decr_nat (fun st => simples.val.length - st.2.2.val)
      (fun st => st.2.2.val ≤ simples.val.length ∧ st.1.val.length ≤ st.2.2.val ∧
        Covers simples.val st.1.val) _ _ _ _
      ⟨by simp, by simp, by simp [Covers]⟩
    rintro ⟨new, ok1, i⟩ ⟨hi, hlen, hcov⟩
    simp only at hi hlen hcov
    unfold always.new_rules_loop.body
    step*
    · refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
      intro r hr
      rcases next_post2 r hr with hr | hr
      · exact hcov r hr
      · exact ⟨simples.val[i.val]'(by scalar_tac), List.getElem_mem _, by rw [← s_post]; exact hr⟩
  step with hloop as ⟨new, ok1, hp⟩
  split
  · intro rs hrs
    simp only [Option.some.injEq] at hrs
    rw [← hrs]; exact hp
  · simp

theorem split_ok (raw : Slice U8) : ∃ o, shell.split raw = ok o :=
  Protocol.Action.exists_ok _ (Protocol.Shell.split_spec raw)

@[step]
theorem proposal_spec (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    always.proposal call policy rules ⦃ o => ∀ rs, o = some rs →
      ∀ r ∈ rs.val, ∃ s, inCall call s ∧ ruleMatches (strs r.val) (words s) ⦄ := by
  unfold always.proposal
  induction call with
  | Files _ _ => simp
  | Unknown => simp
  | Command raw cwd =>
    simp only
    obtain ⟨o, ho⟩ := split_ok (alloc.vec.Vec.deref raw)
    rw [ho]
    simp only [bind_tc_ok]
    cases o with
    | none => simp
    | some script =>
      simp only
      apply WP.spec_mono (new_rules_spec _ policy rules)
      intro o2 h rs hrs r hr
      obtain ⟨s, hs, hm⟩ := h rs hrs r hr
      exact ⟨s, ⟨raw, cwd, script, rfl, ho, hs⟩, hm⟩

/-! ## The check with the classifier -/

attribute [local step] Protocol.Action.classify_spec

theorem clone_vec_ok {T : Type} (c : core.clone.Clone T) (hc : ∀ x, c.clone x = ok x)
    (v : alloc.vec.Vec T) : (core.clone.CloneallocvecVec c).clone v = ok v := by
  have h := Slice.clone_spec (clone := c.clone) (s := v) (fun x _ => hc x)
  obtain ⟨y, hy, he⟩ := WP.spec_imp_exists h
  simp only [alloc.vec.CloneVec.clone]
  rw [hy, ← he]
  rfl

theorem clone_words_ok (x : alloc.vec.Vec U8) :
    (core.clone.CloneallocvecVec core.clone.CloneU8).clone x = ok x :=
  clone_vec_ok _ (fun _ => rfl) x

theorem clone_rule_ok (x : alloc.vec.Vec (alloc.vec.Vec U8)) :
    (core.clone.CloneallocvecVec (core.clone.CloneallocvecVec core.clone.CloneU8)).clone x = ok x :=
  clone_vec_ok _ clone_words_ok x

@[step]
theorem to_vec_rules_spec (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    alloc.slice.Slice.to_vec (core.clone.CloneallocvecVec (core.clone.CloneallocvecVec core.clone.CloneU8))
      rules ⦃ v => v.val = rules.val ⦄ := by
  apply WP.spec_mono (alloc.slice.Slice.to_vec_spec _ rules (fun x _ => clone_rule_ok x))
  intro v hv; rw [← hv]

@[step]
theorem extend_rules_spec (v : alloc.vec.Vec (alloc.vec.Vec (alloc.vec.Vec U8)))
    (s : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (h : v.val.length + s.val.length ≤ Usize.max) :
    alloc.vec.Vec.extend_from_slice (core.clone.CloneallocvecVec (core.clone.CloneallocvecVec core.clone.CloneU8))
      v s ⦃ r => r.val = v.val ++ s.val ⦄ := by
  have hs := Slice.clone_spec (s := s) (fun x _ => clone_rule_ok x)
  obtain ⟨y, hy, he⟩ := WP.spec_imp_exists hs
  unfold alloc.vec.Vec.extend_from_slice
  rw [dif_pos (by scalar_tac)]
  split
  · rename_i s' h'
    rw [hy] at h'
    simp only [ok.injEq] at h'
    simp only [WP.spec_ok]
    rw [← h', ← he]
  · rename_i e h'; rw [hy] at h'; simp at h'
  · rename_i h'; rw [hy] at h'; simp at h'

@[step]
theorem verdict_eq_spec (a b : action.Verdict) :
    action.Verdict.Insts.CoreCmpPartialEqVerdict.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [action.Verdict.Insts.CoreCmpPartialEqVerdict.eq, action.Verdict.read_discriminant]

@[step]
theorem allows_with_spec (call : action.ToolCall) (policy : action.Policy)
    (rules new : Slice (alloc.vec.Vec (alloc.vec.Vec U8)))
    (hlen : rules.val.length + new.val.length ≤ Usize.max) :
    always.allows_with call policy rules new ⦃ b => b = true →
      Protocol.Action.verdictModel call policy (rules.val ++ new.val) .Listed = .Allow ⦄ := by
  unfold always.allows_with
  step*
  rw [← b_post1.mp b_post2, v_post]
  simp only [alloc.vec.Vec.deref]
  rw [all1_post, all_post]

@[simp, scalar_tac_simps]
theorem max_rules_val : always.MAX_RULES.val = 4096 := by unfold always.MAX_RULES; rfl
@[simp, scalar_tac_simps]
theorem max_offer_val : always.MAX_OFFER.val = 3 := by unfold always.MAX_OFFER; rfl

/-- What an offer gives: S38, and the length that S39 needs. -/
def OfferPost (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (rs : alloc.vec.Vec (alloc.vec.Vec (alloc.vec.Vec U8))) :
    Prop :=
  1 ≤ rs.val.length ∧ rs.val.length ≤ 3 ∧
    Protocol.Action.verdictModel call policy (rules.val ++ rs.val) .Listed = .Allow ∧
    (rules.val ++ rs.val).length ≤ Usize.max ∧
    ∀ r ∈ rs.val, ∃ s, inCall call s ∧ ruleMatches (strs r.val) (words s)

@[step]
theorem offer_spec (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    always.offer call policy rules ⦃ o => ∀ rs, o = some rs → OfferPost call policy rules rs ⦄ := by
  unfold always.offer
  step*
  case hlen => simp only [alloc.vec.Vec.deref]; scalar_tac
  rintro rs ⟨⟩
  simp only [alloc.vec.Vec.deref] at *
  refine ⟨by scalar_tac, by scalar_tac, b_post ‹_›, by simp; scalar_tac, o_post _ ‹_›⟩

/-! ## The theorems -/

/-- **S36, proposal shape.** -/
theorem propose_shape (s : shell.Simple) :
    always.propose s ⦃ o => ∀ r, o = some r → ∃ k, (k = 1 ∨ k = 2) ∧ r.val = s.words.val.take k ∧
      (∀ w ∈ r.val, plainWord (bytes w.val)) ∧ ∀ h ∈ r.val.head?, ¬ hasSlash (bytes h.val) ⦄ := by
  apply WP.spec_mono (propose_spec s)
  rintro o ⟨_, h⟩ r hr
  obtain ⟨k, hk, _, hr', hp, hs⟩ := h r hr
  exact ⟨k, hk, hr', hp, hs⟩

/-- **S37, no proposal for the capped.** -/
theorem propose_none (s : shell.Simple)
    (h : desktopSimple s ∨ neverAlways (words s) ∨ noRuleTool (words s)) :
    always.propose s ⦃ o => o = none ⦄ := by
  apply WP.spec_mono (propose_spec s)
  rintro o ⟨h1, _⟩
  exact h1 (Or.inr h)

theorem classify_of_val (call : action.ToolCall) (policy : action.Policy)
    (all : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (l : List (alloc.vec.Vec (alloc.vec.Vec U8)))
    (hall : all.val = l) (h : Protocol.Action.verdictModel call policy l .Listed = .Allow) :
    action.classify call policy all = .ok .Allow := by
  obtain ⟨v, hv, hv'⟩ := WP.spec_imp_exists (Protocol.Action.classify_spec call policy all)
  rw [hv, hv', hall, h]

/-- **S38, an offer allows exactly its call.** -/
theorem offer_allows (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    always.offer call policy rules ⦃ o => ∀ rs, o = some rs →
      1 ≤ rs.val.length ∧ rs.val.length ≤ 3 ∧
      (∀ all : Slice (alloc.vec.Vec (alloc.vec.Vec U8)), all.val = rules.val ++ rs.val →
        action.classify call policy all = .ok .Allow) ∧
      ∀ r ∈ rs.val, ∃ s, inCall call s ∧ ruleMatches (strs r.val) (words s) ⦄ := by
  apply WP.spec_mono (offer_spec call policy rules)
  intro o h rs hrs
  obtain ⟨h1, h2, h3, _, h5⟩ := h rs hrs
  exact ⟨h1, h2, fun all hall => classify_of_val call policy all _ hall h3, h5⟩

/-- **S39, an offer stays under the ceiling.** -/
theorem offer_within_ceiling (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (rs : alloc.vec.Vec (alloc.vec.Vec (alloc.vec.Vec U8)))
    (h : always.offer call policy rules = .ok (some rs)) :
    action.ceiling call policy = .ok .Allow := by
  have hs := offer_spec call policy rules
  rw [h, WP.spec_ok] at hs
  obtain ⟨_, _, h3, h4, _⟩ := hs rs rfl
  have hv := classify_of_val call policy ⟨rules.val ++ rs.val, h4⟩ _ rfl h3
  obtain ⟨c, hc⟩ := Protocol.Action.exists_ok _ (Protocol.Action.ceiling_total call policy)
  have hle := Protocol.Action.classify_ceiling call policy _ .Allow c hv hc
  rw [hc]
  cases c <;> simp_all [rankV]

end Protocol.Always
