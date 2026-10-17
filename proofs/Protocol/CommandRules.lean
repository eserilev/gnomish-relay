import Protocol.PathRules

/-! # The command rules of the action classifier (S17, S28) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.PathRules

namespace Protocol.CommandRules

/-! ## The lists -/

theorem desktop_names_bytes :
    bytes (Array.to_slice command_rules.DESKTOP_NAMES).val = ascii desktopNames := by
  unfold command_rules.DESKTOP_NAMES; rfl
theorem shells_bytes : bytes (Array.to_slice command_rules.SHELLS).val = ascii shellNames := by
  unfold command_rules.SHELLS; rfl
theorem network_bytes : bytes (Array.to_slice command_rules.NETWORK).val = ascii networkNames := by
  unfold command_rules.NETWORK; rfl
set_option maxRecDepth 10000 in
theorem runners_bytes : bytes (Array.to_slice command_rules.RUNNERS).val = ascii runnerNames := by
  unfold command_rules.RUNNERS; rfl
theorem versioned_runners_bytes :
    bytes (Array.to_slice command_rules.VERSIONED_RUNNERS).val = ascii versionedRunnerNames := by
  unfold command_rules.VERSIONED_RUNNERS; rfl
theorem head_runners_bytes :
    bytes (Array.to_slice command_rules.HEAD_RUNNERS).val = ascii headRunnerNames := by
  unfold command_rules.HEAD_RUNNERS; rfl
theorem never_always_bytes :
    bytes (Array.to_slice command_rules.NEVER_ALWAYS).val = ascii neverAlwaysNames := by
  unfold command_rules.NEVER_ALWAYS; rfl
set_option maxRecDepth 10000 in
theorem git_runs_bytes : bytes (Array.to_slice command_rules.GIT_RUNS).val = ascii gitRunFlags := by
  unfold command_rules.GIT_RUNS; rfl
theorem git_force_bytes : bytes (Array.to_slice command_rules.GIT_FORCE).val = ascii gitForceFlags := by
  unfold command_rules.GIT_FORCE; rfl
theorem find_runs_bytes : bytes (Array.to_slice command_rules.FIND_RUNS).val = ascii findRunFlags := by
  unfold command_rules.FIND_RUNS; rfl
theorem dash_dash_bytes : bytes (Array.to_slice command_rules.DASH_DASH).val = ascii "--" := by
  unfold command_rules.DASH_DASH; rfl
theorem dash_o_bytes : bytes (Array.to_slice command_rules.DASH_O).val = ascii "-O" := by
  unfold command_rules.DASH_O; rfl
theorem rm_bytes : bytes (Array.to_slice command_rules.RM).val = ascii "rm" := by
  unfold command_rules.RM; rfl
theorem git_bytes : bytes (Array.to_slice command_rules.GIT).val = ascii "git" := by
  unfold command_rules.GIT; rfl
theorem find_bytes : bytes (Array.to_slice command_rules.FIND).val = ascii "find" := by
  unfold command_rules.FIND; rfl
theorem dot_exe_bytes : bytes (Array.to_slice command_rules.DOT_EXE).val = ascii ".exe" := by
  unfold command_rules.DOT_EXE; rfl

@[simp, scalar_tac_simps]
theorem dot_exe_len : (Array.to_slice command_rules.DOT_EXE).val.length = 4 := by
  unfold command_rules.DOT_EXE; rfl

theorem byte_bv_iff (b : U8) (c : Char) (n : Nat) (hc : c.toNat = n) (hn : n < 256) :
    b.bv = ch c ↔ b.val = n := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simp [ch, hc] at this; omega
  · intro h; apply BitVec.eq_of_toNat_eq; simp [ch, h, hc]; omega

/-! ## Names and flags -/

theorem bytes_take_succ (l : List U8) (i : Nat) (h : i < l.length) :
    bytes (l.take (i + 1)) = bytes (l.take i) ++ [l[i].bv] := by
  rw [List.take_add_one, List.getElem?_eq_getElem h]
  simp only [bytes, List.map_append, Option.toList_some, List.map_cons, List.map_nil]

@[step]
theorem name_step_spec (acc : alloc.vec.Vec U8) (b : U8) (h : acc.val.length < Usize.max) :
    command_rules.name_step acc b ⦃ r =>
      bytes r.val = nameStep (bytes acc.val) b.bv ∧ r.val.length ≤ acc.val.length + 1 ⦄ := by
  unfold command_rules.name_step
  have h47 := byte_bv_iff b '/' 47 rfl (by omega)
  have h92 := byte_bv_iff b '\\' 92 rfl (by omega)
  step*
  · refine ⟨?_, by simp⟩
    rw [nameStep, if_pos (Or.inl (by decide))]; rfl
  · refine ⟨?_, by simp⟩
    rw [nameStep, if_pos (Or.inr (by decide))]; rfl
  · refine ⟨?_, by simp [r_post]⟩
    have n47 : ¬ b.val = 47 := fun h => (by assumption : ¬b = 47#u8) (UScalar.eq_of_val_eq (by simp [h]))
    have n92 : ¬ b.val = 92 := fun h => (by assumption : ¬b = 92#u8) (UScalar.eq_of_val_eq (by simp [h]))
    rw [nameStep, if_neg (by rw [h47, h92]; omega), r_post]
    simp [bytes, i_post]

theorem name_of_loop_spec (word : Slice U8) :
    command_rules.name_of_loop word (alloc.vec.Vec.new U8) 0#usize ⦃ r =>
      bytes r.val = (bytes word.val).foldl nameStep [] ∧ r.val.length ≤ word.val.length ⦄ := by
  unfold command_rules.name_of_loop
  apply loop.spec_decr_nat (fun st => word.val.length - st.2.val)
    (fun st => st.2.val ≤ word.val.length ∧ st.1.val.length ≤ st.2.val ∧
      bytes st.1.val = (bytes (word.val.take st.2.val)).foldl nameStep []) _ _ _ _
    ⟨by simp, by simp, by simp [bytes]⟩
  rintro ⟨acc, i⟩ ⟨hi, hlen, hacc⟩
  simp only at hi hlen hacc
  unfold command_rules.name_of_loop.body
  step*
  · have hlt : i.val < word.val.length := by scalar_tac
    refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
    rw [name1_post1, hacc, i3_post, i2_post, bytes_take_succ _ _ hlt, List.foldl_append]
    rfl
  · have : i.val = word.val.length := by scalar_tac
    rw [this, List.take_length] at hacc
    exact ⟨hacc, by omega⟩

@[step]
theorem without_exe_spec (n : alloc.vec.Vec U8) :
    command_rules.without_exe n ⦃ r => bytes r.val = withoutExe (bytes n.val) ∧ r.val.length ≤ n.val.length ⦄ := by
  unfold command_rules.without_exe
  have hsuf : ascii ".exe" <:+ bytes n.val ↔
      4 ≤ n.val.length ∧ (n.val.drop (n.val.length - 4)) = (Array.to_slice command_rules.DOT_EXE).val := by
    rw [List.suffix_iff_eq_drop, ← dot_exe_bytes]
    constructor
    · intro h
      have hl := congrArg List.length h
      simp [bytes] at hl
      refine ⟨by omega, ?_⟩
      rw [← Protocol.Seen.bytes_eq_iff]
      rw [h]; simp [bytes]
    · rintro ⟨h4, h⟩
      rw [← h]; simp [bytes]; omega
  have hn : n.len.val = n.val.length := by simp
  have hd : (Array.to_slice command_rules.DOT_EXE).len.val = 4 := by simp
  step*
  · refine ⟨?_, le_refl _⟩
    subst s_post
    rw [withoutExe, if_neg]
    rw [hsuf]
    rintro ⟨h4, _⟩
    have : n.len.val < 4 := by
      have := (by assumption : n.len < (Array.to_slice command_rules.DOT_EXE).len)
      simpa using this
    simp at this; omega
  · subst s2_post s4_post; simp; scalar_tac
  · subst s6_post; simp only [deref_val]; omega
  · subst s_post s2_post s3_post s4_post s6_post
    simp only [deref_val] at b_post r_post
    have h4 : 4 ≤ n.val.length := by
      have := (by assumption : ¬ n.len < (Array.to_slice command_rules.DOT_EXE).len)
      rw [UScalar.lt_equiv] at this; omega
    have e2 : i2.val = n.val.length - 4 := by omega
    have e5 : i5.val = n.val.length - 4 := by omega
    have hb : n.val.drop (n.val.length - 4) = (Array.to_slice command_rules.DOT_EXE).val := by
      have := b_post.mp (by assumption)
      simp only [hd] at this
      rw [e2, List.take_of_length_le (by simp; omega), List.take_of_length_le (by simp)] at this
      exact this
    refine ⟨?_, by simp [r_post]⟩
    rw [withoutExe, if_pos (hsuf.mpr ⟨h4, hb⟩), r_post, e5]
    simp [bytes]
  · subst s_post s2_post s3_post s4_post
    simp only [deref_val] at b_post
    have e2 : i2.val = n.val.length - 4 := by omega
    refine ⟨?_, le_refl _⟩
    rw [withoutExe, if_neg]
    rw [hsuf]
    rintro ⟨h4, h⟩
    apply (by assumption : ¬ b = true)
    rw [b_post]
    simp only [hd]
    rw [e2, List.take_of_length_le (by simp; omega), List.take_of_length_le (by simp)]
    exact h

@[step]
theorem name_of_spec (word : Slice U8) :
    command_rules.name_of word ⦃ r => bytes r.val = progName (bytes word.val) ∧ r.val.length ≤ word.val.length ⦄ := by
  unfold command_rules.name_of
  step with name_of_loop_spec word as ⟨acc, hacc, hlen⟩
  step*
  exact ⟨by rw [r_post1, hacc]; rfl, by omega⟩

/-! ## Versions -/

@[step]
theorem is_version_byte_spec (b : U8) :
    command_rules.is_version_byte b ⦃ r => r = versionByte b.bv ⦄ := by
  unfold command_rules.is_version_byte versionByte
  step*

theorem dropWhile_all_nil {α : Type} (p : α → Bool) (l : List α) (h : ∀ x ∈ l, p x = true) :
    l.dropWhile p = [] := by
  induction l with
  | nil => rfl
  | cons x xs ih =>
    rw [List.dropWhile_cons_of_pos (h x List.mem_cons_self)]
    exact ih (fun y hy => h y (List.mem_cons_of_mem _ hy))

/-- The stem ends where the last run of version bytes starts. -/
theorem stem_of_split (l : List Spec.Byte) (k : Nat) (hk : k ≤ l.length)
    (hall : ∀ x ∈ l.drop k, versionByte x = true)
    (hstop : ∀ h : 0 < k, versionByte (l[k - 1]'(by omega)) = false) :
    versionStem l = l.take k := by
  unfold versionStem
  conv => lhs; rw [← List.take_append_drop k l]
  rw [List.reverse_append, List.dropWhile_append,
    dropWhile_all_nil _ _ (fun x hx => hall x (List.mem_reverse.mp hx))]
  simp only [List.isEmpty_nil, if_true]
  rcases Nat.eq_zero_or_pos k with rfl | hpos
  · simp
  · have hrev : (l.take k).reverse = l[k - 1] :: (l.take (k - 1)).reverse := by
      conv => lhs; rw [show k = (k - 1) + 1 by omega, List.take_add_one,
        List.getElem?_eq_getElem (by omega)]
      simp
    rw [hrev, List.dropWhile_cons_of_neg (by simp [hstop hpos]), ← hrev, List.reverse_reverse]

theorem stem_len_loop_spec (nm : Slice U8) (k : Usize) (hk : k.val ≤ nm.val.length)
    (hall : ∀ x ∈ nm.val.drop k.val, versionByte x.bv = true) :
    command_rules.stem_len_loop nm k ⦃ r => r.val ≤ nm.val.length ∧
      bytes (nm.val.take r.val) = versionStem (bytes nm.val) ⦄ := by
  unfold command_rules.stem_len_loop
  apply loop.spec_decr_nat (fun k => k.val)
    (fun k => k.val ≤ nm.val.length ∧ ∀ x ∈ nm.val.drop k.val, versionByte x.bv = true)
    _ _ _ _ ⟨hk, hall⟩
  rintro k ⟨hk, hall⟩
  unfold command_rules.stem_len_loop.body
  have hstem : (∀ h : 0 < k.val, versionByte (nm.val[k.val - 1]'(by omega)).bv = false) →
      bytes (nm.val.take k.val) = versionStem (bytes nm.val) := by
    intro hstop
    rw [stem_of_split (bytes nm.val) k.val (by simp [bytes]; omega)]
    · simp [bytes]
    · intro x hx
      simp only [bytes, ← List.map_drop, List.mem_map] at hx
      obtain ⟨y, hy, rfl⟩ := hx
      exact hall y hy
    · intro h
      simp only [bytes, List.getElem_map]
      exact hstop h
  step*
  · have hlt : i.val < nm.val.length := by scalar_tac
    have hik : i.val + 1 = k.val := by scalar_tac
    have hb : versionByte i1.bv = true := by rw [← b_post]; assumption
    refine ⟨by scalar_tac, fun x hx => ?_, by scalar_tac⟩
    rw [List.drop_eq_getElem_cons hlt, hik] at hx
    rcases List.mem_cons.mp hx with rfl | hx
    · rw [← i1_post]; exact hb
    · exact hall x hx
  · have hik : k.val - 1 = i.val := by scalar_tac
    have hb : versionByte i1.bv = false := by
      rw [← b_post]; simpa using (by assumption : ¬ b = true)
    refine ⟨hk, hstem (fun _ => ?_)⟩
    simp only [hik, ← i1_post]
    exact hb

@[step]
theorem stem_len_spec (nm : Slice U8) :
    command_rules.stem_len nm ⦃ r => r.val ≤ nm.val.length ∧
      bytes (nm.val.take r.val) = versionStem (bytes nm.val) ⦄ := by
  unfold command_rules.stem_len
  exact stem_len_loop_spec nm _ (by simp) (by simp)

/-- `is_versioned`, on bytes. -/
def VersionedB (names n : List Spec.Byte) : Prop :=
  (versionStem n).length < n.length ∧ (ch ' ' :: versionStem n ++ [ch ' ']) <:+: names

@[step]
theorem is_versioned_spec (nm names : Slice U8) (hroom : names.val.length < Usize.max) :
    command_rules.is_versioned nm names ⦃ r =>
      (r = true ↔ VersionedB (bytes names.val) (bytes nm.val)) ⦄ := by
  unfold command_rules.is_versioned
  step*
  · simp only [Bool.false_eq_true, false_iff, VersionedB, not_and]
    intro h
    rw [← k_post2] at h
    simp [bytes] at h
    scalar_tac
  · have hlen : (versionStem (bytes nm.val)).length = k.val := by
      rw [← k_post2]; simp [bytes]; omega
    rw [r_post, Protocol.Search.listed_bytes]
    simp only [deref_val, stem_post, VersionedB, ← k_post2, List.nil_append, List.drop_zero,
      Nat.sub_zero]
    have hlt : k.val < nm.val.length := by scalar_tac
    simp only [bytes, List.length_map, List.length_take] at hlen ⊢
    constructor
    · intro h; exact ⟨by omega, h⟩
    · intro h; exact h.2

theorem takeWhile_at {α : Type} (p : α → Bool) (w : List α) (i : Nat)
    (hall : ∀ x ∈ w.take i, p x = true) (hstop : ∀ h : i < w.length, p w[i] = false) :
    w.takeWhile p = w.take i := by
  conv => lhs; rw [← List.take_append_drop i w]
  rw [List.takeWhile_append_of_pos hall]
  by_cases hi : i < w.length
  · rw [List.drop_eq_getElem_cons hi, List.takeWhile_cons_of_neg (by simp [hstop hi])]
    simp
  · rw [List.drop_eq_nil_of_le (by omega)]
    simp

theorem eq_bv_iff (x : U8) : x.bv = ch '=' ↔ x.val = 61 := byte_bv_iff x '=' 61 rfl (by omega)

@[step]
theorem flag_of_spec (word : Slice U8) :
    command_rules.flag_of word ⦃ r => bytes r.val = flag (bytes word.val) ⦄ := by
  unfold command_rules.flag_of command_rules.flag_of_loop
  apply loop.spec_decr_nat (fun st => word.val.length - st.2.val)
    (fun st => st.2.val ≤ word.val.length ∧ st.1.val.length = st.2.val ∧
      st.1.val = word.val.take st.2.val ∧ ∀ x ∈ word.val.take st.2.val, x.val ≠ 61) _ _ _ _
    ⟨by simp, by simp, by simp, by simp⟩
  rintro ⟨acc, i⟩ ⟨hi, hlen, hacc, hne⟩
  simp only at hi hlen hacc hne
  unfold command_rules.flag_of_loop.body
  have hflag : ∀ k, (∀ x ∈ word.val.take k, x.val ≠ 61) → (∀ h : k < word.val.length, word.val[k].val = 61) →
      bytes (word.val.take k) = flag (bytes word.val) := by
    intro k hall hstop
    unfold flag
    rw [takeWhile_at _ _ k]
    · simp [bytes]
    · intro x hx
      simp only [bytes, ← List.map_take, List.mem_map] at hx
      obtain ⟨y, hy, rfl⟩ := hx
      simpa [eq_bv_iff] using hall y hy
    · intro h
      simp only [bytes, List.length_map] at h
      simp [bytes, eq_bv_iff, hstop h]
  step*
  · have hlt : i.val < word.val.length := by scalar_tac
    have h61 : ¬ i2.val = 61 := by
      have hb := (by assumption : (i2 != 61#u8) = true)
      simp only [bne_iff_ne, ne_eq] at hb
      intro h
      exact hb (UScalar.eq_of_val_eq (by simp [h]))
    refine ⟨by scalar_tac, by simp [flag1_post, hlen, i3_post], ?_, ?_, by scalar_tac⟩
    · rw [flag1_post, hacc, i3_post, List.take_add_one, List.getElem?_eq_getElem hlt, i2_post]; rfl
    · rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt]
      intro x hx
      simp only [List.mem_append, Option.toList_some, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hne x hx
      · rw [← i2_post]; exact h61
  · have hlt : i.val < word.val.length := by scalar_tac
    have h61 : i2.val = 61 := by
      have hb := (by assumption : ¬(i2 != 61#u8) = true)
      simp only [bne_iff_ne, ne_eq, not_not] at hb
      simp [hb]
    rw [hacc]
    exact hflag i.val hne (fun _ => by rw [← i2_post]; exact h61)
  · have : i.val = word.val.length := by scalar_tac
    rw [hacc]
    exact hflag i.val hne (fun h => absurd h (by omega))

theorem head_bv (w : List U8) (b : U8) : (bytes w).head? = some b.bv ↔ w.head? = some b := by
  cases w with
  | nil => simp [bytes]
  | cons x xs =>
    simp only [bytes, List.map_cons, List.head?_cons, Option.some.injEq]
    exact (UScalar.eq_equiv_bv_eq x b).symm

@[step]
theorem starts_with_spec (word : Slice U8) (b : U8) :
    command_rules.starts_with word b ⦃ r => (r = true ↔ word.val.head? = some b) ⦄ := by
  unfold command_rules.starts_with
  step*
  · have hlt : 0 < word.val.length := by scalar_tac
    obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos hlt
    simp only [hx, List.getElem_cons_zero] at i1_post
    rw [hx, i1_post]
    simp
  · have : word.val = [] := List.eq_nil_of_length_eq_zero (by scalar_tac)
    simp [this]

theorem mem_bytes (w : List U8) (x : U8) : x.bv ∈ bytes w ↔ x ∈ w := by
  simp only [bytes, List.mem_map]
  constructor
  · rintro ⟨y, hy, he⟩
    rw [← (UScalar.eq_equiv_bv_eq y x).mpr he]; exact hy
  · intro h; exact ⟨x, h, rfl⟩

theorem dash_ch : ch '-' = (45#u8 : U8).bv := by decide
theorem plus_ch : ch '+' = (43#u8 : U8).bv := by decide
theorem r_ch : ch 'r' = (114#u8 : U8).bv := by decide
theorem big_r_ch : ch 'R' = (82#u8 : U8).bv := by decide
theorem f_ch : ch 'f' = (102#u8 : U8).bv := by decide

@[step]
theorem is_short_flags_spec (word : Slice U8) :
    command_rules.is_short_flags word ⦃ r => (r = true ↔ shortFlags (bytes word.val)) ⦄ := by
  unfold command_rules.is_short_flags
  have hdrop : ((bytes word.val).drop 1).head? = some (ch '-') ↔ (word.val.drop 1).head? = some 45#u8 := by
    rw [dash_ch, ← head_bv]; simp [bytes]
  have hhead : (bytes word.val).head? = some (ch '-') ↔ word.val.head? = some 45#u8 := by
    rw [dash_ch, head_bv]
  step*
  · split
    · step*
      have h1 : (word.val.drop 1).head? = some x := by
        rw [List.head?_drop, List.getElem?_eq_getElem (by scalar_tac), x_post]
      simp only [shortFlags, hhead, ne_eq, hdrop, h1, ← b_post, Option.some.injEq]
      simp [*]
    · step*
      have h1 : (word.val.drop 1).head? = none := by
        rw [List.head?_drop, List.getElem?_eq_none (by scalar_tac)]
      simp only [shortFlags, hhead, ne_eq, hdrop, h1, ← b_post]
      simp [*]
  · rw [shortFlags, hhead, ← b_post]
    simp [*]

@[step]
theorem is_recursive_spec (word : Slice U8) :
    command_rules.is_recursive word ⦃ r => (r = true ↔ recursiveFlag (bytes word.val)) ⦄ := by
  unfold command_rules.is_recursive
  have hhead : (bytes word.val).head? = some (ch '-') ↔ word.val.head? = some 45#u8 := by
    rw [dash_ch, head_bv]
  step*
  · simp only [true_iff]
    exact ⟨hhead.mpr (b_post.mp (by assumption)),
      Or.inl (by rw [r_ch, mem_bytes]; exact b1_post.mp (by assumption))⟩
  · rw [r_post, recursiveFlag, hhead, r_ch, big_r_ch, mem_bytes, mem_bytes, ← b_post, ← b1_post]
    simp [*]
  · rw [recursiveFlag, hhead, ← b_post]
    simp [*]

@[simp, scalar_tac_simps]
theorem git_force_len : (Array.to_slice command_rules.GIT_FORCE).val.length = 64 := by
  unfold command_rules.GIT_FORCE; rfl

/-- `listed`, on the bytes that a Rust list holds. -/
theorem listed_iff (names n : List U8) (s : String) (hs : bytes names = ascii s) :
    (search.SPACE :: n ++ [search.SPACE]) <:+: names ↔ listed s (bytes n) := by
  rw [Protocol.Search.listed_bytes, hs]; rfl

@[step]
theorem is_git_force_spec (word : Slice U8) :
    command_rules.is_git_force word ⦃ r => (r = true ↔ gitForce (bytes word.val)) ⦄ := by
  unfold command_rules.is_git_force
  have hplus : (bytes word.val).head? = some (ch '+') ↔ word.val.head? = some 43#u8 := by
    rw [plus_ch, head_bv]
  have hf : ch 'f' ∈ bytes word.val ↔ 102#u8 ∈ word.val := by rw [f_ch, mem_bytes]
  step*
  all_goals
    have hb : b = true ↔ listed gitForceFlags (flag (bytes word.val)) := by
      rw [b_post, s_post]; simp only [deref_val]; rw [listed_iff _ _ _ git_force_bytes, v_post]
  · simp only [true_iff]
    exact Or.inl (hb.mp (by assumption))
  · simp only [true_iff]
    exact Or.inr (Or.inl (hplus.mpr (b1_post.mp (by assumption))))
  · rw [r_post, gitForce, hplus, hf, ← hb, ← b1_post, ← b2_post]
    simp [*]
  · rw [gitForce, hplus, hf, ← hb, ← b1_post, ← b2_post]
    simp [*]

/-! ## Prefixes and long flags -/

theorem prefix_bytes_iff (a b : List U8) : bytes a <+: bytes b ↔ a <+: b := by
  rw [List.prefix_iff_eq_take, List.prefix_iff_eq_take]
  have hl : (bytes a).length = a.length := by simp [bytes]
  rw [hl, show (bytes b).take a.length = bytes (b.take a.length) by simp [bytes]]
  exact Protocol.Seen.bytes_eq_iff a _

@[step]
theorem has_prefix_spec (word pre : Slice U8) :
    command_rules.has_prefix word pre ⦃ r => (r = true ↔ pre.val <+: word.val) ⦄ := by
  unfold command_rules.has_prefix
  step*
  · have hl : pre.len.val = pre.val.length := by simp
    rw [r_post, List.prefix_iff_eq_take, List.drop_zero, hl, List.take_length]
    exact eq_comm
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have := h.length_le
    scalar_tac

theorem dash_dash_len : (Array.to_slice command_rules.DASH_DASH).val.length = 2 := by
  unfold command_rules.DASH_DASH; rfl

@[step]
theorem starts_long_flag_spec (f names : Slice U8) (hroom : names.val.length < Usize.max) :
    command_rules.starts_long_flag f names ⦃ r =>
      (r = true ↔ 2 < (bytes f.val).length ∧ ascii "--" <+: bytes f.val ∧
        (ch ' ' :: bytes f.val) <:+: bytes names.val) ⦄ := by
  unfold command_rules.starts_long_flag
  have hd := dash_dash_len
  have hp : (Array.to_slice command_rules.DASH_DASH).val <+: f.val ↔ ascii "--" <+: bytes f.val := by
    rw [← prefix_bytes_iff, dash_dash_bytes]
  have hl : (bytes f.val).length = f.val.length := by simp [bytes]
  step*
  subst_vars
  rw [r_post, Protocol.Search.starts_listed_bytes, hl]
  have := b_post.mp rfl
  simp only [hp] at this
  constructor
  · intro h; exact ⟨by scalar_tac, this, h⟩
  · intro h; exact h.2.2

/-- `is_option`, on bytes: the flag is listed, or starts a long flag of the list. -/
def OptionB (names f : List Spec.Byte) : Prop :=
  (ch ' ' :: f ++ [ch ' ']) <:+: names ∨
    (2 < f.length ∧ ascii "--" <+: f ∧ (ch ' ' :: f) <:+: names)

@[step]
theorem is_option_spec (word names : Slice U8) (hroom : names.val.length < Usize.max) :
    command_rules.is_option word names ⦃ r =>
      (r = true ↔ OptionB (bytes names.val) (flag (bytes word.val))) ⦄ := by
  unfold command_rules.is_option
  step*
  · simp only [true_iff, OptionB]
    left
    rw [← flag_post, ← Protocol.Search.listed_bytes]
    exact b_post.mp (by assumption)
  · have hb : ¬ (ch ' ' :: Spec.flag (bytes word.val) ++ [ch ' ']) <:+: bytes names.val := by
      rw [← flag_post, ← Protocol.Search.listed_bytes]
      intro h; exact (by assumption : ¬ b = true) (b_post.mpr h)
    simp only [deref_val] at r_post
    rw [r_post, OptionB, flag_post]
    constructor
    · exact Or.inr
    · rintro (h | h)
      · exact absurd h hb
      · exact h

/-- What `word_passes` checks, on bytes. -/
def WordTest (test : command_rules.Test) (names w : List Spec.Byte) : Prop :=
  match test with
  | .Name => (ch ' ' :: progName w ++ [ch ' ']) <:+: names
  | .Versioned => VersionedB names (progName w)
  | .Option => OptionB names (flag w)
  | .Prefix => names <+: w
  | .Flag => (ch ' ' :: flag w ++ [ch ' ']) <:+: names
  | .Recursive => recursiveFlag w
  | .GitForce => gitForce w

@[step]
theorem word_passes_spec (word : Slice U8) (test : command_rules.Test) (names : Slice U8)
    (hroom : names.val.length < Usize.max) :
    command_rules.word_passes word test names ⦃ r =>
      (r = true ↔ WordTest test (bytes names.val) (bytes word.val)) ⦄ := by
  unfold command_rules.word_passes
  induction test <;> step*
  · rw [r_post, Protocol.Search.listed_bytes]; simp only [deref_val]; rw [v_post1]; rfl
  · rw [r_post]; simp only [deref_val]; rw [v_post1]; rfl
  · rw [r_post, Protocol.Search.listed_bytes]; simp only [deref_val]; rw [v_post]; rfl
  · exact r_post
  · rw [r_post, ← prefix_bytes_iff names.val word.val]; rfl
  · exact r_post
  · exact r_post

def AnyWord (test : command_rules.Test) (names : List Spec.Byte) (words : List (alloc.vec.Vec U8)) : Prop :=
  ∃ w ∈ strs words, WordTest test names w

@[step]
theorem any_word_spec (words : Slice (alloc.vec.Vec U8)) (test : command_rules.Test) (names : Slice U8)
    (hroom : names.val.length < Usize.max) :
    command_rules.any_word words test names ⦃ r => (r = true ↔ AnyWord test (bytes names.val) words.val) ⦄ := by
  unfold command_rules.any_word command_rules.any_word_loop
  apply loop.spec_decr_nat (fun st => words.val.length - st.2.val)
    (fun st => st.2.val ≤ words.val.length ∧
      (st.1 = true ↔ AnyWord test (bytes names.val) (words.val.take st.2.val))) _ _ _ _
    ⟨by simp, by simp [AnyWord, strs]⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold command_rules.any_word_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨w, hw, h⟩ := hfound.mp (by assumption)
    exact ⟨w, List.map_subset _ (List.take_subset _ _) hw, h⟩
  · have hlt : i.val < words.val.length := by scalar_tac
    have hnot : ¬ AnyWord test (bytes names.val) (words.val.take i.val) := by rw [← hfound]; assumption
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [found1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, ← v_post]
    simp only [AnyWord, strs, List.map_append, List.mem_append, Option.toList_some, List.map_cons,
      List.map_nil, List.mem_singleton, deref_val] at hnot ⊢
    constructor
    · intro h
      exact ⟨_, Or.inr rfl, h⟩
    · rintro ⟨w, (hw | rfl), h⟩
      · exact absurd ⟨w, hw, h⟩ hnot
      · exact h
  · simp only [Bool.false_eq_true, false_iff]
    have : i.val = words.val.length := by scalar_tac
    rw [this, List.take_length] at hfound
    intro h
    exact (by assumption : ¬ found = true) (hfound.mpr h)

theorem eq_ch : ch '=' = (61#u8 : U8).bv := by decide

theorem strs_head (l : List (alloc.vec.Vec U8)) (h : 0 < l.length) :
    (strs l).head? = some (bytes l[0].val) := by
  obtain ⟨x, xs, rfl⟩ := List.exists_cons_of_length_pos h
  rfl

theorem strs_head_nil (l : List (alloc.vec.Vec U8)) (h : l.length = 0) : (strs l).head? = none := by
  rw [List.eq_nil_of_length_eq_zero h]; rfl

@[step]
theorem head_is_spec (words : Slice (alloc.vec.Vec U8)) (n : Slice U8) :
    command_rules.head_is words n ⦃ r =>
      (r = true ↔ ∃ h, (strs words.val).head? = some h ∧ progName h = bytes n.val) ⦄ := by
  unfold command_rules.head_is
  step*
  · rw [strs_head _ (by scalar_tac), r_post, ← Protocol.Seen.bytes_eq_iff]
    simp only [deref_val] at v1_post1 ⊢
    rw [v1_post1, v_post]
    simp
  · rw [strs_head_nil _ (by scalar_tac)]
    simp

@[step]
theorem head_listed_spec (words : Slice (alloc.vec.Vec U8)) (names : Slice U8)
    (hroom : names.val.length < Usize.max) :
    command_rules.head_listed words names ⦃ r =>
      (r = true ↔ ∃ h, (strs words.val).head? = some h ∧ (ch ' ' :: progName h ++ [ch ' ']) <:+: bytes names.val) ⦄ := by
  unfold command_rules.head_listed
  step*
  · rw [strs_head _ (by scalar_tac), r_post, Protocol.Search.listed_bytes]
    simp only [deref_val] at v1_post1 ⊢
    rw [v1_post1, v_post]
    simp
  · rw [strs_head_nil _ (by scalar_tac)]
    simp

@[step]
theorem head_assigns_spec (words : Slice (alloc.vec.Vec U8)) :
    command_rules.head_assigns words ⦃ r => (r = true ↔ headAssigns (strs words.val)) ⦄ := by
  unfold command_rules.head_assigns
  step*
  · rw [headAssigns, strs_head _ (by scalar_tac), r_post, eq_ch]
    simp only [deref_val, Option.some.injEq, exists_eq_left', mem_bytes, v_post]
  · rw [headAssigns, strs_head_nil _ (by scalar_tac)]
    simp

theorem anyWord_name (L : String) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Name (ascii L) words ↔ anyName L (strs words) := Iff.rfl

theorem anyWord_versioned (L : String) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Versioned (ascii L) words ↔ ∃ w ∈ strs words, versioned L (progName w) := Iff.rfl

theorem anyWord_option (L : String) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Option (ascii L) words ↔ ∃ w ∈ strs words, option L (flag w) := Iff.rfl

theorem anyWord_prefix (p : String) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Prefix (ascii p) words ↔ ∃ w ∈ strs words, ascii p <+: w := Iff.rfl

theorem anyWord_flag (L : String) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Flag (ascii L) words ↔ ∃ w ∈ strs words, listed L (flag w) := Iff.rfl

theorem headListed_iff (L : String) (words : List (alloc.vec.Vec U8)) :
    (∃ h, (strs words).head? = some h ∧ (ch ' ' :: progName h ++ [ch ' ']) <:+: ascii L) ↔
      headListed L (strs words) := Iff.rfl

@[step]
theorem is_git_runner_spec (words : Slice (alloc.vec.Vec U8)) :
    command_rules.is_git_runner words ⦃ r => (r = true ↔ gitRunner (strs words.val)) ⦄ := by
  unfold command_rules.is_git_runner
  step*
  all_goals subst_vars
  all_goals simp only [git_bytes, git_runs_bytes, dash_o_bytes, anyWord_option, anyWord_prefix] at *
  all_goals grind [gitRunner, headIs]

@[step]
theorem is_runner_spec (words : Slice (alloc.vec.Vec U8)) :
    command_rules.is_runner words ⦃ r => (r = true ↔ runner (strs words.val)) ⦄ := by
  unfold command_rules.is_runner
  step*
  all_goals subst_vars
  all_goals simp only [runners_bytes, versioned_runners_bytes, anyWord_versioned, head_runners_bytes, find_bytes, find_runs_bytes,
    anyWord_name, anyWord_flag, headListed_iff] at *
  all_goals grind [runner, headIs]

@[step]
theorem is_network_spec (words : Slice (alloc.vec.Vec U8)) :
    command_rules.is_network words ⦃ r => (r = true ↔ network (strs words.val)) ⦄ := by
  unfold command_rules.is_network
  step*
  subst_vars
  simp only [network_bytes, anyWord_name] at *
  exact r_post

theorem anyWord_recursive (names : List Spec.Byte) (words : List (alloc.vec.Vec U8)) :
    AnyWord .Recursive names words ↔ ∃ w ∈ strs words, recursiveFlag w := Iff.rfl

theorem anyWord_git_force (names : List Spec.Byte) (words : List (alloc.vec.Vec U8)) :
    AnyWord .GitForce names words ↔ ∃ w ∈ strs words, gitForce w := Iff.rfl

@[step]
theorem is_capped_spec (words : Slice (alloc.vec.Vec U8)) :
    command_rules.is_capped words ⦃ r => (r = true ↔ neverAlways (strs words.val)) ⦄ := by
  unfold command_rules.is_capped
  step*
  all_goals subst_vars
  all_goals simp only [never_always_bytes, rm_bytes, git_bytes, anyWord_name, anyWord_recursive,
    anyWord_git_force] at *
  all_goals simp_all [neverAlways, headIs]

@[step]
theorem link_eq_spec (a b : shell.Link) :
    shell.Link.Insts.CoreCmpPartialEqLink.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [shell.Link.Insts.CoreCmpPartialEqLink.eq, shell.Link.read_discriminant]

@[step]
theorem is_desktop_spec (simple : shell.Simple) :
    command_rules.is_desktop simple ⦃ r => (r = true ↔ desktopSimple simple) ⦄ := by
  unfold command_rules.is_desktop
  step*
  all_goals subst_vars
  all_goals simp only [desktop_names_bytes, shells_bytes, anyWord_name, deref_val] at *
  all_goals simp_all [desktopSimple, words]

/-! ## Rules -/

/-- A rule covers a command that starts with its words. An empty rule covers nothing. -/
def RuleMatch (rule words : List (List Spec.Byte)) : Prop := rule ≠ [] ∧ rule <+: words

@[step]
theorem rule_matches_spec (rule words : Slice (alloc.vec.Vec U8)) :
    command_rules.rule_matches rule words ⦃ r => (r = true ↔ RuleMatch (strs rule.val) (strs words.val)) ⦄ := by
  unfold command_rules.rule_matches
  have hpre : strs rule.val <+: strs words.val ↔ rule.val.length ≤ words.val.length ∧
      Protocol.Folder.agree rule.val words.val rule.val.length := by
    have := Protocol.Folder.prefix_pb rule.val words.val words.val.length (le_refl _)
    rw [List.take_length] at this
    exact this
  step*
  · simp only [Bool.false_eq_true, false_iff, RuleMatch, not_and]
    intro hne
    have : rule.val = [] := List.eq_nil_of_length_eq_zero (by scalar_tac)
    simp [strs, this] at hne
  · simp only [Bool.false_eq_true, false_iff, RuleMatch, not_and]
    intro _ h
    have := (hpre.mp h).1
    scalar_tac
  · have hne : strs rule.val ≠ [] := by
      have : 0 < rule.val.length := by scalar_tac
      simp only [strs, ne_eq, List.map_eq_nil_iff]
      exact List.ne_nil_of_length_pos this
    have hle : rule.val.length ≤ words.val.length := by scalar_tac
    unfold command_rules.rule_matches_loop
    apply loop.spec_decr_nat (fun st => rule.val.length - st.2.val)
      (fun st => st.2.val ≤ rule.val.length ∧
        (st.1 = true ↔ Protocol.Folder.agree rule.val words.val st.2.val)) _ _ _ _
      ⟨by simp, by simp [Protocol.Folder.agree]⟩
    rintro ⟨same, k⟩ ⟨hk, hsame⟩
    simp only at hk hsame
    unfold command_rules.rule_matches_loop.body
    step*
    · have hag := hsame.mp (by assumption)
      refine ⟨by scalar_tac, ?_, by scalar_tac⟩
      rw [same1_post, k1_post]
      simp only [deref_val, v_post, v1_post]
      constructor
      · intro h j h1 h2 hj
        by_cases hjk : j < k.val
        · exact hag j h1 h2 hjk
        · have : j = k.val := by omega
          subst this
          exact h
      · intro h
        exact h k.val (by scalar_tac) (by scalar_tac) (by omega)
    · have : k.val = rule.val.length := by scalar_tac
      rw [this] at hsame
      simp only [true_iff, RuleMatch]
      exact ⟨hne, hpre.mpr ⟨hle, hsame.mp (by assumption)⟩⟩
    · simp only [Bool.false_eq_true, false_iff, RuleMatch, not_and]
      intro _ h
      have hag := (hpre.mp h).2
      have hn : ¬ Protocol.Folder.agree rule.val words.val k.val := by rw [← hsame]; assumption
      exact hn (fun j h1 h2 _ => hag j h1 h2 h1)

def AnyRule (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) (words : List (alloc.vec.Vec U8)) : Prop :=
  ∃ rule ∈ rules, RuleMatch (strs rule.val) (strs words)

@[step]
theorem matches_any_spec (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (words : Slice (alloc.vec.Vec U8)) :
    command_rules.matches_any rules words ⦃ r => (r = true ↔ AnyRule rules.val words.val) ⦄ := by
  unfold command_rules.matches_any command_rules.matches_any_loop
  apply loop.spec_decr_nat (fun st => rules.val.length - st.2.val)
    (fun st => st.2.val ≤ rules.val.length ∧ (st.1 = true ↔ AnyRule (rules.val.take st.2.val) words.val)) _ _ _ _
    ⟨by simp, by simp [AnyRule]⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold command_rules.matches_any_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨rule, hr, h⟩ := hfound.mp (by assumption)
    exact ⟨rule, List.mem_of_mem_take hr, h⟩
  · have hlt : i.val < rules.val.length := by scalar_tac
    have hnot : ¬ AnyRule (rules.val.take i.val) words.val := by rw [← hfound]; assumption
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [found1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [AnyRule, List.mem_append, Option.toList_some, List.mem_singleton, deref_val] at hnot ⊢
    constructor
    · intro h
      exact ⟨_, Or.inr rfl, by rw [← v_post]; exact h⟩
    · rintro ⟨rule, (hr | rfl), h⟩
      · exact absurd ⟨rule, hr, h⟩ hnot
      · rw [v_post]; exact h
  · simp only [Bool.false_eq_true, false_iff]
    have : i.val = rules.val.length := by scalar_tac
    rw [this, List.take_length] at hfound
    intro h
    exact (by assumption : ¬ found = true) (hfound.mpr h)

@[step]
theorem cover_eq_spec (a b : action.Cover) :
    action.Cover.Insts.CoreCmpPartialEqCover.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [action.Cover.Insts.CoreCmpPartialEqCover.eq, action.Cover.read_discriminant]

/-- The config allow table, the ceiling, or a rule from the game covers the command. -/
def Covered (policy : action.Policy) (rules : List (alloc.vec.Vec (alloc.vec.Vec U8)))
    (cover : action.Cover) (words : List (alloc.vec.Vec U8)) : Prop :=
  AnyRule policy.allow.val words ∨ cover = .Every ∨ AnyRule rules words

@[step]
theorem is_covered_spec (words : Slice (alloc.vec.Vec U8)) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    command_rules.is_covered words policy rules cover ⦃ r =>
      (r = true ↔ Covered policy rules.val cover words.val) ⦄ := by
  unfold command_rules.is_covered
  step*
  all_goals try simp only [deref_val] at *
  all_goals simp_all [Covered]

open Classical in
/-- The answer for one simple command. -/
noncomputable def simpleVerdict (s : shell.Simple) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) : action.Verdict :=
  if desktopSimple s then .Desktop
  else if neverAlways (words s) then .Ask
  else if Covered policy rules cover s.words.val then .Allow
  else .Ask

@[step]
theorem simple_verdict_spec (s : shell.Simple) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    command_rules.simple_verdict s policy rules cover ⦃ v => v = simpleVerdict s policy rules.val cover ⦄ := by
  unfold command_rules.simple_verdict
  step*
  all_goals try simp only [deref_val] at *
  all_goals simp_all [simpleVerdict, words]

@[step]
theorem changes_folder_spec (simples : Slice shell.Simple) :
    command_rules.changes_folder simples ⦃ _ => True ⦄ := by
  unfold command_rules.changes_folder command_rules.changes_folder_loop
  apply loop.spec_decr_nat (fun st => simples.val.length - st.2.val)
    (fun st => st.2.val ≤ simples.val.length) _ _ _ _ (by simp)
  rintro ⟨found, i⟩ hi
  simp only at hi
  unfold command_rules.changes_folder_loop.body
  step*

end Protocol.CommandRules
