import Protocol.Shell

/-! # The action classifier (S16, S17, S27, S28)

Each Rust function equals a model in Lean. The theorems then reason on the models.
The answer of a call is the strictest answer of its parts.
-/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.PathRules
  Protocol.CommandRules

namespace Protocol.Action

/-! ## The strictest answer -/

def stricterV (a b : action.Verdict) : action.Verdict := if rankV b < rankV a then b else a

theorem rankV_stricterV (a b : action.Verdict) : rankV (stricterV a b) = min (rankV a) (rankV b) := by
  unfold stricterV; split <;> omega

theorem rankV_le_three (v : action.Verdict) : rankV v ≤ 3 := by cases v <;> simp [rankV]

/-- The strictest answer of the parts, from `allow`. -/
def foldV {α : Type} (l : List α) (f : α → action.Verdict) : action.Verdict :=
  l.foldl (fun acc x => stricterV acc (f x)) .Allow

theorem le_foldl_iff {α : Type} (l : List α) (f : α → action.Verdict) (init : action.Verdict) (k : Nat) :
    k ≤ rankV (l.foldl (fun acc x => stricterV acc (f x)) init) ↔
      k ≤ rankV init ∧ ∀ x ∈ l, k ≤ rankV (f x) := by
  induction l generalizing init with
  | nil => simp
  | cons y ys ih =>
    rw [List.foldl_cons, ih, rankV_stricterV, le_min_iff]
    simp only [List.mem_cons, forall_eq_or_imp]
    tauto

theorem le_foldV_iff {α : Type} (l : List α) (f : α → action.Verdict) (k : Nat) :
    k ≤ rankV (foldV l f) ↔ k ≤ 3 ∧ ∀ x ∈ l, k ≤ rankV (f x) := by
  rw [foldV, le_foldl_iff]; rfl

theorem foldV_snoc {α : Type} (l : List α) (f : α → action.Verdict) (x : α) :
    foldV (l ++ [x]) f = stricterV (foldV l f) (f x) := by
  simp [foldV, List.foldl_append]

theorem foldV_le {α : Type} (l : List α) (f : α → action.Verdict) (x : α) (hx : x ∈ l) :
    rankV (foldV l f) ≤ rankV (f x) :=
  ((le_foldV_iff l f _).mp (le_refl _)).2 x hx

theorem foldV_mono {α : Type} (l : List α) (f g : α → action.Verdict)
    (h : ∀ x ∈ l, rankV (f x) ≤ rankV (g x)) : rankV (foldV l f) ≤ rankV (foldV l g) := by
  have hf := (le_foldV_iff l f _).mp (le_refl _)
  exact (le_foldV_iff l g _).mpr ⟨hf.1, fun x hx => (hf.2 x hx).trans (h x hx)⟩

theorem stricterV_mono (a b b' : action.Verdict) (h : rankV b ≤ rankV b') :
    rankV (stricterV a b) ≤ rankV (stricterV a b') := by
  rw [rankV_stricterV, rankV_stricterV]; omega

theorem stricterV_mono_left (a a' b : action.Verdict) (h : rankV a ≤ rankV a') :
    rankV (stricterV a b) ≤ rankV (stricterV a' b) := by
  rw [rankV_stricterV, rankV_stricterV]; omega

@[step]
theorem rank_spec (v : action.Verdict) : action.rank v ⦃ r => r.val = rankV v ⦄ := by
  unfold action.rank; induction v <;> simp [rankV]

@[step]
theorem stricter_spec (a b : action.Verdict) : action.stricter a b ⦃ r => r = stricterV a b ⦄ := by
  unfold action.stricter
  step*
  all_goals unfold stricterV
  · rw [if_pos (by scalar_tac)]
  · rw [if_neg (by scalar_tac)]

/-- The value of a call that returns. The default is never used. -/
def okOr {α : Type} (r : Result α) (d : α) : α :=
  match r with
  | .ok x => x
  | _ => d

theorem exists_ok {α : Type} (f : Result α) (h : f ⦃ _ => True ⦄) : ∃ x, f = ok x := by
  cases f <;> simp_all

theorem spec_okOr {α : Type} (f : Result α) (d : α) (h : f ⦃ _ => True ⦄) : f ⦃ r => r = okOr f d ⦄ := by
  cases f <;> simp_all [okOr]

/-! ## Files -/

@[step]
theorem paths_verdict_spec (paths : Slice (alloc.vec.Vec U8)) (access : shell.Access) (policy : action.Policy) :
    action.paths_verdict paths access policy ⦃ v => v = foldV (strs paths.val) (pathVerdict policy access) ⦄ := by
  unfold action.paths_verdict action.paths_verdict_loop
  apply loop.spec_decr_nat (fun st => paths.val.length - st.2.val)
    (fun st => st.2.val ≤ paths.val.length ∧ st.1 = foldV (strs (paths.val.take st.2.val)) (pathVerdict policy access))
    _ _ _ _ ⟨by simp, by simp [foldV, strs]⟩
  rintro ⟨v, i⟩ ⟨hi, hv⟩
  simp only at hi hv
  unfold action.paths_verdict_loop.body
  step*
  · have hlt : i.val < paths.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [v3_post, hv, v2_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [strs, List.map_append, Option.toList_some, List.map_cons, List.map_nil, foldV_snoc,
      deref_val, v1_post]
  · have : i.val = paths.val.length := by scalar_tac
    rw [hv, this, List.take_length]

/-- The answer for a file call: the strictest answer of its read and write paths. -/
noncomputable def filesVerdict (policy : action.Policy) (reads writes : List (alloc.vec.Vec U8)) : action.Verdict :=
  stricterV (foldV (strs reads) (pathVerdict policy .Read)) (foldV (strs writes) (pathVerdict policy .Write))

@[step]
theorem files_verdict_spec (reads writes : Slice (alloc.vec.Vec U8)) (policy : action.Policy) :
    action.files_verdict reads writes policy ⦃ v => v = filesVerdict policy reads.val writes.val ⦄ := by
  unfold action.files_verdict
  step*
  rw [v_post]; subst_vars; rfl

/-! ## Commands -/

@[step]
theorem redirects_verdict_spec (redirects : Slice shell.Redirect) (cwd : Slice U8) (policy : action.Policy) :
    action.redirects_verdict redirects cwd policy ⦃ _ => True ⦄ := by
  unfold action.redirects_verdict action.redirects_verdict_loop
  apply loop.spec_decr_nat (fun st => redirects.val.length - st.2.val)
    (fun st => st.2.val ≤ redirects.val.length) _ _ _ _ (by simp)
  rintro ⟨v, i⟩ hi
  simp only at hi
  unfold action.redirects_verdict_loop.body
  step*

@[step]
theorem simples_verdict_spec (simples : Slice shell.Simple) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    action.simples_verdict simples policy rules cover ⦃ v =>
      v = foldV simples.val (fun s => simpleVerdict s policy rules.val cover) ⦄ := by
  unfold action.simples_verdict action.simples_verdict_loop
  apply loop.spec_decr_nat (fun st => simples.val.length - st.2.val)
    (fun st => st.2.val ≤ simples.val.length ∧
      st.1 = foldV (simples.val.take st.2.val) (fun s => simpleVerdict s policy rules.val cover)) _ _ _ _
    ⟨by simp, by simp [foldV]⟩
  rintro ⟨v, i⟩ ⟨hi, hv⟩
  simp only at hi hv
  unfold action.simples_verdict_loop.body
  step*
  · have hlt : i.val < simples.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [v2_post, hv, v1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [Option.toList_some, foldV_snoc, s_post]
  · have : i.val = simples.val.length := by scalar_tac
    rw [hv, this, List.take_length]

@[step]
theorem has_relative_target_spec (redirects : Slice shell.Redirect) :
    action.has_relative_target redirects ⦃ _ => True ⦄ := by
  unfold action.has_relative_target action.has_relative_target_loop
  apply loop.spec_decr_nat (fun st => redirects.val.length - st.2.val)
    (fun st => st.2.val ≤ redirects.val.length) _ _ _ _ (by simp)
  rintro ⟨found, i⟩ hi
  simp only at hi
  unfold action.has_relative_target_loop.body action.is_relative
  step*
  split <;> step*

/-- The answer for a command that parses. `redirects` is the answer of the redirect
targets, and `moved` says that a relative target follows a `cd`. Neither depends on
the rules. -/
def scriptVerdict (redirects : action.Verdict) (moved : Bool) (simples : action.Verdict) : action.Verdict :=
  if moved = true then stricterV (stricterV redirects simples) .Desktop
  else stricterV redirects simples

noncomputable def scriptModel (script : shell.Script) (cwd : Slice U8) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) : action.Verdict :=
  scriptVerdict
    (okOr (action.redirects_verdict (alloc.vec.Vec.deref script.redirects) cwd policy) .Desktop)
    (okOr (action.has_relative_target (alloc.vec.Vec.deref script.redirects)) true &&
      okOr (command_rules.changes_folder (alloc.vec.Vec.deref script.simples)) true)
    (foldV script.simples.val (fun s => simpleVerdict s policy rules cover))

@[step]
theorem script_verdict_spec (script : shell.Script) (cwd : Slice U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    action.script_verdict script cwd policy rules cover ⦃ v => v = scriptModel script cwd policy rules.val cover ⦄ := by
  obtain ⟨R, hR⟩ := exists_ok _ (redirects_verdict_spec (alloc.vec.Vec.deref script.redirects) cwd policy)
  obtain ⟨t, ht⟩ := exists_ok _ (has_relative_target_spec (alloc.vec.Vec.deref script.redirects))
  obtain ⟨c, hc⟩ := exists_ok _ (changes_folder_spec (alloc.vec.Vec.deref script.simples))
  unfold action.script_verdict scriptModel
  simp only [hR, ht, hc, okOr]
  step*
  all_goals unfold scriptVerdict
  all_goals subst_vars
  all_goals simp_all

@[step]
theorem has_substitution_spec (raw : Slice U8) :
    shell.has_substitution raw ⦃ r => (r = true ↔ substitution (bytes raw.val)) ⦄ :=
  Protocol.Shell.has_substitution_spec raw

open Classical in
/-- The answer for a command: `desktop` if it is too long, has a substitution, or does
not parse. Else the answer of its parts. -/
noncomputable def commandModel (raw cwd : Slice U8) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) : action.Verdict :=
  if 2 ^ 20 < raw.val.length ∨ substitution (bytes raw.val) then .Desktop
  else
    match okOr (shell.split raw) none with
    | none => .Desktop
    | some script => scriptModel script cwd policy rules cover

@[step]
theorem command_verdict_spec (raw cwd : Slice U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    action.command_verdict raw cwd policy rules cover ⦃ v => v = commandModel raw cwd policy rules.val cover ⦄ := by
  obtain ⟨o, ho⟩ := exists_ok _ (Protocol.Shell.split_spec raw)
  unfold action.command_verdict commandModel
  simp only [ho, okOr]
  step*

/-! ## Every call -/

noncomputable def verdictModel (call : action.ToolCall) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) : action.Verdict :=
  match call with
  | .Files reads writes => filesVerdict policy reads.val writes.val
  | .Command raw cwd => commandModel (alloc.vec.Vec.deref raw) (alloc.vec.Vec.deref cwd) policy rules cover
  | .Unknown => .Desktop

@[step]
theorem verdict_spec (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (cover : action.Cover) :
    action.verdict call policy rules cover ⦃ v => v = verdictModel call policy rules.val cover ⦄ := by
  unfold action.verdict
  induction call <;> step* <;> (try rw [v_post]) <;> rfl

theorem classify_spec (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    action.classify call policy rules ⦃ v => v = verdictModel call policy rules.val .Listed ⦄ := by
  unfold action.classify
  step*

theorem ceiling_spec (call : action.ToolCall) (policy : action.Policy) :
    action.ceiling call policy ⦃ v => v = verdictModel call policy [] .Every ⦄ := by
  unfold action.ceiling
  step*
  rw [v_post]; rfl

/-! ## The theorems -/

theorem rankV_eq_zero (v : action.Verdict) (h : rankV v = 0) : v = .Deny := by
  cases v <;> simp_all [rankV]

theorem pathVerdict_ok (policy : action.Policy) (access : shell.Access) (p : List Spec.Byte)
    (h : 2 ≤ rankV (pathVerdict policy access p)) : ¬ denied policy p ∧ PathOk policy access p := by
  unfold pathVerdict at h
  split at h
  · simp [rankV] at h
  · split at h
    · exact ⟨by assumption, by assumption⟩
    · simp [rankV] at h

/-- **S16, paths.** -/
theorem classify_paths (reads writes : alloc.vec.Vec (alloc.vec.Vec U8)) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    action.classify (.Files reads writes) policy rules ⦃ v => rankV .Ask ≤ rankV v →
      (∀ r ∈ strs reads.val, ∃ root ∈ strs policy.roots.val, within root r) ∧
      (∀ w ∈ strs writes.val, within (bytes policy.chat.val) w) ∧
      (∀ p ∈ strs reads.val ++ strs writes.val,
        ¬ denied policy p ∧ ¬ matchesPattern (strs policy.desktop_paths.val) p) ∧
      (∀ w ∈ strs writes.val, ¬ matchesPattern (strs policy.desktop_writes.val) w) ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl h
  simp only [verdictModel, filesVerdict, rankV_stricterV, le_min_iff] at h
  have hr := ((le_foldV_iff _ _ _).mp h.1).2
  have hw := ((le_foldV_iff _ _ _).mp h.2).2
  refine ⟨fun r hrm => ?_, fun w hwm => ?_, fun p hp => ?_, fun w hwm => ?_⟩
  · obtain ⟨⟨_, hclean⟩, _, root, hroot, hin⟩ := (pathVerdict_ok _ _ _ (hr r hrm)).2
    exact ⟨root, hroot, hclean, hin⟩
  · obtain ⟨⟨_, hclean⟩, _, hin, _⟩ := (pathVerdict_ok _ _ _ (hw w hwm)).2
    exact ⟨hclean, hin⟩
  · rcases List.mem_append.mp hp with hp | hp
    · obtain ⟨hd, _, hm, _⟩ := pathVerdict_ok _ _ _ (hr p hp)
      exact ⟨hd, hm⟩
    · obtain ⟨hd, _, hm, _⟩ := pathVerdict_ok _ _ _ (hw p hp)
      exact ⟨hd, hm⟩
  · exact (pathVerdict_ok _ _ _ (hw w hwm)).2.2.2.2

/-- **S16, deny.** -/
theorem classify_deny (reads writes : alloc.vec.Vec (alloc.vec.Vec U8)) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8)))
    (h : ∃ p ∈ strs reads.val ++ strs writes.val, denied policy p) :
    action.classify (.Files reads writes) policy rules ⦃ v => v = .Deny ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl
  obtain ⟨p, hp, hd⟩ := h
  have hpv : rankV (pathVerdict policy .Read p) = 0 ∧ rankV (pathVerdict policy .Write p) = 0 := by
    simp [pathVerdict, hd, rankV]
  apply rankV_eq_zero
  simp only [verdictModel, filesVerdict, rankV_stricterV]
  rcases List.mem_append.mp hp with hp | hp
  · have := foldV_le _ (pathVerdict policy .Read) p hp
    omega
  · have := foldV_le _ (pathVerdict policy .Write) p hp
    omega

theorem simpleVerdict_mono (s : shell.Simple) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) :
    rankV (simpleVerdict s policy rules .Listed) ≤ rankV (simpleVerdict s policy [] .Every) := by
  unfold simpleVerdict
  split
  · simp
  · split
    · simp
    · have hc : Covered policy [] .Every s.words.val := Or.inr (Or.inl rfl)
      rw [if_pos hc]
      split <;> simp [rankV]

theorem scriptVerdict_mono (r : action.Verdict) (m : Bool) (a b : action.Verdict)
    (h : rankV a ≤ rankV b) : rankV (scriptVerdict r m a) ≤ rankV (scriptVerdict r m b) := by
  unfold scriptVerdict
  split
  · exact stricterV_mono_left _ _ _ (stricterV_mono _ _ _ h)
  · exact stricterV_mono _ _ _ h

theorem scriptVerdict_le (r : action.Verdict) (m : Bool) (a : action.Verdict) :
    rankV (scriptVerdict r m a) ≤ rankV a := by
  unfold scriptVerdict
  split <;> simp only [rankV_stricterV] <;> omega

theorem verdictModel_mono (call : action.ToolCall) (policy : action.Policy)
    (rules : List (alloc.vec.Vec (alloc.vec.Vec U8))) :
    rankV (verdictModel call policy rules .Listed) ≤ rankV (verdictModel call policy [] .Every) := by
  cases call with
  | Files reads writes => exact le_refl _
  | Unknown => exact le_refl _
  | Command raw cwd =>
    simp only [verdictModel, commandModel]
    split
    · exact le_refl _
    · split
      · exact le_refl _
      · exact scriptVerdict_mono _ _ _ _
          (foldV_mono _ _ _ (fun s _ => simpleVerdict_mono s policy rules))

/-- **S17, ceiling.** -/
theorem classify_ceiling (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (v c : action.Verdict)
    (hv : action.classify call policy rules = ok v) (hc : action.ceiling call policy = ok c) :
    rankV v ≤ rankV c := by
  have h1 := classify_spec call policy rules
  have h2 := ceiling_spec call policy
  rw [hv, WP.spec_ok] at h1
  rw [hc, WP.spec_ok] at h2
  rw [h1, h2]
  exact verdictModel_mono call policy rules.val

/-- **S17, unknown tools.** -/
theorem classify_unknown (policy : action.Policy) (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    action.classify .Unknown policy rules ⦃ v => v = .Desktop ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl
  rfl

/-- A command that parses gets at most the answer of each of its simple commands. -/
theorem command_le_simple (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (script : shell.Script)
    (hsplit : shell.split (alloc.vec.Vec.deref raw) = ok (some script)) (s : shell.Simple)
    (hs : s ∈ script.simples.val) (k : Nat) (hk : 1 ≤ k)
    (hle : rankV (simpleVerdict s policy rules.val .Listed) ≤ k) :
    action.classify (.Command raw cwd) policy rules ⦃ v => rankV v ≤ k ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl
  simp only [verdictModel, commandModel]
  split
  · simp [rankV]; omega
  · simp only [hsplit, okOr]
    have := foldV_le _ (fun s => simpleVerdict s policy rules.val .Listed) s hs
    have := scriptVerdict_le
      (okOr (action.redirects_verdict (alloc.vec.Vec.deref script.redirects) (alloc.vec.Vec.deref cwd) policy)
        .Desktop)
      (okOr (action.has_relative_target (alloc.vec.Vec.deref script.redirects)) true &&
        okOr (command_rules.changes_folder (alloc.vec.Vec.deref script.simples)) true)
      (foldV script.simples.val (fun s => simpleVerdict s policy rules.val .Listed))
    unfold scriptModel
    omega

/-- **S17, never always.** -/
theorem classify_never_always (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (script : shell.Script)
    (hsplit : shell.split (alloc.vec.Vec.deref raw) = ok (some script))
    (h : ∃ s ∈ script.simples.val, neverAlways (words s) ∨ desktopSimple s) :
    action.classify (.Command raw cwd) policy rules ⦃ v => rankV v ≤ rankV .Ask ⦄ := by
  obtain ⟨s, hs, hna⟩ := h
  apply command_le_simple raw cwd policy rules script hsplit s hs _ (by simp [rankV])
  unfold simpleVerdict
  split
  · simp [rankV]
  · rw [if_pos (hna.resolve_right (by assumption))]

/-- **S28, desktop words.** -/
theorem classify_desktop (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (script : shell.Script)
    (hsplit : shell.split (alloc.vec.Vec.deref raw) = ok (some script))
    (h : ∃ s ∈ script.simples.val, desktopSimple s) :
    action.classify (.Command raw cwd) policy rules ⦃ v => rankV v ≤ rankV .Desktop ⦄ := by
  obtain ⟨s, hs, hd⟩ := h
  apply command_le_simple raw cwd policy rules script hsplit s hs _ (by simp [rankV])
  unfold simpleVerdict
  rw [if_pos hd]

/-- **S28, runners and network tools.** -/
theorem classify_capped (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (script : shell.Script)
    (hsplit : shell.split (alloc.vec.Vec.deref raw) = ok (some script))
    (h : ∃ s ∈ script.simples.val, runner (words s) ∨ network (words s)) :
    action.classify (.Command raw cwd) policy rules ⦃ v => rankV v ≤ rankV .Ask ⦄ := by
  obtain ⟨s, hs, hr⟩ := h
  apply classify_never_always raw cwd policy rules script hsplit ⟨s, hs, Or.inl ?_⟩
  rcases hr with hr | hr
  · exact Or.inl hr
  · exact Or.inr (Or.inl hr)

/-- **S28, no parse.** -/
theorem classify_no_parse (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8)))
    (hsplit : shell.split (alloc.vec.Vec.deref raw) = ok none) :
    action.classify (.Command raw cwd) policy rules ⦃ v => v = .Desktop ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl
  simp only [verdictModel, commandModel]
  split
  · rfl
  · simp only [hsplit, okOr]

/-- **S28, substitution.** -/
theorem classify_substitution (raw cwd : alloc.vec.Vec U8) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) (h : substitution (bytes raw.val)) :
    action.classify (.Command raw cwd) policy rules ⦃ v => v = .Desktop ⦄ := by
  apply WP.spec_mono (classify_spec _ policy rules)
  rintro v rfl
  simp only [verdictModel, commandModel, deref_val]
  rw [if_pos (Or.inr h)]

/-- **S27.** -/
theorem classify_total (call : action.ToolCall) (policy : action.Policy)
    (rules : Slice (alloc.vec.Vec (alloc.vec.Vec U8))) :
    action.classify call policy rules ⦃ _ => True ⦄ :=
  WP.spec_mono (classify_spec call policy rules) (fun _ _ => trivial)

theorem ceiling_total (call : action.ToolCall) (policy : action.Policy) :
    action.ceiling call policy ⦃ _ => True ⦄ :=
  WP.spec_mono (ceiling_spec call policy) (fun _ _ => trivial)

end Protocol.Action
