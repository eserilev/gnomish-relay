import Protocol.PathRules
import Protocol.Spec.Hosts

/-! # Host names and allow lists (S33) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.PathRules

namespace Protocol.Hosts

/-- The bytes of `host[start..end]`. -/
def seg (host : List U8) (start stop : Nat) : List Spec.Byte := bytes ((host.drop start).take (stop - start))

theorem toNat_bv (b : U8) : b.bv.toNat = b.val := by simp

@[step]
theorem is_letter_spec (b : U8) : hosts.is_letter b ⦃ r => (r = true ↔ isLetter b.bv) ⦄ := by
  unfold hosts.is_letter isLetter
  rw [toNat_bv]
  step*

@[step]
theorem is_label_byte_spec (b : U8) :
    hosts.is_label_byte b ⦃ r => (r = true ↔ isLabelByte b.bv) ⦄ := by
  unfold hosts.is_label_byte isLabelByte
  rw [isDigit_iff]
  have hd : (b = hosts.DASH ↔ b.bv = ch '-') := by
    unfold hosts.DASH; rw [UScalar.eq_equiv_bv_eq]; rfl
  rw [toNat_bv]
  step*

theorem seg_eq_map (host : List U8) (start stop : Nat) :
    seg host start stop = ((bytes host).drop start).take (stop - start) := by
  simp [seg, bytes, List.map_drop, List.map_take]

theorem seg_length (host : List U8) (start stop : Nat) (h2 : stop ≤ host.length) :
    (seg host start stop).length = stop - start := by
  simp [seg, bytes]; omega

theorem seg_empty (host : List U8) (start stop : Nat) (h : stop ≤ start) : seg host start stop = [] := by
  simp [seg, bytes, show stop - start = 0 by omega]

theorem seg_succ (host : List U8) (start i : Nat) (h1 : start ≤ i) (h2 : i < host.length) :
    seg host start (i + 1) = seg host start i ++ [(host[i]).bv] := by
  simp only [seg, show i + 1 - start = (i - start) + 1 by omega, List.take_add_one, bytes,
    List.map_append]
  rw [List.getElem?_drop, List.getElem?_eq_getElem (by omega : start + (i - start) < host.length)]
  simp [show start + (i - start) = i by omega]

theorem seg_head (host : List U8) (start stop : Nat) (h1 : start < stop) (h2 : stop ≤ host.length) :
    (seg host start stop).head? = some (host[start]'(by omega)).bv := by
  simp only [seg, bytes, List.head?_map, List.head?_take, List.head?_drop]
  rw [if_neg (by omega), List.getElem?_eq_getElem (by omega)]
  rfl

theorem seg_last (host : List U8) (start stop : Nat) (h1 : start < stop) (h2 : stop ≤ host.length) :
    (seg host start stop).getLast? = some (host[stop - 1]'(by omega)).bv := by
  have := seg_succ host start (stop - 1) (by omega) (by omega)
  rw [show stop - 1 + 1 = stop by omega] at this
  rw [this, List.getLast?_append]
  simp

@[step]
theorem all_label_bytes_spec (host : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ host.val.length) :
    hosts.all_label_bytes host start stop ⦃ r =>
      (r = true ↔ ∀ b ∈ seg host.val start.val stop.val, isLabelByte b) ⦄ := by
  unfold hosts.all_label_bytes hosts.all_label_bytes_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => start.val ≤ st.2.val ∧ st.2.val ≤ stop.val ∧
      (st.1 = true ↔ ∀ b ∈ seg host.val start.val st.2.val, isLabelByte b))
    _ _ _ _ ⟨le_refl _, hs, by simp [seg_empty]⟩
  rintro ⟨ok1, i⟩ ⟨hi1, hi2, hok⟩
  simp only at hi1 hi2 hok
  unfold hosts.all_label_bytes_loop.body
  step*
  · refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
    have hprev := hok.mp (by assumption)
    rw [ok2_post, i2_post, seg_succ _ _ _ hi1 (by scalar_tac), i1_post]
    simp only [List.mem_append, List.mem_singleton]
    constructor
    · rintro h b (hb | rfl)
      · exact hprev b hb
      · exact h
    · intro h; exact h _ (Or.inr rfl)
  · simp only [Bool.false_eq_true, false_iff]
    intro h
    have : ¬ ok1 = true := by assumption
    apply this; rw [hok]
    intro b hb
    apply h b
    have ht := seg_eq_map host.val start.val i.val
    have hs2 := seg_eq_map host.val start.val stop.val
    rw [ht] at hb; rw [hs2]
    exact List.take_subset_take_left _ (by omega) hb

theorem dash_iff (b : U8) : (b = hosts.DASH ↔ b.bv = ch '-') := by
  unfold hosts.DASH; rw [UScalar.eq_equiv_bv_eq]; rfl

@[simp, scalar_tac_simps] theorem max_label_val : hosts.MAX_LABEL.val = 63 := by unfold hosts.MAX_LABEL; rfl

@[step]
theorem label_ok_spec (host : Slice U8) (start stop : Usize) (hstop : stop.val ≤ host.val.length) :
    hosts.label_ok host start stop ⦃ r => (r = true ↔ goodLabel (seg host.val start.val stop.val)) ⦄ := by
  unfold hosts.label_ok goodLabel
  step*
  · simp [seg_empty _ _ _ (by scalar_tac : stop.val ≤ start.val)]
  · simp only [Bool.false_eq_true, false_iff, not_and]
    intro _ h; rw [seg_length _ _ _ hstop] at h; scalar_tac
  · have hlt : start.val < stop.val := by scalar_tac
    rw [r_post, seg_length _ _ _ hstop, seg_head _ _ _ hlt hstop, seg_last _ _ _ hlt hstop]
    have h1 : ¬ (host.val[start.val]).bv = ch '-' := by
      rw [← dash_iff, ← i1_post]; simpa using ‹(i1 != hosts.DASH) = true›
    have h2 : ¬ (host.val[stop.val - 1]).bv = ch '-' := by
      have : i3 = host.val[stop.val - 1] := by rw [i3_post]; simp [i2_post1]
      rw [← dash_iff, ← this]; simpa using ‹(i3 != hosts.DASH) = true›
    simp only [ne_eq, Option.some.injEq, h1, h2, not_false_eq_true, true_and]
    constructor
    · intro h; exact ⟨by scalar_tac, by scalar_tac, h⟩
    · intro h; exact h.2.2
  · simp only [Bool.false_eq_true, false_iff]
    rintro ⟨_, _, _, hl, _⟩
    have hlt : start.val < stop.val := by scalar_tac
    rw [seg_last _ _ _ hlt hstop] at hl
    have : i3 = host.val[stop.val - 1] := by rw [i3_post]; simp [i2_post1]
    rw [← this] at hl
    apply hl; congr 1
    rw [← dash_iff]
    have h3 := ‹¬(i3 != hosts.DASH) = true›
    simp at h3
    exact UScalar.eq_of_val_eq h3
  · simp only [Bool.false_eq_true, false_iff]
    rintro ⟨_, _, hh, _, _⟩
    have hlt : start.val < stop.val := by scalar_tac
    rw [seg_head _ _ _ hlt hstop, ← i1_post] at hh
    apply hh; congr 1
    rw [← dash_iff]
    have h3 := ‹¬(i1 != hosts.DASH) = true›
    simp at h3
    exact UScalar.eq_of_val_eq h3

theorem dot_iff (b : U8) : (b = hosts.DOT ↔ b.bv = ch '.') := by
  unfold hosts.DOT; rw [UScalar.eq_equiv_bv_eq]; rfl

@[step]
theorem next_dot_spec (host : Slice U8) (start : Usize) (hs : start.val ≤ host.val.length) :
    hosts.next_dot host start ⦃ r => start.val ≤ r.val ∧ r.val ≤ host.val.length ∧
      (∀ b ∈ seg host.val start.val r.val, b ≠ ch '.') ∧
      (∀ h : r.val < host.val.length, (host.val[r.val]).bv = ch '.') ⦄ := by
  unfold hosts.next_dot hosts.next_dot_loop
  apply loop.spec_decr_nat (fun i => host.val.length - i.val)
    (fun i => start.val ≤ i.val ∧ i.val ≤ host.val.length ∧ ∀ b ∈ seg host.val start.val i.val, b ≠ ch '.')
    _ _ _ _ ⟨le_refl _, hs, by simp [seg_empty]⟩
  rintro i ⟨hi1, hi2, hnd⟩
  unfold hosts.next_dot_loop.body
  step*
  · refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
    rw [i3_post, seg_succ _ _ _ hi1 (by scalar_tac)]
    simp only [List.mem_append, List.mem_singleton]
    rintro b (hb | rfl)
    · exact hnd b hb
    · intro h
      rw [← i2_post, ← dot_iff] at h
      have := ‹(i2 != hosts.DOT) = true›
      simp [h] at this
  · refine ⟨hi1, hi2, hnd, fun _ => ?_⟩
    rw [← dot_iff, ← i2_post]
    have h3 := ‹¬(i2 != hosts.DOT) = true›
    simp at h3
    exact UScalar.eq_of_val_eq h3

@[step]
theorem lower_range_spec (host : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ host.val.length) :
    hosts.lower_range host start stop ⦃ v => bytes v.val = lower (seg host.val start.val stop.val) ⦄ := by
  unfold hosts.lower_range
  step*
  rw [v_post1]
  simp [seg, label_post]

theorem localhost_bytes : bytes (Array.to_slice hosts.LOCALHOST).val = ascii "localhost" := by
  unfold hosts.LOCALHOST; decide

@[step]
theorem last_label_ok_spec (host : Slice U8) (start stop : Usize) (hs : start.val < stop.val)
    (hstop : stop.val ≤ host.val.length) :
    hosts.last_label_ok host start stop ⦃ r => (r = true ↔ goodLastLabel (seg host.val start.val stop.val)) ⦄ := by
  unfold hosts.last_label_ok goodLastLabel
  rw [seg_head _ _ _ hs hstop]
  step*
  · have hl : isLetter (host.val[start.val]).bv := by rw [← i_post]; exact b_post.mp ‹_›
    have hs1 : bytes s1.val = ascii "localhost" := by rw [s1_post]; exact localhost_bytes
    have heq : b1 = true ↔ lower (seg host.val start.val stop.val) = ascii "localhost" := by
      rw [b1_post, ← v_post, ← hs1, Protocol.Seen.bytes_eq_iff]; simp
    simp only [decide_eq_true_eq, heq, Option.some.injEq, exists_eq_left', hl, true_and, lowerAscii]
  · simp only [Bool.false_eq_true, false_iff, not_and, Option.some.injEq, exists_eq_left']
    intro hl
    exact absurd (b_post.mpr (by rw [i_post]; exact hl)) ‹_›

/-! ## The labels of a name -/

/-- Every label is good, and the last one is a good last label. -/
def labelsGood (rest : List Spec.Byte) : Prop :=
  (∀ l ∈ rest.splitOn (ch '.'), goodLabel l) ∧ goodLastLabel ((rest.splitOn (ch '.')).getLast (List.splitOn_ne_nil _ _))

theorem labelsGood_single (xs : List Spec.Byte) (h : ch '.' ∉ xs) :
    labelsGood xs ↔ goodLabel xs ∧ goodLastLabel xs := by
  unfold labelsGood
  simp only [List.splitOn_eq_singleton h, List.mem_singleton, forall_eq, List.getLast_singleton]

theorem labelsGood_cons (xs rest : List Spec.Byte) (h : ch '.' ∉ xs) :
    labelsGood (xs ++ ch '.' :: rest) ↔ goodLabel xs ∧ labelsGood rest := by
  unfold labelsGood
  simp only [List.splitOn_append_cons_self_of_not_mem h, List.mem_cons, forall_eq_or_imp,
    List.getLast_cons (List.splitOn_ne_nil _ _)]
  tauto

theorem drop_last (host : List U8) (start stop : Nat) (_h1 : start ≤ stop) (h2 : stop = host.length) :
    bytes (host.drop start) = seg host start stop := by
  simp [seg, bytes, h2]

theorem drop_mid (host : List U8) (start stop : Nat) (h1 : start ≤ stop) (h2 : stop < host.length)
    (hd : (host[stop]).bv = ch '.') :
    bytes (host.drop start) = seg host start stop ++ ch '.' :: bytes (host.drop (stop + 1)) := by
  have e : host.drop start = (host.drop start).take (stop - start) ++ host[stop] :: host.drop (stop + 1) := by
    conv => lhs; rw [← List.take_append_drop (stop - start) (host.drop start)]
    congr 1
    rw [List.drop_drop, show start + (stop - start) = stop by omega]
    exact List.drop_eq_getElem_cons h2
  rw [e]
  simp only [seg, bytes, List.map_append, List.map_cons, hd]

theorem not_dot_of (xs : List Spec.Byte) (h : ∀ b ∈ xs, b ≠ ch '.') : ch '.' ∉ xs := fun hm => h _ hm rfl

theorem labels_ok_spec (host : Slice U8) (start : Usize) (hs : start.val ≤ host.val.length) :
    hosts.labels_ok host start ⦃ r => (r = true ↔ labelsGood (bytes (host.val.drop start.val))) ⦄ := by
  rw [hosts.labels_ok]
  step as ⟨stop, h1, h2, hnd, hdot⟩
  have hnd' := not_dot_of _ hnd
  step as ⟨b, hb⟩
  split
  · rename_i hbt
    have hgood := hb.mp hbt
    have hlt : start.val < stop.val := by
      have := hgood.1; rw [seg_length _ _ _ h2] at this; omega
    split
    · rename_i heq
      have hstop : stop.val = host.val.length := by scalar_tac
      step as ⟨r, hr⟩
      rw [hr, drop_last _ _ _ h1 hstop, labelsGood_single _ hnd']
      exact ⟨fun h => ⟨hgood, h⟩, fun h => h.2⟩
    · rename_i hne
      have hstop : stop.val < host.val.length := by scalar_tac
      step as ⟨i1, hi1⟩
      have hrec := labels_ok_spec host i1 (by scalar_tac)
      apply WP.spec_mono hrec
      intro r hr
      rw [hr, drop_mid _ _ _ h1 hstop (hdot hstop), labelsGood_cons _ _ hnd', hi1]
      exact ⟨fun h => ⟨hgood, h⟩, fun h => h.2⟩
  · rename_i hbf
    simp only [WP.spec_ok, Bool.false_eq_true, false_iff]
    intro hl
    apply hbf
    apply hb.mpr
    by_cases hstop : stop.val = host.val.length
    · rw [drop_last _ _ _ h1 hstop, labelsGood_single _ hnd'] at hl
      exact hl.1
    · have hstop : stop.val < host.val.length := by omega
      rw [drop_mid _ _ _ h1 hstop (hdot hstop), labelsGood_cons _ _ hnd'] at hl
      exact hl.1
termination_by host.val.length - start.val
decreasing_by scalar_tac

theorem two_labels (l : List Spec.Byte) : 2 ≤ (l.splitOn (ch '.')).length ↔ ch '.' ∈ l := by
  constructor
  · intro h
    by_contra hn
    rw [List.splitOn_eq_singleton hn] at h
    simp at h
  · intro h
    obtain ⟨xs, rest, rfl⟩ := List.append_of_mem h
    rw [List.splitOn_append_cons_self]
    have h1 := List.splitOn_ne_nil (ch '.') xs
    have h2 := List.splitOn_ne_nil (ch '.') rest
    rw [List.length_append]
    have := List.length_pos_iff.mpr h1
    have := List.length_pos_iff.mpr h2
    omega

theorem dot_mem (host : List U8) : hosts.DOT ∈ host ↔ ch '.' ∈ bytes host := by
  simp only [bytes, List.mem_map]
  constructor
  · intro h; exact ⟨_, h, (dot_iff _).mp rfl⟩
  · rintro ⟨x, hx, he⟩
    rw [← dot_iff] at he; rw [← he]; exact hx

@[simp, scalar_tac_simps] theorem max_name_val : hosts.MAX_NAME.val = 253 := by unfold hosts.MAX_NAME; rfl

/-- **S33, a good host name.** -/
@[step]
theorem good_host_name_spec (host : Slice U8) :
    hosts.good_host_name host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ⦄ := by
  unfold hosts.good_host_name goodHostName hostLabels
  simp only [bytes_len]
  step*
  · apply WP.spec_mono (labels_ok_spec host 0#usize (by simp))
    intro r hr
    rw [hr]
    have hd : ch '.' ∈ bytes host.val := (dot_mem _).mp (b_post.mp ‹_›)
    have h0 : (0#usize : Usize).val = 0 := rfl
    rw [h0, List.drop_zero]
    unfold labelsGood
    have h3 := (two_labels _).mpr hd
    have h1 : 1 ≤ host.val.length := by scalar_tac
    have h2 : host.val.length ≤ 253 := by scalar_tac
    constructor
    · intro h; exact ⟨h1, h2, h3, h⟩
    · intro h; exact h.2.2.2
  · simp only [Bool.false_eq_true, false_iff, not_and]
    intro _ _ h2
    have : ch '.' ∈ bytes host.val := (two_labels _).mp h2
    exact absurd (b_post.mpr ((dot_mem _).mpr this)) ‹_›

/-- A name of the list at an index below `i` equals the host, without ASCII case. -/
def FoundBelow (list : Slice (alloc.vec.Vec U8)) (h : List Spec.Byte) (i : Nat) : Prop :=
  ∃ k, ∃ hk : k < list.val.length, k < i ∧ lower (bytes (list.val[k]).val) = lower h

theorem host_allowed_loop_spec (list : Slice (alloc.vec.Vec U8)) (low : alloc.vec.Vec U8)
    (h : List Spec.Byte) (hlow : bytes low.val = lower h) :
    hosts.host_allowed_loop list low false 0#usize ⦃ r =>
      (r = true ↔ FoundBelow list h list.val.length) ⦄ := by
  unfold hosts.host_allowed_loop
  apply loop.spec_decr_nat (fun st => list.val.length - st.2.val)
    (fun st => st.2.val ≤ list.val.length ∧ (st.1 = true ↔ FoundBelow list h st.2.val)) _ _ _ _
    ⟨by simp, by simp [FoundBelow]⟩
  rintro ⟨found, i⟩ ⟨hi, hfound⟩
  simp only at hi hfound
  unfold hosts.host_allowed_loop.body
  step*
  · simp only [true_iff]
    obtain ⟨k, hk, _, he⟩ := hfound.mp ‹_›
    exact ⟨k, hk, hk, he⟩
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hnot : ¬ FoundBelow list h i.val := by rw [← hfound]; assumption
    have hlt : i.val < list.val.length := by scalar_tac
    have heq : found1 = true ↔ lower (bytes (list.val[i.val]).val) = lower h := by
      rw [found1_post]
      simp only [PathRules.deref_val]
      rw [← Protocol.Seen.bytes_eq_iff, v1_post1, hlow, v_post]
      simp
    rw [heq, i2_post]
    constructor
    · intro he; exact ⟨i.val, hlt, by omega, he⟩
    · rintro ⟨k, hk, hki, he⟩
      by_cases hlt' : k < i.val
      · exact absurd ⟨k, hk, hlt', he⟩ hnot
      · have : k = i.val := by omega
        subst this; exact he

theorem foundBelow_iff (list : Slice (alloc.vec.Vec U8)) (h : List Spec.Byte) :
    FoundBelow list h list.val.length ↔ ∃ n ∈ strs list.val, lowerAscii n = lowerAscii h := by
  unfold FoundBelow strs lowerAscii
  simp only [List.mem_map]
  constructor
  · rintro ⟨k, hk, _, he⟩
    exact ⟨_, ⟨list.val[k], List.getElem_mem hk, rfl⟩, he⟩
  · rintro ⟨_, ⟨v, hv, rfl⟩, he⟩
    obtain ⟨k, hk, rfl⟩ := List.getElem_of_mem hv
    exact ⟨k, hk, hk, he⟩

/-- **S33, the allow list.** -/
@[step]
theorem host_allowed_spec (list : Slice (alloc.vec.Vec U8)) (host : Slice U8) :
    hosts.host_allowed list host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ∧
      ∃ h ∈ strs list.val, lowerAscii h = lowerAscii (bytes host.val) ⦄ := by
  unfold hosts.host_allowed
  step*
  · apply WP.spec_mono (host_allowed_loop_spec list lower (bytes host.val) lower_post1)
    intro r hr
    rw [hr, foundBelow_iff]
    have := b_post.mp ‹_›
    tauto

end Protocol.Hosts
