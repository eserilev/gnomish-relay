import Protocol.Spec.Folder

/-! # Path facts, without Rust -/

open Protocol.Spec

namespace Protocol.Folder

/-- A part as `split_parts` makes it: not empty, no slash. -/
def goodPart (p : List Spec.Byte) : Prop := p ≠ [] ∧ slash ∉ p

/-- A part that can stay in a clean path. -/
def cleanPart (p : List Spec.Byte) : Prop := goodPart p ∧ p ≠ ascii "." ∧ p ≠ ascii ".."

/-- `/` plus each part. -/
def joinParts (ps : List (List Spec.Byte)) : List Spec.Byte := ps.flatMap (slash :: ·)

theorem pathParts_append_slash (a b : List Spec.Byte) :
    pathParts (a ++ slash :: b) = pathParts a ++ pathParts b := by
  simp [pathParts, List.splitOn_append_cons_self, List.filter_append]

theorem pathParts_slash_cons (b : List Spec.Byte) : pathParts (slash :: b) = pathParts b := by
  simpa [pathParts] using pathParts_append_slash [] b

theorem pathParts_no_slash (cur : List Spec.Byte) (h : slash ∉ cur) :
    pathParts cur = if cur = [] then [] else [cur] := by
  unfold pathParts
  rw [List.splitOn_eq_singleton h]
  split <;> simp_all

theorem pathParts_good (x : List Spec.Byte) : ∀ p ∈ pathParts x, goodPart p := by
  intro p hp
  unfold pathParts at hp
  rw [List.mem_filter] at hp
  refine ⟨by simpa using hp.2, fun hs => ?_⟩
  -- A part of `splitOn` never holds the separator.
  have key : ∀ (l : List Spec.Byte), ∀ q ∈ l.splitOn slash, slash ∉ q := by
    intro l
    induction l with
    | nil => simp
    | cons b t ih =>
      rw [List.splitOn_cons_eq_if_modifyHead]
      split
      · intro q hq
        rcases List.mem_cons.mp hq with rfl | hq
        · simp
        · exact ih q hq
      · rename_i hb
        intro q hq
        have hne := List.splitOn_ne_nil slash t
        generalize t.splitOn slash = ls at *
        cases ls with
        | nil => exact absurd rfl hne
        | cons l0 ls =>
          simp only [List.modifyHead_cons, List.mem_cons] at hq
          rcases hq with rfl | hq
          · intro hm
            rcases List.mem_cons.mp hm with h | h
            · exact hb (by simp [h])
            · exact ih l0 (by simp) h
          · exact ih q (by simp [hq])
  exact key x p hp.1 hs

theorem joinParts_cons (p : List Spec.Byte) (ps : List (List Spec.Byte)) :
    joinParts (p :: ps) = slash :: (p ++ joinParts ps) := by
  simp [joinParts]

theorem joinParts_append (a b : List (List Spec.Byte)) : joinParts (a ++ b) = joinParts a ++ joinParts b := by
  simp [joinParts]

theorem pathParts_joinParts (ps : List (List Spec.Byte)) (h : ∀ p ∈ ps, goodPart p) :
    pathParts (joinParts ps) = ps := by
  induction ps with
  | nil => simp [joinParts, pathParts]
  | cons p ps ih =>
    have hp := h p (by simp)
    have ih := ih (fun q hq => h q (by simp [hq]))
    rw [joinParts_cons, pathParts_slash_cons]
    cases ps with
    | nil => simp [joinParts, pathParts_no_slash p hp.2, hp.1]
    | cons q qs =>
      rw [joinParts_cons, pathParts_append_slash, pathParts_no_slash p hp.2, if_neg hp.1,
        ← pathParts_slash_cons, ← joinParts_cons, ih]
      rfl

theorem joinParts_clean (ps : List (List Spec.Byte)) (hne : ps ≠ []) (h : ∀ p ∈ ps, cleanPart p) :
    cleanPath (joinParts ps) := by
  have hg : ∀ p ∈ ps, goodPart p := fun p hp => (h p hp).1
  rw [cleanPath, pathParts_joinParts ps hg]
  exact ⟨rfl, hne, fun p hp => (h p hp).2⟩

/-! ## The splitter, as the Rust loop runs it -/

def endPart (ps : List (List Spec.Byte)) (cur : List Spec.Byte) : List (List Spec.Byte) :=
  ps ++ if cur = [] then [] else [cur]

def splitGo : List Spec.Byte → List (List Spec.Byte) → List Spec.Byte → List (List Spec.Byte)
  | [], ps, cur => endPart ps cur
  | b :: t, ps, cur => if b = slash then splitGo t (endPart ps cur) [] else splitGo t ps (cur ++ [b])

theorem splitGo_eq (l : List Spec.Byte) (ps : List (List Spec.Byte)) (cur : List Spec.Byte)
    (h : slash ∉ cur) : splitGo l ps cur = ps ++ pathParts (cur ++ l) := by
  induction l generalizing ps cur with
  | nil => simp [splitGo, endPart, pathParts_no_slash cur h]
  | cons b t ih =>
    unfold splitGo
    split
    · rename_i hb
      subst hb
      rw [ih _ _ (by simp), pathParts_append_slash, pathParts_no_slash cur h]
      simp [endPart]
    · rename_i hb
      rw [ih _ _ (by simp [h, Ne.symm hb])]
      simp

theorem splitGo_nil (l : List Spec.Byte) : splitGo l [] [] = pathParts l := by
  simpa using splitGo_eq l [] [] (by simp)

/-! ## The walk over parts -/

/-- One part: `.` stays, `..` goes up, and `none` means a `..` went above `/`. -/
def walkStep : Option (List (List Spec.Byte)) → List Spec.Byte → Option (List (List Spec.Byte))
  | none, _ => none
  | some st, part =>
    if part = ascii "." then some st
    else if part = ascii ".." then (if st = [] then none else some st.dropLast)
    else some (st ++ [part])

def walkAll (st : Option (List (List Spec.Byte))) (parts : List (List Spec.Byte)) :
    Option (List (List Spec.Byte)) :=
  parts.foldl walkStep st

theorem walkAll_none (parts : List (List Spec.Byte)) : walkAll none parts = none := by
  induction parts with
  | nil => rfl
  | cons p ps ih => simpa [walkAll, walkStep] using ih

theorem walkAll_append (st : Option (List (List Spec.Byte))) (a b : List (List Spec.Byte)) :
    walkAll st (a ++ b) = walkAll (walkAll st a) b := by
  simp [walkAll, List.foldl_append]

theorem walkAll_snoc (st : Option (List (List Spec.Byte))) (a : List (List Spec.Byte)) (p : List Spec.Byte) :
    walkAll st (a ++ [p]) = walkStep (walkAll st a) p := by
  simp [walkAll, List.foldl_append]

/-- Clean parts pass through the walk unchanged. -/
theorem walkAll_clean (st : List (List Spec.Byte)) (parts : List (List Spec.Byte))
    (h : ∀ p ∈ parts, p ≠ ascii "." ∧ p ≠ ascii "..") : walkAll (some st) parts = some (st ++ parts) := by
  induction parts generalizing st with
  | nil => simp [walkAll]
  | cons p ps ih =>
    have hp := h p (by simp)
    simp only [walkAll, List.foldl_cons, walkStep, if_neg hp.1, if_neg hp.2]
    rw [← walkAll, ih _ (fun q hq => h q (by simp [hq]))]
    simp

/-- The walk keeps only good parts that are not `.` or `..`. -/
theorem walkAll_parts_clean (st st' : List (List Spec.Byte)) (parts : List (List Spec.Byte))
    (hst : ∀ p ∈ st, cleanPart p) (hparts : ∀ p ∈ parts, goodPart p)
    (h : walkAll (some st) parts = some st') : ∀ p ∈ st', cleanPart p := by
  induction parts generalizing st with
  | nil => simp [walkAll] at h; subst h; exact hst
  | cons q qs ih =>
    have hq := hparts q (by simp)
    have hqs : ∀ p ∈ qs, goodPart p := fun p hp => hparts p (by simp [hp])
    simp only [walkAll, List.foldl_cons, walkStep] at h
    split_ifs at h with h1 h2 h3
    · exact ih st hst hqs h
    · rw [← walkAll, walkAll_none] at h; exact absurd h (by simp)
    · exact ih _ (fun p hp => hst p (List.mem_of_mem_dropLast hp)) hqs h
    · refine ih _ (fun p hp => ?_) hqs h
      rcases List.mem_append.mp hp with hp | hp
      · exact hst p hp
      · rw [List.mem_singleton] at hp; subst hp; exact ⟨hq, h1, h2⟩

/-! ## Sizes -/

theorem joinParts_length_splitGo (l : List Spec.Byte) (ps : List (List Spec.Byte)) (cur : List Spec.Byte) :
    (joinParts (splitGo l ps cur)).length ≤ (joinParts ps).length + cur.length + 1 + l.length := by
  induction l generalizing ps cur with
  | nil =>
    simp only [splitGo, endPart, joinParts_append]
    split <;> simp [joinParts] <;> omega
  | cons b t ih =>
    unfold splitGo
    split
    · have h1 := ih (endPart ps cur) []
      have h2 : (joinParts (endPart ps cur)).length ≤ (joinParts ps).length + cur.length + 1 := by
        unfold endPart; split <;> simp [joinParts] <;> omega
      simp only [List.length_nil, List.length_cons] at h1 ⊢
      omega
    · have := ih ps (cur ++ [b])
      simp only [List.length_append, List.length_cons, List.length_nil] at this ⊢
      omega

theorem joinParts_pathParts_length (l : List Spec.Byte) : (joinParts (pathParts l)).length ≤ l.length + 1 := by
  have := joinParts_length_splitGo l [] []
  rw [splitGo_nil] at this
  have h0 : (joinParts []).length = 0 := rfl
  simp only [List.length_nil, h0] at this
  omega

theorem joinParts_dropLast_length (st : List (List Spec.Byte)) :
    (joinParts st.dropLast).length ≤ (joinParts st).length := by
  rcases List.eq_nil_or_concat st with h | ⟨L, b, h⟩
  · simp [h]
  · subst h
    simp [joinParts_append]

theorem walkAll_length (st st' : List (List Spec.Byte)) (parts : List (List Spec.Byte))
    (h : walkAll (some st) parts = some st') :
    (joinParts st').length ≤ (joinParts st).length + (joinParts parts).length := by
  induction parts generalizing st with
  | nil => simp [walkAll] at h; subst h; simp [joinParts]
  | cons q qs ih =>
    simp only [walkAll, List.foldl_cons, walkStep] at h
    rw [joinParts_cons]
    split_ifs at h with h1 h2 h3
    · have := ih st h; simp; omega
    · rw [← walkAll, walkAll_none] at h; exact absurd h (by simp)
    · have := ih _ h; have := joinParts_dropLast_length st; simp; omega
    · have := ih _ h
      have hq : (joinParts [q]).length = q.length + 1 := by simp [joinParts]
      rw [joinParts_append] at this
      simp only [List.length_append, List.length_cons] at this ⊢
      omega

theorem length_le_joinParts (st : List (List Spec.Byte)) : st.length ≤ (joinParts st).length := by
  induction st with
  | nil => simp [joinParts]
  | cons p ps ih => rw [joinParts_cons]; simp; omega

/-! ## Joining a clean relative path -/

theorem joinParts_eq_intercalate (ls : List (List Spec.Byte)) (h : ls ≠ []) :
    joinParts ls = slash :: [slash].intercalate ls := by
  induction ls with
  | nil => exact absurd rfl h
  | cons l ls ih =>
    cases ls with
    | nil => simp [joinParts]
    | cons m ms =>
      rw [joinParts_cons, ih (by simp), List.intercalate_cons_cons]
      simp

end Protocol.Folder
