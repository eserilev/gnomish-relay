import Protocol.Search
import Protocol.Folder.Code
import Protocol.Spec.Action

/-! # The path rules of the action classifier (S16) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.Folder

namespace Protocol.PathRules

@[simp] theorem bytes_len (l : List U8) : (bytes l).length = l.length := by simp [bytes]

@[simp] theorem deref_val {α : Type} (v : alloc.vec.Vec α) : (alloc.vec.Vec.deref v).val = v.val := rfl

@[step]
theorem to_lower_spec (b : U8) : ascii.to_lower b ⦃ r => r.bv = lowerByte b.bv ⦄ := by
  unfold ascii.to_lower
  step*
  all_goals unfold lowerByte
  · have h1 : b.val ≤ 90 := by scalar_tac
    rw [if_pos (by simp; scalar_tac)]
    apply BitVec.eq_of_toNat_eq
    have h32 : (32 : BitVec 8).toNat = 32 := rfl
    simp only [UScalar.bv_toNat, r_post, BitVec.toNat_add, h32]
    omega
  · rw [if_neg (by simp; scalar_tac)]
  · rw [if_neg (by simp; scalar_tac)]

theorem lower_append (a b : List Spec.Byte) : lower (a ++ b) = lower a ++ lower b := by
  simp [lower]

@[step]
theorem lower_bytes_spec (s : Slice U8) :
    path_rules.lower_bytes s ⦃ v => bytes v.val = lower (bytes s.val) ∧ v.val.length = s.val.length ⦄ := by
  unfold path_rules.lower_bytes path_rules.lower_bytes_loop
  apply loop.spec_decr_nat (fun st => s.val.length - st.2.val)
    (fun st => st.2.val ≤ s.val.length ∧ st.1.val.length = st.2.val ∧
      bytes st.1.val = lower (bytes (s.val.take st.2.val))) _ _ _ _
    ⟨by simp, by simp, by simp [bytes, lower]⟩
  rintro ⟨out, i⟩ ⟨hi, hlen, hout⟩
  simp only at hi hlen hout
  unfold path_rules.lower_bytes_loop.body
  step*
  · have hlt : i.val < s.val.length := by scalar_tac
    refine ⟨by scalar_tac, by simp [out1_post, hlen, i4_post], ?_, by scalar_tac⟩
    rw [out1_post, i4_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [bytes, List.map_append, List.map_cons, List.map_nil, Option.toList_some] at hout ⊢
    rw [hout, i3_post, i2_post]
    simp [lower]
  · have : i.val = s.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

@[step]
theorem lower_all_spec (list : Slice (alloc.vec.Vec U8)) :
    path_rules.lower_all list ⦃ v => strs v.val = (strs list.val).map lower ⦄ := by
  unfold path_rules.lower_all path_rules.lower_all_loop
  apply loop.spec_decr_nat (fun st => list.val.length - st.2.val)
    (fun st => st.2.val ≤ list.val.length ∧ st.1.val.length = st.2.val ∧
      strs st.1.val = (strs (list.val.take st.2.val)).map lower) _ _ _ _
    ⟨by simp, by simp, by simp [strs]⟩
  rintro ⟨out, i⟩ ⟨hi, hlen, hout⟩
  simp only at hi hlen hout
  unfold path_rules.lower_all_loop.body
  step*
  · have hlt : i.val < list.val.length := by scalar_tac
    refine ⟨by scalar_tac, by simp [out1_post, hlen, i2_post], ?_, by scalar_tac⟩
    rw [out1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [strs, List.map_append, List.map_cons, List.map_nil, Option.toList_some] at hout ⊢
    rw [hout, v1_post1, v_post]
    simp
  · have : i.val = list.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact hout

@[step]
theorem is_named_part_spec (part : Slice U8) :
    path_rules.is_named_part part ⦃ r =>
      (r = true ↔ bytes part.val ≠ ascii "." ∧ bytes part.val ≠ ascii "..") ⦄ := by
  unfold path_rules.is_named_part
  step*

@[simp, scalar_tac_simps]
theorem max_path_val : path_rules.MAX_PATH.val = 2 ^ 20 := by unfold path_rules.MAX_PATH; rfl

def noDots (parts : List (List Spec.Byte)) : Prop := ∀ p ∈ parts, p ≠ ascii "." ∧ p ≠ ascii ".."

@[step]
theorem no_dot_parts_spec (parts : Slice (alloc.vec.Vec U8)) :
    path_rules.no_dot_parts parts ⦃ r => (r = true ↔ noDots (strs parts.val)) ⦄ := by
  unfold path_rules.no_dot_parts path_rules.no_dot_parts_loop
  apply loop.spec_decr_nat (fun st => parts.val.length - st.2.val)
    (fun st => st.2.val ≤ parts.val.length ∧ (st.1 = true ↔ noDots (strs (parts.val.take st.2.val)))) _ _ _ _
    ⟨by simp, by simp [noDots, strs]⟩
  rintro ⟨clean, i⟩ ⟨hi, hclean⟩
  simp only at hi hclean
  unfold path_rules.no_dot_parts_loop.body
  step*
  · have hlt : i.val < parts.val.length := by scalar_tac
    have hc : noDots (strs (parts.val.take i.val)) := hclean.mp (by assumption)
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [clean1_post, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, ← v_post]
    simp only [noDots, strs, List.map_append, List.mem_append, Option.toList_some, List.map_cons,
      List.map_nil, List.mem_singleton, deref_val] at hc ⊢
    constructor
    · rintro h p (hp | rfl)
      · exact hc p hp
      · exact h
    · intro h
      exact h _ (Or.inr rfl)
  · have : i.val = parts.val.length := by scalar_tac
    rw [this, List.take_length] at hclean
    simp only [true_iff]
    exact hclean.mp (by assumption)
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have hn : ¬ noDots (strs (parts.val.take i.val)) := by rw [← hclean]; assumption
    apply hn
    intro p hp
    exact h p (by simp only [strs] at hp ⊢; exact List.map_subset _ (List.take_subset _ _) hp)

@[step]
theorem is_clean_spec (path : Slice U8) :
    path_rules.is_clean path ⦃ r => (r = true ↔ path.val.length ≤ 2 ^ 20 ∧ cleanPath (bytes path.val)) ⦄ := by
  unfold path_rules.is_clean
  step*
  · simp
  · rw [List.take_of_length_le (by simp)]
    simp only [deref_val]
    rw [parts_post1]
    have := joinParts_pathParts_length (bytes path.val)
    have : (bytes path.val).length = path.val.length := by simp [bytes]
    scalar_tac
  · simp only [deref_val] at b_post r_post v_post
    have hparts : strs parts.val = pathParts (bytes path.val) := parts_post1
    have hn : noDots (strs parts.val) := b_post.mp (by assumption)
    rw [r_post]
    rw [List.take_of_length_le (by simp)] at v_post
    have hne : pathParts (bytes path.val) ≠ [] := by
      rw [← hparts]
      have : 0 < parts.val.length := by scalar_tac
      simp only [strs, ne_eq, List.map_eq_nil_iff]
      exact List.ne_nil_of_length_pos this
    constructor
    · intro h
      refine ⟨?_, ?_, hne, ?_⟩
      · have hm : ¬path.len > path_rules.MAX_PATH := by assumption
        rw [gt_iff_lt, UScalar.lt_equiv, max_path_val] at hm
        simpa using hm
      · show bytes path.val = joinParts (pathParts (bytes path.val))
        rw [← parts_post1, ← v_post, h]
      · rw [← hparts]; exact hn
    · rintro ⟨_, hj, _, _⟩
      rw [← Protocol.Seen.bytes_eq_iff, v_post, parts_post1]
      exact hj.symm
  · simp only [deref_val] at b_post
    simp only [Bool.false_eq_true, false_iff, not_and]
    intro _ hc
    have hn : ¬ noDots (strs parts.val) := by rw [← b_post]; assumption
    apply hn
    have hparts : strs parts.val = pathParts (bytes path.val) := parts_post1
    rw [hparts]
    exact hc.2.2
  · simp only [Bool.false_eq_true, false_iff, not_and]
    intro _ hc
    have h0 : parts.val.length = 0 := by scalar_tac
    have : pathParts (bytes path.val) = [] := by
      rw [← parts_post1, List.eq_nil_of_length_eq_zero h0]; rfl
    exact hc.2.1 this

theorem star_bv (x : U8) : x.bv = ch '*' ↔ x.val = 42 := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simpa [ch] using this
  · intro h; apply BitVec.eq_of_toNat_eq; simp [ch, h]

@[step]
theorem part_matches_spec (pp part : Slice U8) :
    path_rules.part_matches pp part ⦃ r => (r = true ↔ partMatch (bytes pp.val) (bytes part.val)) ⦄ := by
  unfold path_rules.part_matches
  have hlast : (bytes pp.val).getLast? = pp.val[pp.val.length - 1]?.map (·.bv) := by
    simp [bytes, List.getLast?_eq_getElem?]
  have hstar : ch '*' = (42#u8 : U8).bv := by rfl
  step*
  · have hlen : i.val = pp.val.length - 1 := by scalar_tac
    have hl : (bytes pp.val).getLast? = some (ch '*') := by
      have hlt : i.val < pp.val.length := by scalar_tac
      rename_i h42 _
      rw [hlast, ← hlen, List.getElem?_eq_getElem hlt, ← i1_post, h42]
      rfl
    rw [r_post, partMatch, if_pos hl, List.drop_zero, List.prefix_iff_eq_take]
    have hd : (bytes pp.val).dropLast = bytes (pp.val.take i.val) := by
      simp [bytes, List.dropLast_eq_take, hlen]
    rw [hd]
    have hbl : (bytes (pp.val.take i.val)).length = i.val := by simp [bytes]; scalar_tac
    rw [hbl]
    have ht : (bytes part.val).take i.val = bytes (part.val.take i.val) := by simp [bytes]
    rw [ht, Protocol.Seen.bytes_eq_iff]
    exact eq_comm
  · simp only [Bool.false_eq_true, false_iff]
    have hlen : i.val = pp.val.length - 1 := by scalar_tac
    have hl : (bytes pp.val).getLast? = some (ch '*') := by
      have hlt : i.val < pp.val.length := by scalar_tac
      rename_i h42 _
      rw [hlast, ← hlen, List.getElem?_eq_getElem hlt, ← i1_post, h42]
      rfl
    rw [partMatch, if_pos hl]
    intro h
    have := h.length_le
    simp [bytes] at this
    scalar_tac
  · rw [r_post, partMatch, if_neg, Protocol.Seen.bytes_eq_iff]
    rw [hlast, List.getElem?_eq_getElem (by scalar_tac)]
    simp only [Option.map_some, Option.some.injEq]
    intro h
    rename_i h42
    apply h42
    rw [i1_post]
    have hlen : i.val = pp.val.length - 1 := by scalar_tac
    simp only [hlen]
    exact (UScalar.eq_equiv_bv_eq _ _).mpr (h.trans hstar)
  · rw [r_post, partMatch, if_neg, Protocol.Seen.bytes_eq_iff]
    have : pp.val = [] := List.eq_nil_of_length_eq_zero (by scalar_tac)
    simp [this, bytes]

/-- The pattern matches the parts from `start` on. -/
def MatchesAt (pattern parts : List (alloc.vec.Vec U8)) (start k : Nat) : Prop :=
  ∀ j (hj : j < k) (h1 : j < pattern.length) (h2 : start + j < parts.length),
    partMatch (bytes pattern[j].val) (bytes parts[start + j].val)

@[step]
theorem pattern_at_spec (pattern parts : Slice (alloc.vec.Vec U8)) (start : Usize)
    (h : start.val + pattern.val.length ≤ parts.val.length) :
    path_rules.pattern_at pattern parts start ⦃ r =>
      (r = true ↔ MatchesAt pattern.val parts.val start.val pattern.val.length) ⦄ := by
  unfold path_rules.pattern_at path_rules.pattern_at_loop
  apply loop.spec_decr_nat (fun st => pattern.val.length - st.2.val)
    (fun st => st.2.val ≤ pattern.val.length ∧
      (st.1 = true ↔ MatchesAt pattern.val parts.val start.val st.2.val)) _ _ _ _
    ⟨by simp, by simp [MatchesAt]⟩
  rintro ⟨same, k⟩ ⟨hk, hsame⟩
  simp only at hk hsame
  unfold path_rules.pattern_at_loop.body
  step*
  · have hm : MatchesAt pattern.val parts.val start.val k.val := hsame.mp (by assumption)
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [same1_post, k1_post]
    simp only [deref_val, v_post, v1_post, i1_post]
    constructor
    · intro hp j hj h1 h2
      by_cases hjk : j < k.val
      · exact hm j hjk h1 h2
      · have : j = k.val := by omega
        subst this
        exact hp
    · intro hall
      exact hall k.val (by omega) (by scalar_tac) (by scalar_tac)
  · simp only [Bool.false_eq_true, false_iff]
    intro hall
    have hn : ¬ MatchesAt pattern.val parts.val start.val k.val := by rw [← hsame]; assumption
    exact hn (fun j hj h1 h2 => hall j h1 h1 h2)

def FoundAt (pattern parts : List (alloc.vec.Vec U8)) (n : Nat) : Prop :=
  ∃ i < n, i + pattern.length ≤ parts.length ∧ MatchesAt pattern parts i pattern.length

theorem patternIn_iff (pattern parts : List (alloc.vec.Vec U8)) :
    patternIn (strs pattern) (strs parts) ↔
      ∃ i, i + pattern.length ≤ parts.length ∧ MatchesAt pattern parts i pattern.length := by
  unfold patternIn MatchesAt strs
  simp only [List.length_map, List.getElem_map]
  constructor
  · rintro ⟨i, hi, h⟩
    exact ⟨i, hi, fun j _ h1 _ => h j h1⟩
  · rintro ⟨i, hi, h⟩
    exact ⟨i, hi, fun k hk => h k hk hk (by omega)⟩

@[step]
theorem pattern_in_loop_spec (pattern parts : Slice (alloc.vec.Vec U8)) (last : Usize)
    (hlast : last.val + pattern.val.length = parts.val.length) (hroom : parts.val.length < Usize.max) :
    path_rules.pattern_in_loop pattern parts last false 0#usize ⦃ r =>
      (r = true ↔ FoundAt pattern.val parts.val (last.val + 1)) ⦄ := by
  unfold path_rules.pattern_in_loop
  apply loop.spec_decr_nat (fun st => last.val + 1 - st.2.val)
    (fun st => st.2.val ≤ last.val + 1 ∧ (st.1 = true ↔ FoundAt pattern.val parts.val st.2.val)) _ _ _ _
    ⟨by simp, by simp [FoundAt]⟩
  rintro ⟨found, pos⟩ ⟨hpos, hfound⟩
  simp only at hpos hfound
  unfold path_rules.pattern_in_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨i, hi, h⟩ := hfound.mp (by assumption)
    exact ⟨i, by omega, h⟩
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hnot : ¬ FoundAt pattern.val parts.val pos.val := by rw [← hfound]; assumption
    rw [found1_post, at1_post]
    constructor
    · intro h
      exact ⟨pos.val, by omega, by scalar_tac, h⟩
    · rintro ⟨i, hi, hl, h⟩
      by_cases hip : i < pos.val
      · exact absurd ⟨i, hip, hl, h⟩ hnot
      · have : i = pos.val := by omega
        subst this
        exact h

@[step]
theorem pattern_in_spec (pattern parts : Slice (alloc.vec.Vec U8)) (hroom : parts.val.length < Usize.max) :
    path_rules.pattern_in pattern parts ⦃ r => (r = true ↔ patternIn (strs pattern.val) (strs parts.val)) ⦄ := by
  unfold path_rules.pattern_in
  rw [patternIn_iff]
  step*
  rw [r_post]
  constructor
  · rintro ⟨i, _, hl, h⟩
    exact ⟨i, hl, h⟩
  · rintro ⟨i, hl, h⟩
    exact ⟨i, by scalar_tac, hl, h⟩

theorem strs_eq_pb (l : List (alloc.vec.Vec U8)) : strs l = pb l := rfl

def AnyPattern (patterns : List (alloc.vec.Vec U8)) (p : List Spec.Byte) : Prop :=
  ∃ pat ∈ strs patterns, patternIn (pathParts (lower pat)) (pathParts (lower p))

@[step]
theorem matches_any_pattern_spec (path : Slice U8) (patterns : Slice (alloc.vec.Vec U8))
    (hroom : path.val.length < Usize.max) :
    path_rules.matches_any_pattern path patterns ⦃ r =>
      (r = true ↔ matchesPattern (strs patterns.val) (bytes path.val)) ⦄ := by
  unfold path_rules.matches_any_pattern path_rules.matches_any_pattern_loop
  step*
  apply loop.spec_decr_nat (fun st => patterns.val.length - st.2.val)
    (fun st => st.2.val ≤ patterns.val.length ∧
      (st.1 = true ↔ AnyPattern (patterns.val.take st.2.val) (bytes path.val))) _ _ _ _
    ⟨by simp, by simp [AnyPattern, strs]⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold path_rules.matches_any_pattern_loop.body
  simp only [deref_val] at parts_post1 parts_post2
  rw [v_post1] at parts_post1
  step*
  · simp only [true_iff]
    obtain ⟨pat, hp, h⟩ := hfound.mp (by assumption)
    refine ⟨pat, ?_, h⟩
    simp only [strs] at hp ⊢
    exact List.map_subset _ (List.take_subset _ _) hp
  · simp only [deref_val]; scalar_tac
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hnot : ¬ AnyPattern (patterns.val.take i.val) (bytes path.val) := by
      rw [← hfound]; assumption
    have hlt : i.val < patterns.val.length := by scalar_tac
    simp only [deref_val] at v1_post1 v2_post1 found1_post
    rw [found1_post, i2_post, strs_eq_pb, strs_eq_pb, v2_post1, v1_post1, parts_post1, v_post]
    rw [List.take_add_one, List.getElem?_eq_getElem hlt]
    simp only [AnyPattern, strs, List.map_append, List.mem_append, Option.toList_some, List.map_cons,
      List.map_nil, List.mem_singleton] at hnot ⊢
    constructor
    · intro h
      exact ⟨_, Or.inr rfl, h⟩
    · rintro ⟨pat, (hp | rfl), h⟩
      · exact absurd ⟨pat, hp, h⟩ hnot
      · exact h
  · simp only [Bool.false_eq_true, false_iff]
    have : i.val = patterns.val.length := by scalar_tac
    rw [this, List.take_length] at hfound
    intro h
    exact (by assumption : ¬ found = true) (hfound.mpr h)

@[step]
theorem access_eq_spec (a b : shell.Access) :
    shell.Access.Insts.CoreCmpPartialEqAccess.eq a b ⦃ r => (r = true ↔ a = b) ⦄ := by
  induction a <;> induction b <;>
    simp [shell.Access.Insts.CoreCmpPartialEqAccess.eq, shell.Access.read_discriminant]

@[step]
theorem is_denied_spec (path : Slice U8) (folders : Slice (alloc.vec.Vec U8)) :
    path_rules.is_denied path folders ⦃ r =>
      (r = true ↔ ∃ f ∈ strs folders.val, insideCI f (bytes path.val)) ⦄ := by
  unfold path_rules.is_denied
  step*
  · simp
  · simp only [deref_val] at parts_post1 r_post
    rw [v_post1] at parts_post1
    rw [r_post]
    unfold insideOne insideCI insideRoot
    have hlen : parts.len.val = parts.val.length := by simp
    rw [hlen, List.take_length, parts_post1]
    constructor
    · rintro ⟨root, hr, h⟩
      have hm : bytes root.val ∈ strs v1.val := List.mem_map_of_mem hr
      rw [v1_post, List.mem_map] at hm
      obtain ⟨f, hf, he⟩ := hm
      exact ⟨f, hf, by rw [he]; exact h⟩
    · rintro ⟨f, hf, h⟩
      have hm : lower f ∈ strs v1.val := by rw [v1_post]; exact List.mem_map_of_mem hf
      simp only [strs, List.mem_map] at hm
      obtain ⟨root, hr, he⟩ := hm
      exact ⟨root, hr, by rw [he]; exact h⟩

/-- The checks after `deny`: a clean path of at most 1 MiB, no `desktop` pattern, and
inside the roots for a read or inside the chat folder for a write. -/
def PathOk (policy : action.Policy) (access : shell.Access) (p : List Spec.Byte) : Prop :=
  (p.length ≤ 2 ^ 20 ∧ cleanPath p) ∧ ¬ matchesPattern (strs policy.desktop_paths.val) p ∧
    match access with
    | .Read => ∃ root ∈ strs policy.roots.val, insideRoot root p
    | .Write => insideRoot (bytes policy.chat.val) p ∧ ¬ matchesPattern (strs policy.desktop_writes.val) p

@[step]
theorem path_allowed_spec (path : Slice U8) (access : shell.Access) (policy : action.Policy) :
    path_rules.path_allowed path access policy ⦃ r => (r = true ↔ PathOk policy access (bytes path.val)) ⦄ := by
  unfold path_rules.path_allowed
  step*
  · simp only [Bool.false_eq_true, false_iff, PathOk, deref_val] at b1_post ⊢
    intro h
    exact h.2.1 (b1_post.mp (by assumption))
  · simp
  · have hr : access = .Read := b2_post.mp (by assumption)
    subst hr
    simp only [deref_val] at b_post b1_post r_post
    have hlen : parts.len.val = parts.val.length := by simp
    unfold insideOne at r_post
    rw [r_post, hlen, List.take_length, parts_post1]
    have hb := b_post.mp (by assumption)
    have hb1 : ¬ matchesPattern (strs policy.desktop_paths.val) (bytes path.val) := by
      rw [← b1_post]; assumption
    unfold PathOk insideRoot
    constructor
    · rintro ⟨root, hr, h⟩
      exact ⟨⟨by simpa using hb.1, hb.2⟩, hb1, bytes root.val, List.mem_map_of_mem hr, h⟩
    · rintro ⟨_, _, _, hm, h⟩
      simp only [strs, List.mem_map] at hm
      obtain ⟨root, hr, rfl⟩ := hm
      exact ⟨root, hr, h⟩
  · simp
  · have hr : access = .Write := by
      cases access
      · exact absurd (b2_post.mpr rfl) (by assumption)
      · rfl
    subst hr
    simp only [deref_val] at b_post b1_post b3_post b4_post v_post1
    have hlen : parts.len.val = parts.val.length := by simp
    rw [hlen, List.take_length, v_post1, parts_post1] at b3_post
    have hb := b_post.mp (by assumption)
    have hb1 : ¬ matchesPattern (strs policy.desktop_paths.val) (bytes path.val) := by
      rw [← b1_post]; assumption
    have hb3 := b3_post.mp (by assumption)
    simp only [PathOk, bytes_len, hb, hb1, not_false_eq_true, true_and, insideRoot, hb3, decide_eq_true_iff,
      b4_post]
  · have hr : access = .Write := by
      cases access
      · exact absurd (b2_post.mpr rfl) (by assumption)
      · rfl
    subst hr
    simp only [deref_val] at b3_post v_post1
    have hlen : parts.len.val = parts.val.length := by simp
    rw [hlen, List.take_length, v_post1, parts_post1] at b3_post
    simp only [Bool.false_eq_true, false_iff, PathOk, insideRoot, not_and]
    intro _ _ h _
    exact (by assumption : ¬ b3 = true) (b3_post.mpr h)
  · simp only [Bool.false_eq_true, false_iff, PathOk, not_and, bytes_len]
    intro h
    exact absurd (b_post.mpr h) (by assumption)

@[step]
theorem has_glob_spec (s : Slice U8) : path_rules.has_glob s ⦃ _ => True ⦄ := by
  unfold path_rules.has_glob
  step*

@[step]
theorem is_literal_target_spec (s : Slice U8) : path_rules.is_literal_target s ⦃ _ => True ⦄ := by
  unfold path_rules.is_literal_target
  step*

@[step]
theorem top_root_spec : path_rules.top_root ⦃ _ => True ⦄ := by
  unfold path_rules.top_root
  step*

open Classical in
/-- The answer for one resolved path. -/
noncomputable def pathVerdict (policy : action.Policy) (access : shell.Access) (p : List Spec.Byte) :
    action.Verdict :=
  if denied policy p then .Deny else if PathOk policy access p then .Allow else .Desktop

@[step]
theorem path_verdict_spec (path : Slice U8) (access : shell.Access) (policy : action.Policy) :
    path_rules.path_verdict path access policy ⦃ v => v = pathVerdict policy access (bytes path.val) ⦄ := by
  unfold path_rules.path_verdict
  step*
  · simp only [deref_val] at b_post
    rw [pathVerdict, if_pos (show denied policy _ from b_post.mp (by assumption))]
  · simp only [deref_val] at b_post
    rw [pathVerdict, if_neg (by unfold denied; rw [← b_post]; assumption),
      if_pos (b1_post.mp (by assumption))]
  · simp only [deref_val] at b_post
    rw [pathVerdict, if_neg (by unfold denied; rw [← b_post]; assumption),
      if_neg (by rw [← b1_post]; assumption)]

@[step]
theorem target_verdict_spec (target : Slice U8) (access : shell.Access) (cwd : Slice U8)
    (policy : action.Policy) :
    path_rules.target_verdict target access cwd policy ⦃ _ => True ⦄ := by
  unfold path_rules.target_verdict
  step*
  step with resolve_folder_spec v.deref cwd target (by scalar_tac) as ⟨o, ho⟩
  cases o <;> step*

end Protocol.PathRules
