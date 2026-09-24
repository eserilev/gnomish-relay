import Protocol.Seen
import Protocol.Folder.Pure

/-! # The folder policy in Rust (S5) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Folder

@[simp, scalar_tac_simps]
theorem slash_val : folder.SLASH.val = 47 := by unfold folder.SLASH; rfl

theorem slash_iff (b : U8) : b.bv = slash ↔ b.val = 47 := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simpa [slash, ch] using this
  · intro h; apply BitVec.eq_of_toNat_eq; simp [slash, ch, h]

/-- The parts in a Rust vector of byte vectors. -/
def pb (l : List (alloc.vec.Vec U8)) : List (List Spec.Byte) := l.map (fun p => bytes p.val)

theorem pb_append (a b : List (alloc.vec.Vec U8)) : pb (a ++ b) = pb a ++ pb b := by simp [pb]

theorem bytes_nil_iff (l : List U8) : bytes l = [] ↔ l = [] := by simp [bytes]

@[step]
theorem end_part_spec (parts : alloc.vec.Vec (alloc.vec.Vec U8)) (cur : alloc.vec.Vec U8)
    (h : parts.val.length < Usize.max) :
    folder.end_part parts cur ⦃ r =>
      pb r.val = endPart (pb parts.val) (bytes cur.val) ∧ r.val.length ≤ parts.val.length + 1 ∧
        r.val.length ≤ parts.val.length + cur.val.length ⦄ := by
  unfold folder.end_part
  step*
  · refine ⟨?_, by simp [*], by simp [*]; scalar_tac⟩
    have hne : bytes cur.val ≠ [] := by rw [ne_eq, bytes_nil_iff]; intro h0; simp_all
    simp [endPart, pb, *]
  · refine ⟨?_, by simp, by simp⟩
    have he : bytes cur.val = [] := by
      rw [bytes_nil_iff]; exact List.eq_nil_of_length_eq_zero (by scalar_tac)
    simp [endPart, he]

@[step]
theorem take_byte_spec (parts : alloc.vec.Vec (alloc.vec.Vec U8)) (cur : alloc.vec.Vec U8) (b : U8)
    (hp : parts.val.length < Usize.max) (hc : cur.val.length < Usize.max) :
    folder.take_byte parts cur b ⦃ r =>
      (∀ t, splitGo t (pb r.1.val) (bytes r.2.val) = splitGo (b.bv :: t) (pb parts.val) (bytes cur.val)) ∧
      2 * r.1.val.length + r.2.val.length ≤ 2 * parts.val.length + cur.val.length + 1 ⦄ := by
  unfold folder.take_byte
  step*
  · have hs : folder.SLASH.bv = slash := (slash_iff _).mpr slash_val
    refine ⟨fun t => ?_, by simp; omega⟩
    conv => rhs; unfold splitGo
    rw [hs, if_pos rfl, v_post1]
    rfl
  · have hb : b.bv ≠ slash := by
      rw [ne_eq, slash_iff]; intro h47; apply (by assumption : ¬ b = folder.SLASH)
      exact UScalar.eq_of_val_eq (by simp [h47])
    refine ⟨fun t => ?_, by simp [cur1_post]; omega⟩
    conv => rhs; unfold splitGo
    rw [if_neg hb, cur1_post]
    simp [bytes]

def SplitInv (path : Slice U8) (st : alloc.vec.Vec (alloc.vec.Vec U8) × alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.2.val ≤ path.val.length ∧ 2 * st.1.val.length + st.2.1.val.length ≤ st.2.2.val ∧
    splitGo (bytes (path.val.drop st.2.2.val)) (pb st.1.val) (bytes st.2.1.val) = pathParts (bytes path.val)

theorem split_parts_loop_spec (path : Slice U8) :
    folder.split_parts_loop path (alloc.vec.Vec.new _) (alloc.vec.Vec.new U8) 0#usize ⦃ r =>
      endPart (pb r.1.val) (bytes r.2.val) = pathParts (bytes path.val) ∧
        2 * r.1.val.length + r.2.val.length ≤ path.val.length ⦄ := by
  unfold folder.split_parts_loop
  apply loop.spec_decr_nat (fun st => path.val.length - st.2.2.val) (SplitInv path) _ _ _ _
    ⟨by simp, by simp, by simp [pb, splitGo_nil, bytes]⟩
  rintro ⟨parts, cur, i⟩ ⟨hi, hlen, hgo⟩
  simp only at hi hlen hgo
  unfold folder.split_parts_loop.body
  step*
  · have hlt : i.val < path.val.length := by scalar_tac
    have hd : bytes (path.val.drop i.val) = i2.bv :: bytes (path.val.drop (i.val + 1)) := by
      rw [List.drop_eq_getElem_cons hlt, i2_post]; rfl
    unfold SplitInv
    dsimp only
    refine ⟨⟨by omega, by omega, ?_⟩, by omega⟩
    rw [i3_post, p_post1, ← hd, hgo]
  · have : i.val = path.val.length := by scalar_tac
    rw [this, List.drop_length] at hgo
    exact ⟨hgo, by omega⟩

@[step]
theorem split_parts_spec (path : Slice U8) :
    folder.split_parts path ⦃ r => pb r.val = pathParts (bytes path.val) ∧ r.val.length ≤ path.val.length ⦄ := by
  unfold folder.split_parts
  step with split_parts_loop_spec path as ⟨res, hgo, hlen⟩
  step*

theorem dot_bytes : ascii "." = [ch '.'] := rfl
theorem dot_dot_bytes : ascii ".." = [ch '.', ch '.'] := rfl

theorem dot_dot_ne_dot : ascii ".." ≠ ascii "." := by simp [dot_bytes, dot_dot_bytes]

theorem bv_dot_iff (x : U8) : x.bv = ch '.' ↔ x.val = 46 := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simpa [ch] using this
  · intro h; apply BitVec.eq_of_toNat_eq; simp [ch, h]

@[step]
theorem is_dot_spec (part : Slice U8) : folder.is_dot part ⦃ r => (r = true ↔ bytes part.val = ascii ".") ⦄ := by
  unfold folder.is_dot
  step*
  · obtain ⟨x, hx⟩ := List.length_eq_one_iff.mp (by scalar_tac : part.val.length = 1)
    have hi : i1 = x := by rw [i1_post]; simp [hx]
    subst hi
    rw [hx, decide_eq_true_iff, dot_bytes]
    simp only [bytes, List.map_cons, List.map_nil, List.cons.injEq, and_true, bv_dot_iff]
    exact ⟨fun h => by simp [h], fun h => UScalar.eq_of_val_eq (by simpa using h)⟩
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have := congrArg List.length h
    simp [bytes, ascii] at this
    scalar_tac

@[step]
theorem is_dot_dot_spec (part : Slice U8) :
    folder.is_dot_dot part ⦃ r => (r = true ↔ bytes part.val = ascii "..") ⦄ := by
  unfold folder.is_dot_dot
  step*
  all_goals first
    | (simp only [Bool.false_eq_true, false_iff]
       intro h
       have := congrArg List.length h
       simp [bytes, ascii] at this
       scalar_tac)
    | skip
  all_goals
    obtain ⟨x, y, hx⟩ := List.length_eq_two.mp (by scalar_tac : part.val.length = 2)
    rw [hx, dot_dot_bytes]
    simp only [bytes, List.map_cons, List.map_nil, List.cons.injEq, and_true, bv_dot_iff]
  · have h1 : i1 = x := by rw [i1_post]; simp [hx]
    have h2 : i2 = y := by rw [i2_post]; simp [hx]
    subst h1 h2
    have : i1.val = 46 := by scalar_tac
    rw [decide_eq_true_iff]
    exact ⟨fun h => ⟨this, by simp [h]⟩, fun h => UScalar.eq_of_val_eq (by simpa using h.2)⟩
  · have h1 : i1 = x := by rw [i1_post]; simp [hx]
    subst h1
    simp only [Bool.false_eq_true, false_iff, not_and]
    intro h
    have hne : ¬ i1 = 46#u8 := by assumption
    exact absurd (UScalar.eq_of_val_eq (by simp [h])) hne

@[step]
theorem put_spec (stack : alloc.vec.Vec (alloc.vec.Vec U8)) (depth : Usize) (part : alloc.vec.Vec U8)
    (hd : depth.val ≤ stack.val.length) (hroom : stack.val.length < Usize.max) :
    folder.put stack depth part ⦃ r =>
      r.val.take (depth.val + 1) = stack.val.take depth.val ++ [part] ∧
        depth.val + 1 ≤ r.val.length ∧ r.val.length ≤ stack.val.length + 1 ⦄ := by
  unfold folder.put
  step*
  · have hlt : depth.val < stack.val.length := by scalar_tac
    simp only [__post2, alloc.vec.Vec.set_val_eq]
    refine ⟨?_, by simp; omega, by simp⟩
    rw [List.take_add_one, List.take_set_of_le (le_refl _), List.getElem?_set_self hlt]
    rfl
  · have : depth.val = stack.val.length := by scalar_tac
    rw [r_post, this]
    simp

/-- The walk as the pure model sees it. -/
def walkOf (w : folder.Walk) : Option (List (List Spec.Byte)) :=
  if w.ok then some (pb (w.stack.val.take w.depth.val)) else none

theorem pb_take_dropLast (l : List (alloc.vec.Vec U8)) (d : Nat) (hd : 0 < d) (hl : d ≤ l.length) :
    pb (l.take (d - 1)) = (pb (l.take d)).dropLast := by
  simp only [pb, ← List.map_dropLast]
  rw [List.dropLast_eq_take, List.length_take, List.take_take]
  congr 2
  omega

@[step]
theorem apply_part_spec (w : folder.Walk) (part : Slice U8) (hok : w.ok = true)
    (hd : w.depth.val ≤ w.stack.val.length) (hroom : w.stack.val.length < Usize.max) :
    folder.apply_part w part ⦃ w' =>
      walkOf w' = walkStep (walkOf w) (bytes part.val) ∧ w'.depth.val ≤ w'.stack.val.length ∧
        w'.stack.val.length ≤ w.stack.val.length + 1 ⦄ := by
  unfold folder.apply_part
  have hw : walkOf w = some (pb (w.stack.val.take w.depth.val)) := by simp [walkOf, hok]
  step*
  · have hdot : bytes part.val = ascii "." := b_post.mp (by assumption)
    refine ⟨?_, hd, by omega⟩
    rw [hw, hdot]
    simp [walkStep]
  · have hdot : bytes part.val ≠ ascii "." := fun h => (by assumption : ¬ b = true) (b_post.mpr h)
    have hdd : bytes part.val = ascii ".." := b1_post.mp (by assumption)
    have h0 : w.depth.val = 0 := by scalar_tac
    refine ⟨?_, by simp, by omega⟩
    rw [hw, h0]
    simp [walkOf, walkStep, hdd, dot_dot_ne_dot, pb]
  · have hdot : bytes part.val ≠ ascii "." := fun h => (by assumption : ¬ b = true) (b_post.mpr h)
    have hdd : bytes part.val = ascii ".." := b1_post.mp (by assumption)
    refine ⟨?_, by omega, by omega⟩
    have hne : pb (w.stack.val.take w.depth.val) ≠ [] := by
      intro h
      have := congrArg List.length h
      simp [pb] at this
      rcases this with h1 | h1
      · omega
      · have hl : w.stack.val.length = 0 := (congrArg List.length h1).trans rfl
        omega
    rw [hw]
    simp only [walkOf, hok, if_true, walkStep, hdd, if_neg dot_dot_ne_dot, if_neg hne]
    rw [i_post1, pb_take_dropLast _ _ i_post2 hd]
  · have hdot : bytes part.val ≠ ascii "." := fun h => (by assumption : ¬ b = true) (b_post.mpr h)
    have hdd : bytes part.val ≠ ascii ".." := fun h => (by assumption : ¬ b1 = true) (b1_post.mpr h)
    refine ⟨?_, by omega, v1_post3⟩
    rw [hw]
    simp only [walkOf, hok, if_true, walkStep, if_neg hdot, if_neg hdd]
    rw [i_post, v1_post1, pb_append]
    simp [pb, v_post]

def AllInv (parts : Slice (alloc.vec.Vec U8)) (w0 : folder.Walk) (st : folder.Walk × Usize) : Prop :=
  st.2.val ≤ parts.val.length ∧ st.1.depth.val ≤ st.1.stack.val.length ∧
    st.1.stack.val.length ≤ w0.stack.val.length + st.2.val ∧
    walkOf st.1 = walkAll (walkOf w0) (pb (parts.val.take st.2.val))

@[step]
theorem apply_all_spec (w0 : folder.Walk) (parts : Slice (alloc.vec.Vec U8))
    (hd : w0.depth.val ≤ w0.stack.val.length) (hroom : w0.stack.val.length + parts.val.length < Usize.max) :
    folder.apply_all w0 parts ⦃ w =>
      walkOf w = walkAll (walkOf w0) (pb parts.val) ∧ w.depth.val ≤ w.stack.val.length ∧
        w.stack.val.length ≤ w0.stack.val.length + parts.val.length ⦄ := by
  unfold folder.apply_all
  step with (show folder.apply_all_loop w0 parts 0#usize ⦃ r =>
      walkOf ⟨r.1, r.2.1, r.2.2⟩ = walkAll (walkOf w0) (pb parts.val) ∧ r.2.2.val ≤ r.2.1.val.length ∧
        r.2.1.val.length ≤ w0.stack.val.length + parts.val.length ⦄ from by
    unfold folder.apply_all_loop
    apply loop.spec_decr_nat (fun st => parts.val.length - st.2.val) (AllInv parts w0) _ _ _ _
      ⟨by simp, hd, by simp, by simp [pb, walkAll]⟩
    rintro ⟨w, i⟩ ⟨hi, hwd, hws, hwalk⟩
    simp only at hi hwd hws hwalk
    unfold folder.apply_all_loop.body
    step*
    · have hlt : i.val < parts.val.length := by scalar_tac
      unfold AllInv
      dsimp only
      refine ⟨⟨by omega, walk1_post2, by omega, ?_⟩, by omega⟩
      rw [walk1_post1, hwalk, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, pb_append]
      have hv : pb (some parts.val[i.val]).toList = [bytes v.val] := by simp [pb, v_post]
      rw [hv, walkAll_snoc]
      rfl
    · have hok : w.ok = true := by assumption
      have : i.val = parts.val.length := by scalar_tac
      rw [this, List.take_length] at hwalk
      refine ⟨?_, hwd, by omega⟩
      rw [← hwalk]
      simp [walkOf, hok]
    · have hok : ¬ w.ok = true := by assumption
      have hnone : walkAll (walkOf w0) (pb (parts.val.take i.val)) = none := by
        rw [← hwalk]; simp [walkOf, hok]
      refine ⟨?_, hwd, by omega⟩
      rw [← List.take_append_drop i.val parts.val, pb_append, walkAll_append, hnone, walkAll_none]
      simp [walkOf]) as ⟨b, v, d, h1, h2, h3⟩
  step*

/-- The walk before the request: empty for an absolute request, else the base. -/
def startWalk (base request : List Spec.Byte) : Option (List (List Spec.Byte)) :=
  if request.head? = some slash then some [] else walkAll (some []) (pathParts base)

@[step]
theorem start_spec (base request : Slice U8) (hb : base.val.length ≤ 2 ^ 20) :
    folder.start base request ⦃ w =>
      walkOf w = startWalk (bytes base.val) (bytes request.val) ∧ w.depth.val ≤ w.stack.val.length ∧
        w.stack.val.length ≤ base.val.length ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold folder.start
  have hw0 : walkOf { ok := true, stack := alloc.vec.Vec.new (alloc.vec.Vec U8), depth := 0#usize } = some [] := by
    simp [walkOf, pb]
  step*
  · have hlt : 0 < request.val.length := by scalar_tac
    have hh : (bytes request.val).head? = some slash := by
      obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos hlt
      have : i1 = x := by rw [i1_post]; simp [hx]
      subst this
      rw [hx]
      simp [bytes, slash_iff, UScalar.val_eq_of_eq (by assumption : i1 = folder.SLASH)]
    refine ⟨?_, by simp, by simp⟩
    rw [hw0, startWalk, if_pos hh]
  all_goals try (have hl : (alloc.vec.Vec.deref v).val.length = v.val.length := rfl; simp only [hl]; simp; omega)
  all_goals
    have hl : (alloc.vec.Vec.deref v).val.length = v.val.length := rfl
    refine ⟨?_, w_post2, by simp only [hl] at w_post3; simp at w_post3; omega⟩
    rw [w_post1, hw0, startWalk, if_neg ?_]
    · rw [← v_post1]; rfl
  · have hlt : 0 < request.val.length := by scalar_tac
    obtain ⟨x, xs, hx⟩ := List.exists_cons_of_length_pos hlt
    have : i1 = x := by rw [i1_post]; simp [hx]
    subst this
    rw [hx]
    simp only [bytes, List.map_cons, List.head?_cons, Option.some.injEq, slash_iff]
    intro h47
    exact (by assumption : ¬ i1 = folder.SLASH) (UScalar.eq_of_val_eq (by simp [h47]))
  · have h0 : request.val = [] := List.eq_nil_of_length_eq_zero (by scalar_tac)
    simp [h0, bytes]

def agree (root stack : List (alloc.vec.Vec U8)) (j : Nat) : Prop :=
  ∀ k (h1 : k < root.length) (h2 : k < stack.length), k < j → root[k].val = stack[k].val

theorem prefix_pb (root stack : List (alloc.vec.Vec U8)) (d : Nat) (hd : d ≤ stack.length) :
    pb root <+: pb (stack.take d) ↔ root.length ≤ d ∧ agree root stack root.length := by
  rw [List.prefix_iff_getElem]
  simp only [pb, List.length_map, List.length_take, List.getElem_map, List.getElem_take,
    Protocol.Seen.bytes_eq_iff, agree]
  constructor
  · rintro ⟨hl, h⟩
    exact ⟨by omega, fun k h1 _ _ => h k h1⟩
  · rintro ⟨hl, h⟩
    exact ⟨by omega, fun k h1 => h k h1 (by omega) h1⟩

def PrefixInv (root stack : Slice (alloc.vec.Vec U8)) (depth : Nat) (st : Bool × Usize) : Prop :=
  st.2.val ≤ root.val.length ∧
    (st.1 = true ↔ root.val.length ≤ depth ∧ agree root.val stack.val st.2.val)

@[step]
theorem is_prefix_spec (root stack : Slice (alloc.vec.Vec U8)) (depth : Usize)
    (hd : depth.val ≤ stack.val.length) :
    folder.is_prefix root stack depth ⦃ r =>
      (r = true ↔ pb root.val <+: pb (stack.val.take depth.val)) ⦄ := by
  unfold folder.is_prefix folder.is_prefix_loop
  rw [prefix_pb _ _ _ hd]
  apply loop.spec_decr_nat (fun st => root.val.length - st.2.val) (PrefixInv root stack depth.val) _ _ _ _
    ⟨by simp, by simp [agree]⟩
  rintro ⟨same, j⟩ ⟨hj, hsame⟩
  simp only at hj hsame
  unfold folder.is_prefix_loop.body
  step*
  · obtain ⟨hl, hag⟩ := hsame.mp (by assumption)
    have hjr : j.val < root.val.length := by scalar_tac
    have hjs : j.val < stack.val.length := by omega
    unfold PrefixInv
    dsimp only
    refine ⟨⟨by omega, ?_⟩, by omega⟩
    rw [same1_post]
    show v.val = v1.val ↔ _
    subst v_post v1_post
    constructor
    · intro h
      refine ⟨hl, fun k h1 h2 hk => ?_⟩
      by_cases hkj : k < j.val
      · exact hag k h1 h2 hkj
      · have : k = j.val := by omega
        subst this
        exact h
    · rintro ⟨_, h⟩
      exact h j.val hjr hjs (by omega)
  · simp only [Bool.false_eq_true, false_iff]
    rintro ⟨hl, hag⟩
    exact (by assumption : ¬ same = true) (hsame.mpr ⟨hl, fun k h1 h2 _ => hag k h1 h2 h1⟩)

def insideOne (stack : List (alloc.vec.Vec U8)) (depth : Nat) (root : alloc.vec.Vec U8) : Prop :=
  pathParts (bytes root.val) <+: pb (stack.take depth)

@[step]
theorem inside_any_spec (roots stack : Slice (alloc.vec.Vec U8)) (depth : Usize)
    (hd : depth.val ≤ stack.val.length) :
    folder.inside_any roots stack depth ⦃ r =>
      (r = true ↔ ∃ root ∈ roots.val, insideOne stack.val depth.val root) ⦄ := by
  unfold folder.inside_any folder.inside_any_loop
  apply loop.spec_decr_nat (fun st => roots.val.length - st.2.val)
    (fun st => st.2.val ≤ roots.val.length ∧
      (st.1 = true ↔ ∃ root ∈ roots.val.take st.2.val, insideOne stack.val depth.val root)) _ _ _ _
    ⟨by simp, by simp⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold folder.inside_any_loop.body
  step*
  · -- Found in an earlier root.
    simp only [true_iff]
    obtain ⟨root, hr, h⟩ := hfound.mp (by assumption)
    exact ⟨root, List.mem_of_mem_take hr, h⟩
  · have hlt : i.val < roots.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [found1_post, i2_post]
    have hnot : ¬ ∃ root ∈ roots.val.take i.val, insideOne stack.val depth.val root := by
      rw [← hfound]; assumption
    have hv1 : pb v1.val = pathParts (bytes roots.val[i.val]) := by rw [v1_post1, v_post]; rfl
    have htake : roots.val.take (i.val + 1) = roots.val.take i.val ++ [roots.val[i.val]] := by
      rw [List.take_add_one, List.getElem?_eq_getElem hlt]; rfl
    rw [htake]
    constructor
    · intro h
      exact ⟨roots.val[i.val], List.mem_append_right _ (List.mem_singleton_self _), by unfold insideOne; rw [← hv1]; exact h⟩
    · rintro ⟨root, hr, h⟩
      rcases List.mem_append.mp hr with hr | hr
      · exact absurd ⟨root, hr, h⟩ hnot
      · rw [List.mem_singleton] at hr
        subst hr
        unfold insideOne at h
        rw [← hv1] at h
        exact h
  · have : i.val = roots.val.length := by scalar_tac
    rw [this, List.take_length] at hfound
    rw [← hfound]
    simp_all

theorem joinParts_take_le (l : List (alloc.vec.Vec U8)) (a d : Nat) (h : a ≤ d) :
    (joinParts (pb (l.take a))).length ≤ (joinParts (pb (l.take d))).length := by
  have : l.take a = (l.take d).take a := by rw [List.take_take, min_eq_left h]
  rw [this, ← List.take_append_drop a (l.take d)]
  simp only [List.take_append_drop]
  conv => rhs; rw [← List.take_append_drop a (l.take d)]
  rw [pb_append, joinParts_append, List.length_append]
  omega

theorem pb_take_succ (l : List (alloc.vec.Vec U8)) (j : Nat) (h : j < l.length) :
    pb (l.take (j + 1)) = pb (l.take j) ++ [bytes l[j].val] := by
  rw [List.take_add_one, List.getElem?_eq_getElem h, pb_append]
  rfl

theorem join_step (l : List (alloc.vec.Vec U8)) (j : Nat) (h : j < l.length) :
    joinParts (pb (l.take (j + 1))) = joinParts (pb (l.take j)) ++ slash :: bytes l[j].val := by
  rw [pb_take_succ _ _ h, joinParts_append]
  simp [joinParts]

theorem join_step_len (l : List (alloc.vec.Vec U8)) (j d : Nat) (hd : d ≤ l.length) (hj : j + 1 ≤ d) :
    (joinParts (pb (l.take j))).length + 1 + (l[j]'(by omega)).val.length ≤ (joinParts (pb (l.take d))).length := by
  have h1 := joinParts_take_le l (j + 1) d hj
  rw [join_step l j (by omega)] at h1
  simp [bytes] at h1
  omega

@[step]
theorem join_spec (stack : Slice (alloc.vec.Vec U8)) (depth : Usize) (hd : depth.val ≤ stack.val.length)
    (hroom : (joinParts (pb (stack.val.take depth.val))).length ≤ 2 ^ 31) :
    folder.join stack depth ⦃ r => bytes r.val = joinParts (pb (stack.val.take depth.val)) ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold folder.join folder.join_loop
  apply loop.spec_decr_nat (fun st => depth.val - st.2.val)
    (fun st => st.2.val ≤ depth.val ∧ bytes st.1.val = joinParts (pb (stack.val.take st.2.val))) _ _ _ _
    ⟨by simp, by simp [bytes, pb, joinParts]⟩
  rintro ⟨out, j⟩ ⟨hj, hout⟩
  simp only at hj hout
  unfold folder.join_loop.body
  have hol : out.val.length = (joinParts (pb (stack.val.take j.val))).length := by
    have := congrArg List.length hout; simpa [bytes] using this
  step*
  · have := join_step_len stack.val j.val depth.val hd (by scalar_tac)
    omega
  · have := join_step_len stack.val j.val depth.val hd (by scalar_tac)
    have hlt : j.val < stack.val.length := by scalar_tac
    subst v_post
    simp only [out1_post, List.length_append, List.length_singleton]
    exact (by omega : out.val.length + 1 + (stack.val[j.val]'hlt).val.length ≤ Usize.max)
  · have hlt : j.val < stack.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [j1_post, join_step _ _ hlt, ← hout, out2_post1, out1_post, v_post]
    simp only [bytes, List.map_append, List.map_cons, List.append_assoc, List.cons_append,
      List.nil_append]
    rw [(slash_iff folder.SLASH).mpr slash_val]
    rfl

theorem startWalk_length (b q : List Spec.Byte) (s0 : List (List Spec.Byte)) (h : startWalk b q = some s0) :
    (joinParts s0).length ≤ b.length + 1 := by
  unfold startWalk at h
  split at h
  · simp at h; subst h; simp [joinParts]
  · have := walkAll_length [] s0 _ h
    have := joinParts_pathParts_length b
    simp [joinParts] at *
    omega

/-- The pure result of a request. -/
def resolved (base request : List Spec.Byte) : Option (List (List Spec.Byte)) :=
  walkAll (startWalk base request) (pathParts request)

theorem resolve_folder_spec (roots : Slice (alloc.vec.Vec U8)) (base request : Slice U8)
    (hlen : base.val.length + request.val.length ≤ 2 ^ 20) :
    folder.resolve_folder roots base request ⦃ r => match r with
      | some p => ∃ st, resolved (bytes base.val) (bytes request.val) = some st ∧ st ≠ [] ∧
          (∃ root ∈ roots.val, pathParts (bytes root.val) <+: st) ∧ bytes p.val = joinParts st
      | none => ∀ st, resolved (bytes base.val) (bytes request.val) = some st → st ≠ [] →
          ¬ ∃ root ∈ roots.val, pathParts (bytes root.val) <+: st ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold folder.resolve_folder
  step*
  · have : (alloc.vec.Vec.deref v).val.length = v.val.length := rfl
    omega
  all_goals
    have hv : pb (alloc.vec.Vec.deref v).val = pathParts (bytes request.val) := v_post1
    have hres : resolved (bytes base.val) (bytes request.val) = walkOf walk := by
      rw [walk_post1, w_post1, hv]; rfl
  · -- Depth 0 is `/`, never a clean path.
    have hok : walk.ok = true := by assumption
    have h0 : walk.depth.val = 0 := by scalar_tac
    intro st hst hne
    rw [hres] at hst
    simp [walkOf, hok, h0, pb] at hst
    exact absurd hst hne
  · exact walk_post2
  · exact walk_post2
  · -- The joined folder is at most as long as the base and the request.
    have hok : walk.ok = true := by assumption
    have hsome : resolved (bytes base.val) (bytes request.val) =
        some (pb (walk.stack.val.take walk.depth.val)) := by rw [hres]; simp [walkOf, hok]
    unfold resolved at hsome
    cases hs : startWalk (bytes base.val) (bytes request.val) with
    | none => rw [hs, walkAll_none] at hsome; exact absurd hsome (by simp)
    | some s0 =>
      rw [hs] at hsome
      have h1 := walkAll_length _ _ _ hsome
      have h2 := startWalk_length _ _ _ hs
      have h3 := joinParts_pathParts_length (bytes request.val)
      have hb1 : (bytes base.val).length = base.val.length := by simp [bytes]
      have hb2 : (bytes request.val).length = request.val.length := by simp [bytes]
      exact (by omega : (joinParts (pb (walk.stack.val.take walk.depth.val))).length ≤ 2 ^ 31)
  · have hok : walk.ok = true := by assumption
    have hne0 : walk.depth.val ≠ 0 := by scalar_tac
    obtain ⟨root, hr, hin⟩ := b_post.mp (by assumption)
    refine ⟨pb (walk.stack.val.take walk.depth.val), by rw [hres]; simp [walkOf, hok], ?_, ⟨root, hr, hin⟩,
      v1_post⟩
    intro h
    have := congrArg List.length h
    simp [pb] at this
    rcases this with h1 | h1
    · exact hne0 h1
    · have hl : walk.stack.val.length = 0 := (congrArg List.length h1).trans rfl
      omega
  · have hok : walk.ok = true := by assumption
    have hb : ¬ b = true := by assumption
    intro st hst _ hin
    rw [hres] at hst
    simp only [walkOf, hok, if_true, Option.some.injEq] at hst
    subst hst
    exact hb (b_post.mpr hin)
  · have hok : ¬ walk.ok = true := by assumption
    intro st hst
    rw [hres] at hst
    simp [walkOf, hok] at hst

theorem resolved_clean (b q : List Spec.Byte) (st : List (List Spec.Byte)) (h : resolved b q = some st) :
    ∀ p ∈ st, cleanPart p := by
  unfold resolved at h
  cases hs : startWalk b q with
  | none => rw [hs, walkAll_none] at h; exact absurd h (by simp)
  | some s0 =>
    rw [hs] at h
    have h0 : ∀ p ∈ s0, cleanPart p := by
      unfold startWalk at hs
      split at hs
      · simp at hs; subst hs; simp
      · exact walkAll_parts_clean [] s0 _ (by simp) (pathParts_good b) hs
    exact walkAll_parts_clean s0 st _ h0 (pathParts_good q) h

/-- **S5.** -/
theorem folder_sound (roots : Slice (alloc.vec.Vec U8)) (base request : Slice U8)
    (hlen : base.val.length + request.val.length ≤ 2 ^ 20) :
    folder.resolve_folder roots base request ⦃ r => match r with
      | some p => cleanPath (bytes p.val) ∧ ∃ root ∈ roots.val, insideRoot (bytes root.val) (bytes p.val)
      | none => True ⦄ := by
  apply WP.spec_mono (resolve_folder_spec roots base request hlen)
  intro r hr
  cases r with
  | none => trivial
  | some p =>
    obtain ⟨st, hst, hne, ⟨root, hr, hin⟩, hp⟩ := hr
    have hclean := resolved_clean _ _ _ hst
    show cleanPath (bytes p.val) ∧ ∃ root ∈ roots.val, insideRoot (bytes root.val) (bytes p.val)
    rw [hp]
    refine ⟨joinParts_clean st hne hclean, root, hr, ?_⟩
    unfold insideRoot
    rw [pathParts_joinParts st (fun q hq => (hclean q hq).1)]
    exact hin

theorem pathParts_relative (q : List Spec.Byte) (h : cleanRelative q) : pathParts q = q.splitOn slash := by
  unfold pathParts
  rw [List.filter_eq_self]
  intro a ha
  simpa using (h.2.2 a ha).1

theorem joinParts_relative (q : List Spec.Byte) (h : cleanRelative q) : joinParts (pathParts q) = slash :: q := by
  rw [pathParts_relative q h, joinParts_eq_intercalate _ (List.splitOn_ne_nil _ _), List.intercalate_splitOn]

/-- **S5, usefulness.** -/
theorem folder_complete (roots : Slice (alloc.vec.Vec U8)) (base request : Slice U8) (root : alloc.vec.Vec U8)
    (hlen : base.val.length + request.val.length ≤ 2 ^ 20)
    (hroot : root ∈ roots.val) (hbase : cleanPath (bytes base.val))
    (hin : insideRoot (bytes root.val) (bytes base.val)) (hreq : cleanRelative (bytes request.val)) :
    folder.resolve_folder roots base request ⦃ r => ∃ p, r = some p ∧
      bytes p.val = bytes base.val ++ [slash] ++ bytes request.val ⦄ := by
  apply WP.spec_mono (resolve_folder_spec roots base request hlen)
  obtain ⟨hbj, hbne, hbparts⟩ := hbase
  have hPR : ∀ p ∈ pathParts (bytes request.val), p ≠ ascii "." ∧ p ≠ ascii ".." := by
    intro p hp
    rw [pathParts_relative _ hreq] at hp
    exact ⟨(hreq.2.2 p hp).2.1, (hreq.2.2 p hp).2.2⟩
  have hres : resolved (bytes base.val) (bytes request.val) = some (pathParts (bytes base.val) ++ pathParts (bytes request.val)) := by
    unfold resolved startWalk
    rw [if_neg hreq.2.1, walkAll_clean [] _ hbparts, List.nil_append, walkAll_clean _ _ hPR]
  have hne : pathParts (bytes base.val) ++ pathParts (bytes request.val) ≠ [] := by simp [hbne]
  have hinside : ∃ root ∈ roots.val, pathParts (bytes root.val) <+: pathParts (bytes base.val) ++ pathParts (bytes request.val) :=
    ⟨root, hroot, hin.trans (List.prefix_append _ _)⟩
  intro r hr
  cases r with
  | none => exact absurd hinside (hr _ hres hne)
  | some p =>
    obtain ⟨st, hst, _, _, hp⟩ := hr
    rw [hres, Option.some.injEq] at hst
    subst hst
    refine ⟨p, rfl, ?_⟩
    rw [hp, joinParts_append, joinParts_relative _ hreq]
    have : joinParts (pathParts (bytes base.val)) = bytes base.val := hbj.symm
    rw [this]
    simp

end Protocol.Folder