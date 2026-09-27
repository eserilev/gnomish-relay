import Protocol.Hosts
import Protocol.Spec.Connect

/-! # The target of a `CONNECT` request (S35) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.PathRules Protocol.Hosts

namespace Protocol.Connect

theorem byte_iff (b c : U8) : (b = c ↔ b.bv = c.bv) := UScalar.eq_equiv_bv_eq b c

@[step]
theorem is_line_end_spec (head : Slice U8) (at' : Usize) (h : at'.val + 1 < head.val.length) :
    connect.is_line_end head at' ⦃ r => (r = true ↔ (head.val[at'.val]).bv = ch '\r' ∧
      (head.val[at'.val + 1]).bv = ch '\n') ⦄ := by
  unfold connect.is_line_end
  have hcr : connect.CR.bv = ch '\r' := by unfold connect.CR; rfl
  have hlf : connect.LF.bv = ch '\n' := by unfold connect.LF; rfl
  step*
  · have e : head.val[at'.val + 1] = i2 := by rw [i2_post]; simp [i1_post]
    rw [← i_post, e]
    simp only [decide_eq_true_eq, byte_iff, hlf]
    have : i.bv = ch '\r' := by rw [← hcr]; exact (byte_iff _ _).mp ‹_›
    simp [this]
  · simp only [Bool.false_eq_true, false_iff, not_and]
    intro hc
    rw [← i_post] at hc
    exact absurd ((byte_iff _ _).mpr (hc.trans hcr.symm)) ‹_›

/-- A CR LF at `a`. -/
def CrLf (head : Slice U8) (a : Nat) : Prop :=
  ∃ h : a + 1 < head.val.length, (head.val[a]).bv = ch '\r' ∧ (head.val[a + 1]).bv = ch '\n'

theorem line_end_loop_spec (head : Slice U8) :
    connect.line_end_loop head 0#usize ⦃ a => (a.val = 0 ∨ a.val < head.val.length) ∧
      (a.val + 1 < head.val.length → CrLf head a.val) ⦄ := by
  unfold connect.line_end_loop
  apply loop.spec_decr_nat (fun a => head.val.length + 1 - a.val)
    (fun a => a.val = 0 ∨ a.val < head.val.length) _ _ _ _ (Or.inl rfl)
  intro a ha
  unfold connect.line_end_loop.body
  step*
  exact ⟨ha, fun h => ⟨h, (b_post.mp ‹_›)⟩⟩

@[step]
theorem line_end_spec (head : Slice U8) :
    connect.line_end head ⦃ r => r.val ≤ head.val.length ∧ (r.val < head.val.length → CrLf head r.val) ⦄ := by
  unfold connect.line_end
  step with line_end_loop_spec head as ⟨a, ha1, ha2⟩
  step*

theorem space_iff (b : U8) : (b = connect.SPACE ↔ b.bv = ch ' ') := by
  unfold connect.SPACE; rw [UScalar.eq_equiv_bv_eq]; rfl

@[step]
theorem next_space_spec (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ line.val.length) :
    connect.next_space line start stop ⦃ r => start.val ≤ r.val ∧ r.val ≤ stop.val ∧
      (∀ b ∈ seg line.val start.val r.val, b ≠ ch ' ') ∧
      (∀ h : r.val < stop.val, (line.val[r.val]'(by omega)).bv = ch ' ') ⦄ := by
  unfold connect.next_space connect.next_space_loop
  apply loop.spec_decr_nat (fun i => stop.val - i.val)
    (fun i => start.val ≤ i.val ∧ i.val ≤ stop.val ∧ ∀ b ∈ seg line.val start.val i.val, b ≠ ch ' ')
    _ _ _ _ ⟨le_refl _, hs, by simp [seg_empty]⟩
  rintro pos ⟨hi1, hi2, hnd⟩
  unfold connect.next_space_loop.body
  step*
  · refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
    rw [at1_post, seg_succ _ _ _ hi1 (by scalar_tac)]
    simp only [List.mem_append, List.mem_singleton]
    rintro b (hb | rfl)
    · exact hnd b hb
    · intro h
      rw [← i_post, ← space_iff] at h
      have := ‹(i != connect.SPACE) = true›
      simp [h] at this
  · refine ⟨hi1, hi2, hnd, fun _ => ?_⟩
    rw [← space_iff, ← i_post]
    have h3 := ‹¬(i != connect.SPACE) = true›
    simp at h3
    exact UScalar.eq_of_val_eq h3

@[step]
theorem copy_range_spec (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ line.val.length) :
    connect.copy_range line start stop ⦃ v => bytes v.val = seg line.val start.val stop.val ∧
      v.val.length = stop.val - start.val ⦄ := by
  unfold connect.copy_range
  step*
  constructor
  · simp [seg, v_post]
  · simp [v_post]; omega

@[step]
theorem equals_at_spec (line : Slice U8) (start stop : Usize) (word : Slice U8)
    (hs : start.val ≤ stop.val) (hstop : stop.val ≤ line.val.length) :
    connect.equals_at line start stop word ⦃ r => r = true →
      seg line.val start.val stop.val = bytes word.val ⦄ := by
  unfold connect.equals_at
  step*
  have ht := r_post1.mp r_post2
  have hl : stop.val - start.val = word.val.length := by scalar_tac
  simp only [Slice.len_val] at ht
  simp only [seg, hl, ht, List.take_length]

@[step]
theorem is_digit_spec (b : U8) : connect.is_digit b ⦃ r => (r = true ↔ isDigit b.bv) ⦄ := by
  unfold connect.is_digit
  rw [isDigit_iff, toNat_bv]
  step*

theorem http_bytes : bytes (Array.to_slice connect.HTTP_1).val = ascii "HTTP/1." := by
  unfold connect.HTTP_1; decide

@[simp] theorem http_len : (Array.to_slice connect.HTTP_1).val.length = 7 := by
  unfold connect.HTTP_1; rfl

@[step]
theorem is_version_spec (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ line.val.length) :
    connect.is_version line start stop ⦃ r => r = true →
      ∃ h : stop.val = start.val + 8, seg line.val start.val (start.val + 7) = ascii "HTTP/1." ∧
        isDigit (line.val[start.val + 7]'(by omega)).bv ⦄ := by
  unfold connect.is_version
  step*
  have hs2 : s2.val.length = 7 := by rw [s2_post]; exact http_len
  have h8 : stop.val = start.val + 8 := by
    have : s.val.length = 7 := by rw [s_post]; exact http_len
    scalar_tac
  refine ⟨h8, ?_, ?_⟩
  · have ht := b_post.mp ‹_›
    simp only [Slice.len_val, hs2] at ht
    have h7 : (s1.val).take 7 = s1.val := List.take_of_length_le (by rw [s1_post]; simp)
    rw [h7] at ht
    simp only [seg, show start.val + 7 - start.val = 7 by omega, ht, s1_post, http_bytes]
  · have e : line.val[start.val + 7] = i5 := by rw [i5_post]; simp [i4_post1, h8]
    rw [e]; exact r_post1.mp r_post2

theorem getD_getElem {α : Type} (l : List α) (i : Nat) (d : α) (h : i < l.length) :
    l.getD i d = l[i] := by
  simp [List.getD_eq_getElem?_getD, List.getElem?_eq_getElem h]

theorem colon_iff (b : U8) : (b = connect.COLON ↔ b.bv = ch ':') := by
  unfold connect.COLON; rw [UScalar.eq_equiv_bv_eq]; rfl

@[step]
theorem last_colon_spec (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ line.val.length) :
    connect.last_colon line start stop ⦃ r => start.val ≤ r.val ∧ r.val ≤ stop.val ∧
      (r.val < stop.val → (line.val.getD r.val 0#u8).bv = ch ':') ⦄ := by
  unfold connect.last_colon connect.last_colon_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => start.val ≤ st.2.val ∧ st.2.val ≤ stop.val ∧
      (st.1 = stop ∨ (start.val ≤ st.1.val ∧ st.1.val < st.2.val ∧
        (line.val.getD st.1.val 0#u8).bv = ch ':')))
    _ _ _ _ ⟨le_refl _, hs, Or.inl rfl⟩
  rintro ⟨found, pos⟩ ⟨hi1, hi2, hf⟩
  simp only at hi1 hi2 hf
  unfold connect.last_colon_loop.body
  by_cases hin : pos < stop
  · simp only [hin, if_true]
    step as ⟨i1, i1_post⟩
    split
    · rename_i hc
      step as ⟨p1, p1_post⟩
      refine ⟨by scalar_tac, by scalar_tac, Or.inr ⟨hi1, by scalar_tac, ?_⟩, by scalar_tac⟩
      rw [getD_getElem _ _ _ (by scalar_tac), ← colon_iff, ← i1_post]; exact hc
    · step as ⟨p1, p1_post⟩
      refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
      rcases hf with hf | ⟨hle, hlt, hc⟩
      · exact Or.inl hf
      · exact Or.inr ⟨hle, by scalar_tac, hc⟩
  · simp only [hin, if_false, WP.spec_ok]
    rcases hf with hf | ⟨hle, hlt, hc⟩
    · subst hf; exact ⟨hs, le_refl _, fun h => absurd h (lt_irrefl _)⟩
    · exact ⟨hle, by scalar_tac, fun _ => hc⟩

theorem decimalValue_snoc (l : List Spec.Byte) (b : Spec.Byte) :
    decimalValue (l ++ [b]) = 10 * decimalValue l + (b.toNat - 48) := by
  simp [decimalValue, List.foldl_append]

@[step]
theorem digit_value_spec (b : U8) (h : isDigit b.bv) :
    connect.digit_value b ⦃ v => v.val = b.bv.toNat - 48 ∧ v.val ≤ 9 ⦄ := by
  unfold connect.digit_value
  rw [isDigit_iff] at h
  rw [toNat_bv] at h ⊢
  step*

def DigitsInv (line : Slice U8) (start : Nat) (st : U32 × Bool × Usize) : Prop :=
  start ≤ st.2.2.val ∧
  (st.2.1 = true → allDigits (seg line.val start st.2.2.val) ∧
    st.1.val = decimalValue (seg line.val start st.2.2.val) ∧ st.1.val < 10 ^ (st.2.2.val - start))

@[step]
theorem digits_spec (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val)
    (hstop : stop.val ≤ line.val.length) (h5 : stop.val - start.val ≤ 5) :
    connect.digits line start stop ⦃ r => r.1 = true →
      allDigits (seg line.val start.val stop.val) ∧ r.2.val = decimalValue (seg line.val start.val stop.val) ⦄ := by
  unfold connect.digits connect.digits_loop
  apply loop.spec_decr_nat (fun st => stop.val + 1 - st.2.2.val)
    (fun st => st.2.2.val ≤ stop.val ∧ DigitsInv line start.val st) _ _ _ _
    ⟨hs, le_refl _, fun _ => ⟨by simp [seg_empty, allDigits], by simp [seg_empty, decimalValue], by simp⟩⟩
  rintro ⟨value, ok1, pos⟩ ⟨hp, hs1, hinv⟩
  simp only at hp hs1 hinv
  unfold connect.digits_loop.body
  by_cases hok : ok1 = true
  · simp only [hok, if_true]
    obtain ⟨hall, hval, hlt⟩ := hinv hok
    by_cases hin : pos < stop
    · simp only [hin, if_true]
      step as ⟨i1, i1_post⟩
      step as ⟨ok2, ok2_post⟩
      have hseg : seg line.val start.val (pos.val + 1) = seg line.val start.val pos.val ++ [i1.bv] := by
        rw [seg_succ _ _ _ hs1 (by scalar_tac), i1_post]
      by_cases hd : ok2 = true
      · simp only [hd, if_true]
        have hdig := ok2_post.mp hd
        have hk : pos.val - start.val ≤ 4 := by scalar_tac
        have hpow : 10 ^ (pos.val - start.val) ≤ 10 ^ 4 := Nat.pow_le_pow_right (by omega) hk
        step as ⟨i2, i2_post⟩
        step as ⟨i3, i3_post, i3_le⟩
        step as ⟨v1, v1_post⟩
        step as ⟨p1, p1_post⟩
        refine ⟨by scalar_tac, ⟨by scalar_tac, fun _ => ⟨?_, ?_, ?_⟩⟩, by scalar_tac⟩
        · rw [p1_post, hseg]
          intro b hb
          simp only [List.mem_append, List.mem_singleton] at hb
          rcases hb with hb | rfl
          · exact hall b hb
          · exact hdig
        · rw [p1_post, hseg, decimalValue_snoc, v1_post, i2_post, i3_post, hval]; ring
        · rw [p1_post, show pos.val + 1 - start.val = (pos.val - start.val) + 1 by scalar_tac,
            Nat.pow_succ]
          scalar_tac
      · simp only [Bool.not_eq_true] at hd
        simp only [hd, Bool.false_eq_true, if_false]
        step as ⟨p1, p1_post⟩
        exact ⟨by scalar_tac, ⟨by scalar_tac, fun h => absurd h (by simp)⟩, by scalar_tac⟩
    · simp only [hin, if_false, WP.spec_ok]
      intro _
      have : pos.val = stop.val := by scalar_tac
      rw [← this]; exact ⟨hall, hval⟩
  · simp only [Bool.not_eq_true] at hok
    simp only [hok, Bool.false_eq_true, if_false, WP.spec_ok]
    intro h; simp at h

@[step]
theorem parse_port_spec (line : Slice U8) (start stop : Usize) (hstop : stop.val ≤ line.val.length) :
    connect.parse_port line start stop ⦃ r => ∀ p, r = some p →
      start.val < stop.val ∧ stop.val - start.val ≤ 5 ∧ allDigits (seg line.val start.val stop.val) ∧
      decimalValue (seg line.val start.val stop.val) = p.val ⦄ := by
  unfold connect.parse_port
  step*

@[step]
theorem last_label_start_spec (host : Slice U8) :
    connect.last_label_start host ⦃ r => r.val ≤ host.val.length ⦄ := by
  unfold connect.last_label_start connect.last_label_start_loop
  apply loop.spec_decr_nat (fun st => host.val.length - st.2.val)
    (fun st => st.1.val ≤ host.val.length ∧ st.2.val ≤ host.val.length) _ _ _ _ ⟨by simp, by simp⟩
  rintro ⟨st, pos⟩ ⟨h1, h2⟩
  simp only at h1 h2
  unfold connect.last_label_start_loop.body
  by_cases hin : pos < host.len
  · simp only [hin, if_true]
    step as ⟨i2, i2_post⟩
    split
    · step as ⟨s1, s1_post⟩
      step as ⟨p1, p1_post⟩
      exact ⟨by scalar_tac, by scalar_tac, by scalar_tac⟩
    · step as ⟨p1, p1_post⟩
      exact ⟨h1, by scalar_tac, by scalar_tac⟩
  · simp only [hin, if_false, WP.spec_ok]
    exact h1

@[step]
theorem looks_like_ip_spec (host : Slice U8) :
    connect.looks_like_ip host ⦃ r => r = false → ch ':' ∉ bytes host.val ⦄ := by
  unfold connect.looks_like_ip
  have hc : ¬ connect.COLON ∈ host.val → ch ':' ∉ bytes host.val := by
    intro h hm
    simp only [bytes, List.mem_map] at hm
    obtain ⟨x, hx, he⟩ := hm
    rw [← colon_iff] at he
    exact h (he ▸ hx)
  step*

@[step]
theorem is_listed_port_spec (ports : Slice U16) (port : U16) :
    connect.is_listed_port ports port ⦃ r => (r = true ↔ port ∈ ports.val) ⦄ := by
  unfold connect.is_listed_port connect.is_listed_port_loop
  apply loop.spec_decr_nat (fun st => ports.val.length - st.2.val)
    (fun st => st.2.val ≤ ports.val.length ∧ (st.1 = true ↔ port ∈ ports.val.take st.2.val))
    _ _ _ _ ⟨by simp, by simp⟩
  rintro ⟨found, i⟩ ⟨hi, hf⟩
  simp only at hi hf
  unfold connect.is_listed_port_loop.body
  step*
  · simp only [true_iff]; exact List.mem_of_mem_take (hf.mp ‹_›)
  · refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    have hlt : i.val < ports.val.length := by scalar_tac
    have hnot : port ∉ ports.val.take i.val := by rw [← hf]; assumption
    rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt, decide_eq_true_iff]
    simp only [Option.toList_some, List.mem_append, List.mem_singleton, hnot, false_or, i2_post]
    exact eq_comm
  · have : i.val = ports.val.length := by scalar_tac
    rw [this, List.take_length] at hf
    rw [← hf]; simp_all

theorem dangerous_val : (Array.to_slice connect.DANGEROUS_PORTS).val = [2375#u16, 2376#u16, 9222#u16] := by
  unfold connect.DANGEROUS_PORTS; rfl

@[step]
theorem local_target_spec (ports : Slice U16) (port : U16) :
    connect.local_target ports port ⦃ r => ∀ t, r = .Ok t →
      t = .Local port ∧ port ∈ ports.val ∧ port.val ∉ [2375, 2376, 9222] ⦄ := by
  unfold connect.local_target
  step*
  intro t ht
  cases ht
  refine ⟨rfl, b1_post.mp ‹_›, ?_⟩
  intro hm
  apply ‹¬b = true›
  rw [b_post, s_post, dangerous_val]
  simp only [List.mem_cons, List.mem_nil_iff, or_false] at hm ⊢
  rcases hm with h | h | h
  · left; exact UScalar.eq_of_val_eq h
  · right; left; exact UScalar.eq_of_val_eq h
  · right; right; exact UScalar.eq_of_val_eq h

@[step]
theorem host_passes_spec (mode : connect.Mode) (list : Slice (alloc.vec.Vec U8)) (host : Slice U8) :
    connect.host_passes mode list host ⦃ r => r = true →
      (mode = .Listed → hostAllowed list (bytes host.val)) ∧
      (mode = .Public → goodHostName (bytes host.val)) ⦄ := by
  unfold connect.host_passes
  induction mode
  · step*
    exact ⟨r_post1.mp r_post2, fun h' => by simp at h'⟩
  · step*

theorem seg_full (host : List U8) : seg host 0 host.length = bytes host := by
  simp [seg]

@[step]
theorem remote_target_spec (mode : connect.Mode) (list : Slice (alloc.vec.Vec U8)) (host : Slice U8)
    (port : U16) :
    connect.remote_target mode list host port ⦃ r => ∀ t, r = .Ok t →
      ∃ th, t = .Remote th port ∧ (port.val = 443 ∨ port.val = 80) ∧ bytes th.val = lowerAscii (bytes host.val) ∧
        (mode = .Listed → hostAllowed list (bytes host.val)) ∧
        (mode = .Public → goodHostName (bytes host.val)) ⦄ := by
  unfold connect.remote_target
  have hlen : ∀ v : alloc.vec.Vec U8, bytes v.val = lower (seg host.val 0 host.len.val) →
      bytes v.val = lowerAscii (bytes host.val) := by
    intro v hv; rw [hv, Slice.len_val, seg_full]
  step*

theorem c_localhost : bytes (Array.to_slice connect.LOCALHOST).val = ascii "localhost" := by
  unfold connect.LOCALHOST; decide

theorem colon_not_in_lower (h : List Spec.Byte) (hl : lower h = ascii "localhost") : ch ':' ∉ h := by
  intro hm
  have : lowerByte (ch ':') ∈ lower h := List.mem_map_of_mem hm
  rw [hl] at this
  revert this
  decide

theorem colon_facts (line : Slice U8) (start stop colon : Nat) (h1 : start ≤ colon) (h2 : colon ≤ stop)
    (h3 : colon < stop → (line.val.getD colon 0#u8).bv = ch ':') (hne : colon ≠ stop) :
    start ≤ colon ∧ colon < stop ∧ (line.val.getD colon 0#u8).bv = ch ':' := by
  have : colon < stop := by omega
  exact ⟨h1, this, h3 this⟩

theorem host_seg (host : alloc.vec.Vec U8) (h : List Spec.Byte) (hh : bytes host.val = h) :
    seg (alloc.vec.Vec.deref host).val 0 host.len.val = h := by
  simp only [PathRules.deref_val, alloc.vec.Vec.len_val]
  rw [seg_full, hh]

/-- What the target of a request is, when the check passes: `h` and `ds` are the bytes
before and after the last `:`. -/
def TargetOk (mode : connect.Mode) (list : Slice (alloc.vec.Vec U8)) (ports : Slice U16)
    (h ds : List Spec.Byte) (t : connect.Target) : Prop :=
  ∃ p : U16, allDigits ds ∧ 1 ≤ ds.length ∧ ds.length ≤ 5 ∧ decimalValue ds = p.val ∧ ch ':' ∉ h ∧
    ((t = .Local p ∧ lowerAscii h = ascii "localhost" ∧ p ∈ ports.val ∧ p.val ∉ [2375, 2376, 9222]) ∨
      (∃ th, t = .Remote th p ∧ (p.val = 443 ∨ p.val = 80) ∧ bytes th.val = lowerAscii h ∧
        (mode = .Listed → hostAllowed list h) ∧ (mode = .Public → goodHostName h)))

@[step]
theorem check_host_port_spec (mode : connect.Mode) (list : Slice (alloc.vec.Vec U8)) (ports : Slice U16)
    (line : Slice U8) (start stop : Usize) (hs : start.val ≤ stop.val) (hstop : stop.val ≤ line.val.length) :
    connect.check_host_port mode list ports line start stop ⦃ r => ∀ t, r = .Ok t →
      ∃ colon, start.val ≤ colon ∧ colon < stop.val ∧ (line.val.getD colon 0#u8).bv = ch ':' ∧
        TargetOk mode list ports (seg line.val start.val colon) (seg line.val (colon + 1) stop.val) t ⦄ := by
  unfold connect.check_host_port
  step*
  all_goals try (simp; done)
  · -- `localhost` and a port of the list.
    obtain ⟨hloc, hin, hnd⟩ := r_post1 _ r_post2
    subst hloc
    obtain ⟨hlt, h5, hall, hval⟩ := o_post port ‹_›
    have hc := colon_facts line _ _ _ colon_post1 colon_post2 colon_post3
      (fun h => ‹¬colon = stop› (UScalar.eq_of_val_eq h))
    have hl : lower (seg line.val start.val colon.val) = ascii "localhost" := by
      rw [← host_seg host _ host_post1, ← v_post, ← c_localhost, ← s2_post]
      have := b_post.mp ‹_›
      simp only [PathRules.deref_val] at this
      rw [this]
    refine ⟨colon.val, hc.1, hc.2.1, hc.2.2, port, ?_⟩
    rw [← i1_post]
    refine ⟨hall, by rw [seg_length _ _ _ hstop]; omega, by rw [seg_length _ _ _ hstop]; omega, hval,
      colon_not_in_lower _ hl, Or.inl ⟨rfl, hl, hin, hnd⟩⟩
  · -- A remote host.
    obtain ⟨th, hrem, hport, hth, hlist, hpub⟩ := r_post1 _ r_post2
    subst hrem
    obtain ⟨hlt, h5, hall, hval⟩ := o_post port ‹_›
    have hc := colon_facts line _ _ _ colon_post1 colon_post2 colon_post3
      (fun h => ‹¬colon = stop› (UScalar.eq_of_val_eq h))
    have hh : bytes (alloc.vec.Vec.deref host).val = seg line.val start.val colon.val := by
      simp only [PathRules.deref_val]; exact host_post1
    rw [hh] at hth hlist hpub
    have hnc := b1_post (by simpa using ‹¬b1 = true›)
    rw [hh] at hnc
    refine ⟨colon.val, hc.1, hc.2.1, hc.2.2, port, ?_⟩
    rw [← i1_post]
    exact ⟨hall, by rw [seg_length _ _ _ hstop]; omega, by rw [seg_length _ _ _ hstop]; omega, hval,
      hnc, Or.inr ⟨th, rfl, hport, hth, hlist, hpub⟩⟩

/-! ## The whole line -/

theorem seg_split (l : List U8) (a b c : Nat) (hab : a ≤ b) (hbc : b ≤ c) (_hc : c ≤ l.length) :
    seg l a c = seg l a b ++ seg l b c := by
  simp only [seg, bytes, ← List.map_append]
  congr 1
  rw [show c - a = (b - a) + (c - b) by omega, List.take_add, List.drop_drop,
    show a + (b - a) = b by omega]

theorem seg_one (l : List U8) (a : Nat) (h : a < l.length) : seg l a (a + 1) = [(l[a]).bv] := by
  rw [seg_succ l a a (le_refl _) h, seg_empty l a a (le_refl _)]; rfl

theorem seg_one_getD (l : List U8) (a : Nat) (h : a < l.length) :
    seg l a (a + 1) = [(l.getD a 0#u8).bv] := by
  rw [seg_one l a h, getD_getElem _ _ _ h]

theorem bytes_eq_seg (l : List U8) : bytes l = seg l 0 l.length := by simp [seg]

theorem seg_to_end (l : List U8) (a : Nat) : seg l a l.length = bytes (l.drop a) := by
  simp [seg, bytes]

theorem connect_bytes : bytes (Array.to_slice connect.CONNECT).val = ascii "CONNECT" := by
  unfold connect.CONNECT; decide

theorem not_mem_seg_left (l : List U8) (a b c : Nat) (x : Spec.Byte) (hab : a ≤ b) (hbc : b ≤ c)
    (hc : c ≤ l.length) (h : x ∉ seg l a c) : x ∉ seg l a b := by
  rw [seg_split l a b c hab hbc hc] at h
  simp only [List.mem_append, not_or] at h
  exact h.1

/-- **S35.** -/
theorem check_target_spec (mode : connect.Mode) (list : Slice (alloc.vec.Vec U8)) (ports : Slice U16)
    (head : Slice U8) :
    connect.check_target mode list ports head ⦃ r => ∀ t, r = .Ok t →
      ∃ (h ds : List Spec.Byte) (p : U16) (d : Spec.Byte) (rest : List Spec.Byte),
        bytes head.val = ascii "CONNECT " ++ h ++ [ch ':'] ++ ds ++ ascii " HTTP/1." ++ [d] ++
          ascii "\r\n" ++ rest ∧
        isDigit d ∧ allDigits ds ∧ 1 ≤ ds.length ∧ ds.length ≤ 5 ∧ decimalValue ds = p.val ∧
        ch ' ' ∉ h ∧ ch ':' ∉ h ∧
        ((t = .Local p ∧ lowerAscii h = ascii "localhost" ∧ p ∈ ports.val ∧ p.val ∉ [2375, 2376, 9222]) ∨
          (∃ th, t = .Remote th p ∧ (p.val = 443 ∨ p.val = 80) ∧ bytes th.val = lowerAscii h ∧
            (mode = .Listed → hostAllowed list h) ∧ (mode = .Public → goodHostName h))) ⦄ := by
  unfold connect.check_target
  step*
  obtain ⟨colon, hc1, hc2, hcol, p, hall, hd1, hd5, hval, hnc, htgt⟩ := r_post1 _ r_post2
  obtain ⟨h8, hhttp, hdig⟩ := b1_post ‹_›
  have hcon := b2_post ‹_›
  rw [s1_post, connect_bytes] at hcon
  have hend : «end».val < head.val.length := by
    have : ¬ «end» = head.len := ‹_›
    have : «end».val ≠ head.len.val := fun h => this (UScalar.eq_of_val_eq h)
    simp at this; omega
  have hf7 : first.val = 7 := by
    have := congrArg List.length hcon
    rw [seg_length _ _ _ (by omega)] at this
    simp [ascii] at this; omega
  obtain ⟨hl1, hcr, hlf⟩ := end_post2 hend
  have hsp1 := first_post4 (by scalar_tac)
  have hsp2 := second_post4 (by scalar_tac)
  -- The line, cut at each place that the checks name.
  have e : bytes head.val =
      seg head.val 0 7 ++ seg head.val 7 8 ++ seg head.val 8 colon ++ seg head.val colon (colon + 1) ++
      seg head.val (colon + 1) second.val ++ seg head.val second.val (second.val + 1) ++
      seg head.val (second.val + 1) (second.val + 8) ++ seg head.val (second.val + 8) (second.val + 9) ++
      seg head.val (second.val + 9) (second.val + 10) ++ seg head.val (second.val + 10) (second.val + 11) ++
      seg head.val (second.val + 11) head.val.length := by
    rw [bytes_eq_seg]
    rw [seg_split _ 0 7 _ (by omega) (by omega) (le_refl _),
      seg_split _ 7 8 _ (by omega) (by omega) (le_refl _),
      seg_split _ 8 colon _ (by omega) (by omega) (le_refl _),
      seg_split _ colon (colon + 1) _ (by omega) (by omega) (le_refl _),
      seg_split _ (colon + 1) second.val _ (by omega) (by omega) (le_refl _),
      seg_split _ second.val (second.val + 1) _ (by omega) (by omega) (le_refl _),
      seg_split _ (second.val + 1) (second.val + 8) _ (by omega) (by omega) (le_refl _),
      seg_split _ (second.val + 8) (second.val + 9) _ (by omega) (by omega) (le_refl _),
      seg_split _ (second.val + 9) (second.val + 10) _ (by omega) (by omega) (le_refl _),
      seg_split _ (second.val + 10) (second.val + 11) _ (by omega) (by omega) (le_refl _)]
    simp only [List.append_assoc]
  have e7 : seg head.val 0 7 = ascii "CONNECT" := by rw [← hf7]; exact hcon
  have e8 : seg head.val 7 8 = [ch ' '] := by
    have := seg_one head.val first.val (by omega)
    rw [hsp1, hf7] at this; exact this
  have ec : seg head.val colon (colon + 1) = [ch ':'] := by
    rw [seg_one_getD _ _ (by omega), hcol]
  have es : seg head.val second.val (second.val + 1) = [ch ' '] := by
    have := seg_one head.val second.val (by omega)
    rw [hsp2] at this; exact this
  have eh : seg head.val (second.val + 1) (second.val + 8) = ascii "HTTP/1." := by
    rw [i2_post] at hhttp; rw [show second.val + 8 = second.val + 1 + 7 by omega]; exact hhttp
  have hdig' : isDigit (head.val.getD (second.val + 8) 0#u8).bv := by
    have := getD_getElem head.val (i2.val + 7) 0#u8 (by omega)
    rw [← this, i2_post] at hdig; exact hdig
  have ed : seg head.val (second.val + 8) (second.val + 9) = [(head.val.getD (second.val + 8) 0#u8).bv] :=
    seg_one_getD _ _ (by omega)
  have ecr : seg head.val (second.val + 9) (second.val + 10) = [ch '\r'] := by
    have := seg_one head.val «end».val (by omega)
    rw [hcr, h8, i2_post] at this; exact this
  have elf : seg head.val (second.val + 10) (second.val + 11) = [ch '\n'] := by
    have := seg_one head.val («end».val + 1) (by omega)
    rw [hlf, h8, i2_post] at this; exact this
  rw [e7, e8, ec, es, eh, ed, ecr, elf, seg_to_end] at e
  refine ⟨seg head.val 8 colon, seg head.val (colon + 1) second.val, p, (head.val.getD (second.val + 8) 0#u8).bv,
    bytes (head.val.drop (second.val + 11)), ?_, hdig', hall, hd1, hd5, hval, ?_, ?_, ?_⟩
  · rw [e]
    simp only [List.append_assoc, List.cons_append, List.nil_append]
    rfl
  · have := not_mem_seg_left head.val (first.val + 1) colon second.val (ch ' ') (by omega) (by omega)
      (by omega) (fun hm => second_post3 _ (by rw [i1_post] at *; exact hm) rfl)
    rw [hf7] at this; exact this
  · rw [i1_post, hf7] at hnc; exact hnc
  · rw [i1_post, hf7] at htgt; exact htgt

end Protocol.Connect
