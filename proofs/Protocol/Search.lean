import Protocol.Ascii
import Protocol.Seen

/-! # Byte search for the action classifier -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Search

theorem take_succ_iff {α : Type} (l1 l2 : List α) (k : Nat) (h1 : k < l1.length) (h2 : k < l2.length) :
    l1.take (k + 1) = l2.take (k + 1) ↔ l1.take k = l2.take k ∧ l1[k] = l2[k] := by
  rw [List.take_add_one, List.take_add_one, List.getElem?_eq_getElem h1, List.getElem?_eq_getElem h2]
  constructor
  · intro h
    have hl : (l1.take k).length = (l2.take k).length := by simp; omega
    obtain ⟨ha, hb⟩ := List.append_inj h hl
    exact ⟨ha, by simpa using hb⟩
  · rintro ⟨ha, hb⟩
    rw [ha, hb]

theorem take_mono_eq {α : Type} (l1 l2 : List α) (k n : Nat) (hk : k ≤ n) (h : l1.take n = l2.take n) :
    l1.take k = l2.take k := by
  have e1 : l1.take k = (l1.take n).take k := by rw [List.take_take, Nat.min_eq_left hk]
  have e2 : l2.take k = (l2.take n).take k := by rw [List.take_take, Nat.min_eq_left hk]
  rw [e1, e2, h]

@[step]
theorem equal_run_spec (a : Slice U8) (start : Usize) (b : Slice U8) (n : Usize)
    (h1 : start.val + n.val ≤ a.val.length) (h2 : n.val ≤ b.val.length) :
    search.equal_run a start b n ⦃ r =>
      (r = true ↔ (a.val.drop start.val).take n.val = b.val.take n.val) ⦄ := by
  unfold search.equal_run search.equal_run_loop
  apply loop.spec_decr_nat (fun st => n.val - st.2.val)
    (fun st => st.2.val ≤ n.val ∧
      (st.1 = true ↔ (a.val.drop start.val).take st.2.val = b.val.take st.2.val)) _ _ _ _
    ⟨by simp, by simp⟩
  rintro ⟨same, k⟩ ⟨hk, hsame⟩
  simp only at hk hsame
  unfold search.equal_run_loop.body
  step*
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hs : same = true := by assumption
    have hlt : k.val < n.val := by scalar_tac
    rw [k1_post, take_succ_iff _ _ _ (by simp; omega) (by omega), ← hsame, decide_eq_true_iff]
    simp only [hs, true_and, List.getElem_drop]
    simp only [i1_post, i2_post, i_post]
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have hs : ¬ same = true := by assumption
    exact hs (hsame.mpr (take_mono_eq _ _ _ _ hk h))

theorem infix_iff_take_drop (hay needle : List U8) :
    needle <:+: hay ↔ ∃ i, i + needle.length ≤ hay.length ∧ (hay.drop i).take needle.length = needle := by
  constructor
  · rintro ⟨s, t, rfl⟩
    refine ⟨s.length, by simp, ?_⟩
    rw [List.append_assoc, List.drop_left, List.take_left]
  · rintro ⟨i, hi, h⟩
    refine ⟨hay.take i, hay.drop (i + needle.length), ?_⟩
    conv => rhs; rw [← List.take_append_drop i hay]
    rw [List.append_assoc]
    congr 1
    conv => rhs; rw [← List.take_append_drop needle.length (hay.drop i)]
    rw [h, List.drop_drop, Nat.add_comm]

def Found (hay needle : List U8) (k : Nat) : Prop :=
  ∃ i < k, i + needle.length ≤ hay.length ∧ (hay.drop i).take needle.length = needle

@[step]
theorem contains_loop_spec (hay needle : Slice U8) (last : Usize)
    (hlast : last.val + needle.val.length = hay.val.length) (hroom : hay.val.length < Usize.max) :
    search.contains_loop hay needle last false 0#usize ⦃ r =>
      (r = true ↔ Found hay.val needle.val (last.val + 1)) ⦄ := by
  unfold search.contains_loop
  apply loop.spec_decr_nat (fun st => last.val + 1 - st.2.val)
    (fun st => st.2.val ≤ last.val + 1 ∧ (st.1 = true ↔ Found hay.val needle.val st.2.val)) _ _ _ _
    ⟨by simp, by simp [Found]⟩
  rintro ⟨found, pos⟩ ⟨hpos, hfound⟩
  simp only at hpos hfound
  unfold search.contains_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨i, hi, h⟩ := hfound.mp (by assumption)
    exact ⟨i, by omega, h⟩
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hnot : ¬ Found hay.val needle.val pos.val := by rw [← hfound]; assumption
    have hn : needle.len.val = needle.val.length := by simp
    rw [found1_post, at1_post, hn, List.take_length]
    constructor
    · intro h
      exact ⟨pos.val, by omega, by scalar_tac, h⟩
    · rintro ⟨i, hi, hl, h⟩
      by_cases hip : i < pos.val
      · exact absurd ⟨i, hip, hl, h⟩ hnot
      · have : i = pos.val := by omega
        subst this
        rw [h]

@[step]
theorem contains_spec (hay needle : Slice U8) (hroom : hay.val.length < Usize.max) :
    search.contains hay needle ⦃ r => (r = true ↔ needle.val <:+: hay.val) ⦄ := by
  unfold search.contains
  rw [infix_iff_take_drop]
  step*
  rw [r_post]
  constructor
  · rintro ⟨i, hi, hl, h⟩
    exact ⟨i, hl, h⟩
  · rintro ⟨i, hl, h⟩
    exact ⟨i, by scalar_tac, hl, h⟩

theorem infix_bytes_iff (a b : List U8) : bytes a <:+: bytes b ↔ a <:+: b := by
  constructor
  · intro h
    obtain ⟨l, hl, he⟩ := List.infix_map_iff.mp h
    have : a = l := (Protocol.Seen.bytes_eq_iff a l).mp he
    rw [this]; exact hl
  · exact fun h => h.map _

@[simp, scalar_tac_simps]
theorem space_val : search.SPACE.val = 32 := by unfold search.SPACE; rfl

/-- The name with a space before and after it is in the list. -/
@[step]
theorem listed_spec (names n : Slice U8) (hroom : names.val.length < Usize.max) :
    search.listed names n ⦃ r => (r = true ↔ (search.SPACE :: n.val ++ [search.SPACE]) <:+: names.val) ⦄ := by
  unfold search.listed
  step*
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have := h.length_le
    simp at this
    scalar_tac
  rw [r_post]
  simp only [alloc.vec.Vec.deref, needle2_post, needle1_post1, needle_post]
  simp

theorem listed_bytes (names n : List U8) :
    (search.SPACE :: n ++ [search.SPACE]) <:+: names ↔ (ch ' ' :: bytes n ++ [ch ' ']) <:+: bytes names := by
  have hs : search.SPACE.bv = ch ' ' := by
    apply BitVec.eq_of_toNat_eq; simp [ch, space_val]
  have : ch ' ' :: bytes n ++ [ch ' '] = bytes (search.SPACE :: n ++ [search.SPACE]) := by
    simp [bytes, hs]
  rw [this]
  exact (infix_bytes_iff _ _).symm

@[step]
theorem has_byte_spec (s : Slice U8) (b : U8) :
    search.has_byte s b ⦃ r => (r = true ↔ b ∈ s.val) ⦄ := by
  unfold search.has_byte search.has_byte_loop
  apply loop.spec_decr_nat (fun st => s.val.length - st.2.val)
    (fun st => st.2.val ≤ s.val.length ∧ (st.1 = true ↔ b ∈ s.val.take st.2.val)) _ _ _ _
    ⟨by simp, by simp⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold search.has_byte_loop.body
  step*
  · simp only [true_iff]
    exact List.mem_of_mem_take (hfound.mp (by assumption))
  · have hlt : i.val < s.val.length := by scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt, decide_eq_true_iff]
    have hnot : b ∉ s.val.take i.val := by rw [← hfound]; assumption
    simp only [Option.toList_some, List.mem_append, List.mem_singleton, hnot, false_or, i2_post]
    exact eq_comm
  · have : i.val = s.val.length := by scalar_tac
    rw [this, List.take_length] at hfound
    rw [← hfound]
    simp_all

end Protocol.Search
