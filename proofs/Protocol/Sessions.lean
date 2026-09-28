import Protocol.Ascii
import Protocol.Seen
import Protocol.Spec.Sessions

/-! # The terminal sessions and their notices (S41) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Sessions

@[simp, scalar_tac_simps]
theorem max_sessions_val : sessions.MAX_SESSIONS.val = 32 := by unfold sessions.MAX_SESSIONS; rfl

theorem vec_ext {α : Type} (a b : alloc.vec.Vec α) (h : a.val = b.val) : a = b := Subtype.ext h

@[step]
theorem copy_notice_spec (n : live.Notice) : sessions.copy_notice n ⦃ r => r = n ⦄ := by
  unfold sessions.copy_notice
  step*
  cases n
  simp only [live.Notice.mk.injEq, true_and]
  constructor <;> apply Subtype.ext <;> simp_all

@[step]
theorem copy_optional_notice_spec (o : Option live.Notice) :
    sessions.copy_optional_notice o ⦃ r => r = o ⦄ := by
  unfold sessions.copy_optional_notice
  induction o <;> step*

@[step]
theorem copy_session_spec (s : sessions.Session) : sessions.copy_session s ⦃ r => r = s ⦄ := by
  unfold sessions.copy_session
  step*
  cases s
  simp only [sessions.Session.mk.injEq, true_and]
  constructor
  · apply Subtype.ext; simp_all
  · simp_all

theorem sessionId_eq_iff (s : sessions.Session) (id : List U8) :
    sessionId s = bytes id ↔ s.id.val = id := by
  rw [sessionId, Protocol.Seen.bytes_eq_iff]

theorem find_session_loop_spec (table : Slice sessions.Session) (id : Slice U8) (i : Usize)
    (hi : i.val ≤ table.val.length)
    (hnone : ∀ j (hj : j < i.val), sessionId (table.val[j]'(by omega)) ≠ bytes id.val) :
    sessions.find_session_loop table id i ⦃ r =>
      r.val ≤ table.val.length ∧
      (∀ h : r.val < table.val.length, sessionId (table.val[r.val]'h) = bytes id.val) ∧
      (r.val = table.val.length → ∀ s ∈ table.val, sessionId s ≠ bytes id.val) ⦄ := by
  unfold sessions.find_session_loop
  apply loop.spec_decr_nat (fun i => table.val.length - i.val)
    (fun i => ∃ _ : i.val ≤ table.val.length,
      ∀ j (hj : j < i.val), sessionId (table.val[j]'(by omega)) ≠ bytes id.val)
    _ _ _ _ ⟨hi, hnone⟩
  rintro i ⟨hi, hnone⟩
  unfold sessions.find_session_loop.body
  step*
  · -- Found it at i.
    have hb : b = true := by assumption
    have heq := b_post.mp hb
    refine ⟨by scalar_tac, fun h => ?_, fun h => by scalar_tac⟩
    rw [sessionId_eq_iff, ← heq, s_post]
    rfl
  · refine ⟨⟨by scalar_tac, fun j hj => ?_⟩, by scalar_tac⟩
    by_cases hji : j < i.val
    · exact hnone j hji
    · have : j = i.val := by scalar_tac
      subst this
      rw [ne_eq, sessionId_eq_iff, ← s_post]
      intro h
      have hb : ¬ b = true := by assumption
      exact hb (b_post.mpr h)
  · refine ⟨by scalar_tac, fun h => by scalar_tac, fun _ s hs => ?_⟩
    obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hs
    exact hnone j (by scalar_tac)

@[step]
theorem find_session_spec (table : Slice sessions.Session) (id : Slice U8) :
    sessions.find_session table id ⦃ r =>
      r.val ≤ table.val.length ∧
      (∀ h : r.val < table.val.length, sessionId (table.val[r.val]'h) = bytes id.val) ∧
      (r.val = table.val.length → ∀ s ∈ table.val, sessionId s ≠ bytes id.val) ⦄ := by
  unfold sessions.find_session
  exact find_session_loop_spec table id 0#usize (by simp) (fun j hj => absurd hj (by simp))

theorem eraseIdx_snoc {α : Type} (l : List α) (x : α) (k : Nat) (hk : k ≠ l.length) :
    (l ++ [x]).eraseIdx k = l.eraseIdx k ++ [x] := by
  rcases Nat.lt_or_gt_of_ne hk with h | h
  · exact List.eraseIdx_append_of_lt_length h _
  · rw [List.eraseIdx_of_length_le (by simp; omega), List.eraseIdx_of_length_le (by omega)]

theorem eraseIdx_snoc_self {α : Type} (l : List α) (x : α) (k : Nat) (hk : k = l.length) :
    (l ++ [x]).eraseIdx k = l := by
  subst hk
  rw [List.eraseIdx_append_of_length_le (le_refl _)]
  simp

theorem without_session_loop_spec (table : Slice sessions.Session) (at' : Usize)
    (out : alloc.vec.Vec sessions.Session) (i : Usize) (hi : i.val ≤ table.val.length)
    (hout : out.val = (table.val.take i.val).eraseIdx at'.val) (hlen : table.val.length ≤ 2 ^ 16) :
    sessions.without_session_loop table at' out i ⦃ r => r.val = table.val.eraseIdx at'.val ⦄ := by
  unfold sessions.without_session_loop
  apply loop.spec_decr_nat (fun st => table.val.length - st.2.val)
    (fun st => st.2.val ≤ table.val.length ∧ st.1.val = (table.val.take st.2.val).eraseIdx at'.val)
    _ _ _ _ ⟨hi, hout⟩
  rintro ⟨o, i⟩ ⟨hi, hout⟩
  simp only at hi hout
  unfold sessions.without_session_loop.body
  simp only [bne_iff_ne, ne_eq]
  step*
  · have hlt : i.val < table.val.length := by scalar_tac
    have hol : o.val.length ≤ i.val := by
      have := List.length_eraseIdx_le (table.val.take i.val) at'.val
      rw [hout]; simp at this ⊢; omega
    by_cases heq : i = at'
    · subst heq
      rw [if_neg (not_not.mpr rfl)]
      step*
      refine ⟨by scalar_tac, ?_, by scalar_tac⟩
      rw [hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, Option.toList_some]
      rw [eraseIdx_snoc_self _ _ _ (by simp; omega), List.eraseIdx_of_length_le (by simp)]
    · rw [if_pos heq]
      have hne : at'.val ≠ i.val := by
        intro h; exact heq (UScalar.eq_of_val_eq h.symm)
      step*
      refine ⟨by scalar_tac, ?_, by scalar_tac⟩
      rw [out1_post, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, Option.toList_some,
        eraseIdx_snoc _ _ _ (by simp; omega), x_post]
      simp_all
  · have : i.val = table.val.length := by scalar_tac
    rw [hout, this, List.take_length]

@[step]
theorem without_session_spec (table : Slice sessions.Session) (at' : Usize) (hlen : table.val.length ≤ 2 ^ 16) :
    sessions.without_session table at' ⦃ r => r.val = table.val.eraseIdx at'.val ⦄ := by
  unfold sessions.without_session
  exact without_session_loop_spec table at' _ 0#usize (by simp) (by simp) hlen

@[step]
theorem with_session_spec (table : Slice sessions.Session) (at' : Usize) (s : sessions.Session)
    (hlen : table.val.length ≤ 2 ^ 16) :
    sessions.with_session table at' s ⦃ r => r.val = table.val.eraseIdx at'.val ++ [s] ⦄ := by
  unfold sessions.with_session
  step*
  · rw [out_post]
    have := List.length_eraseIdx_le table.val at'.val
    scalar_tac

@[step]
theorem is_end_spec (k : sessions.EventKind) :
    sessions.is_end k ⦃ b => (b = true ↔ k = .SessionEnd) ⦄ := by
  unfold sessions.is_end
  induction k <;> step*

@[step]
theorem started_at_spec (table : Slice sessions.Session) (at' : Usize) :
    sessions.started_at table at' ⦃ _ => True ⦄ := by
  unfold sessions.started_at
  step*

@[step]
theorem took_spec (started now : U32) : sessions.took started now ⦃ _ => True ⦄ := by
  unfold sessions.took
  step*

@[step]
theorem new_notice_spec (e : sessions.Event) (k : live.NoticeKind) (took now : U32) :
    sessions.new_notice e k took now ⦃ n => noticeOfEvent e k n ∧ n.at = now ⦄ := by
  unfold sessions.new_notice noticeOfEvent
  step*
  simp_all

/-- What `next_session` leaves: the id of the event, and the notice of the event or none. -/
def nextOk (e : sessions.Event) (s : sessions.Session) : Prop :=
  sessionId s = bytes e.session.val ∧
    match noticeKindOf e.kind with
    | some k => ∃ n, s.notice = some n ∧ noticeOfEvent e k n
    | none => s.notice = none

@[step]
theorem next_session_spec (started : U32) (e : sessions.Event) (now : U32) :
    sessions.next_session started e now ⦃ s => nextOk e s ⦄ := by
  unfold sessions.next_session nextOk
  rcases e with ⟨session, kind, source, repo, text, id⟩
  induction kind <;> step* <;> simp_all [sessionId, noticeKindOf]

def hasNotice (s : sessions.Session) : Prop := s.notice ≠ none

/-- The order of a full table: a session with no notice first, by its latest event, then
the sessions with a notice, by the time of the notice. -/
def key (s : sessions.Session) : Nat :=
  match s.notice with
  | some n => 2 ^ 32 + n.at.val
  | none => s.last.val

@[step]
theorem has_notice_spec (s : sessions.Session) :
    sessions.has_notice s ⦃ b => (b = true ↔ s.notice ≠ none) ⦄ := by
  unfold sessions.has_notice
  rcases s with ⟨id, started, last, notice⟩
  induction notice <;> step*

@[step]
theorem notice_time_spec (s : sessions.Session) :
    sessions.notice_time s ⦃ t => t.val = noticeTime s ⦄ := by
  unfold sessions.notice_time noticeTime
  rcases s with ⟨id, started, last, notice⟩
  induction notice <;> step*

@[step]
theorem goes_before_spec (a b : sessions.Session) :
    sessions.goes_before a b ⦃ r => (r = true ↔ key a < key b) ⦄ := by
  unfold sessions.goes_before
  step*
  all_goals
    rcases a with ⟨ida, sa, la, na⟩
    rcases b with ⟨idb, sb, lb, nb⟩
    cases na <;> cases nb <;> simp_all [key, noticeTime] <;> scalar_tac

/-- `best` has the smallest key of the first `i` sessions. -/
def MinInv (table : List sessions.Session) (st : Usize × Usize) : Prop :=
  st.1.val < table.length ∧ 1 ≤ st.2.val ∧ st.2.val ≤ table.length ∧ st.1.val < st.2.val ∧
    ∀ j, j < st.2.val → ∀ (hjl : j < table.length) (hbl : st.1.val < table.length),
      key (table[st.1.val]'hbl) ≤ key (table[j]'hjl)

@[step]
theorem replaced_in_full_table_spec (table : Slice sessions.Session) (h1 : 1 ≤ table.val.length)
    (hlen : table.val.length ≤ 2 ^ 16) :
    sessions.replaced_in_full_table table ⦃ r =>
      ∃ hr : r.val < table.val.length,
        ∀ j (hj : j < table.val.length), key (table.val[r.val]'hr) ≤ key (table.val[j]'hj) ⦄ := by
  unfold sessions.replaced_in_full_table sessions.replaced_in_full_table_loop
  apply loop.spec_decr_nat (fun st => table.val.length - st.2.val) (MinInv table.val) _ _ _ _
    ⟨by simp; omega, by simp, by simp; omega, by simp, fun j hj _ _ => by
      have : j = 0 := by simp at hj; omega
      subst this; simp⟩
  rintro ⟨best, i⟩ ⟨hb, hi1, hi, hbi, hmin⟩
  simp only at hb hi1 hi hbi hmin
  unfold sessions.replaced_in_full_table_loop.body
  step*
  · have hlt : i.val < table.val.length := by scalar_tac
    by_cases hbt : b = true
    · -- The session at i goes before the best one: it is the new best.
      rw [if_pos hbt]
      step*
      have hk : key table.val[i.val] < key table.val[best.val] := by
        rw [← s_post, ← s1_post]; exact b_post.mp hbt
      refine ⟨⟨hlt, by scalar_tac, by scalar_tac, by scalar_tac, fun j hj hjl _ => ?_⟩, by scalar_tac⟩
      by_cases hji : j < i.val
      · have := hmin j hji hjl hb; simp only at *; omega
      · have : j = i.val := by scalar_tac
        subst this; simp
    · rw [if_neg hbt]
      step*
      have hk : ¬ key table.val[i.val] < key table.val[best.val] := by
        rw [← s_post, ← s1_post]; intro h; exact hbt (b_post.mpr h)
      refine ⟨⟨hb, by scalar_tac, by scalar_tac, by scalar_tac, fun j hj hjl _ => ?_⟩, by scalar_tac⟩
      by_cases hji : j < i.val
      · exact hmin j hji hjl hb
      · have : j = i.val := by scalar_tac
        subst this; simp only at *; omega

@[step]
theorem target_spec (table : Slice sessions.Session) (at' : Usize) (hat : at'.val ≤ table.val.length)
    (hlen : table.val.length ≤ 2 ^ 16) :
    sessions.target table at' ⦃ r =>
      (at'.val < table.val.length ∨ table.val.length < 32 → r = at') ∧
      (table.val.length ≤ at'.val → 32 ≤ table.val.length →
        ∃ hr : r.val < table.val.length,
          ∀ j (hj : j < table.val.length), key (table.val[r.val]'hr) ≤ key (table.val[j]'hj)) ⦄ := by
  unfold sessions.target
  step*

/-! ## Lists without one index -/

theorem mem_of_mem_eraseIdx {α : Type} {l : List α} {k : Nat} {x : α} (h : x ∈ l.eraseIdx k) : x ∈ l :=
  (List.eraseIdx_sublist l k).subset h

theorem nodup_eraseIdx (l : List sessions.Session) (k : Nat) (h : oneNoticeEach l) :
    oneNoticeEach (l.eraseIdx k) :=
  List.Nodup.sublist ((List.eraseIdx_sublist l k).map _) h

theorem mem_eraseIdx_of_ne (l : List sessions.Session) (k j : Nat) (hj : j < l.length) (hne : j ≠ k) :
    l[j] ∈ l.eraseIdx k :=
  List.mem_eraseIdx_iff_getElem.mpr ⟨j, hj, hne, rfl⟩

theorem sid_unique (l : List sessions.Session) (h : oneNoticeEach l) (i j : Nat) (hi : i < l.length)
    (hj : j < l.length) (heq : sessionId l[i] = sessionId l[j]) : i = j := by
  have := (List.Nodup.getElem_inj_iff (l := l.map sessionId) h (hi := by simpa using hi)
    (hj := by simpa using hj)).mp (by simpa using heq)
  exact this

/-- After the session at `k` goes, no session has its id. -/
theorem erased_id_gone (l : List sessions.Session) (h : oneNoticeEach l) (k : Nat) (hk : k < l.length) :
    ∀ s ∈ l.eraseIdx k, sessionId s ≠ sessionId l[k] := by
  intro s hs heq
  obtain ⟨i, hi, hne, rfl⟩ := List.mem_eraseIdx_iff_getElem.mp hs
  exact hne (sid_unique l h i k hi hk heq)

theorem others_append_self (l : List sessions.Session) (s : sessions.Session) (e : sessions.Event)
    (hs : sessionId s = bytes e.session.val) : others (l ++ [s]) e = others l e := by
  simp [others, List.filter_append, hs]

theorem others_eraseIdx (l : List sessions.Session) (e : sessions.Event) (k : Nat) :
    (others l e).length ≤ (others (l.eraseIdx k) e).length + 1 := by
  rw [List.eraseIdx_eq_take_drop_succ]
  by_cases hk : k < l.length
  · conv => lhs; rw [← List.take_append_drop k l, List.drop_eq_getElem_cons hk]
    simp only [others, List.filter_append, List.length_append, List.filter_cons]
    split <;> (simp; try omega)
  · rw [List.drop_of_length_le (by omega), List.take_of_length_le (by omega)]
    simp

theorem nodup_snoc (l : List sessions.Session) (s : sessions.Session) (h : oneNoticeEach l)
    (hs : ∀ t ∈ l, sessionId t ≠ sessionId s) : oneNoticeEach (l ++ [s]) := by
  unfold oneNoticeEach at *
  rw [List.map_append, List.nodup_append]
  refine ⟨h, by simp, ?_⟩
  intro a ha b hb
  simp only [List.map_cons, List.map_nil, List.mem_singleton] at hb
  obtain ⟨t, ht, rfl⟩ := List.mem_map.mp ha
  rw [hb]
  exact hs t ht

/-! ## S41 -/

theorem noticeAfter_next (r : List sessions.Session) (e : sessions.Event) (hk : e.kind ≠ .SessionEnd)
    (s : sessions.Session) (hs : s ∈ r) (hok : nextOk e s) : noticeAfter r e := by
  unfold noticeAfter
  split
  · contradiction
  · exact ⟨s, hs, hok⟩

theorem noticeAfter_end (r : List sessions.Session) (e : sessions.Event) (hk : e.kind = .SessionEnd)
    (h : ∀ s ∈ r, sessionId s ≠ bytes e.session.val) : noticeAfter r e := by
  unfold noticeAfter
  rw [hk]
  exact h

theorem key_notice (s : sessions.Session) (h : s.notice ≠ none) : key s = 2 ^ 32 + noticeTime s := by
  unfold key noticeTime
  cases hn : s.notice
  · exact absurd hn h
  · rfl

theorem key_no_notice (s : sessions.Session) (h : s.notice = none) : key s < 2 ^ 32 := by
  unfold key
  rw [h]
  exact s.last.hBounds

/-- **S41.** -/
theorem apply_event_spec (ss : Slice sessions.Session) (e : sessions.Event) (now : U32)
    (hfit : sessionsFit ss.val) :
    sessions.apply_event ss e now ⦃ r =>
      r.val.length ≤ maxSessions ∧ oneNoticeEach r.val ∧ noticeAfter r.val e ∧ othersKept ss.val r.val e ⦄ := by
  obtain ⟨hlen, hnodup⟩ := hfit
  simp only [maxSessions] at hlen
  unfold sessions.apply_event
  step*
  · -- The end of a session.
    simp only [Protocol.Seen.deref_val] at at_post2 at_post3
    have hk : e.kind = .SessionEnd := b_post.mp (by assumption)
    rw [r_post]
    have hgone : ∀ s ∈ ss.val.eraseIdx «at».val, sessionId s ≠ bytes e.session.val := by
      by_cases hlt : «at».val < ss.val.length
      · have := erased_id_gone ss.val hnodup «at».val hlt
        rw [at_post2 hlt] at this
        exact this
      · rw [List.eraseIdx_of_length_le (by omega)]
        exact at_post3 (by omega)
    refine ⟨?_, nodup_eraseIdx _ _ hnodup, noticeAfter_end _ _ hk hgone, ?_⟩
    · have := List.length_eraseIdx_le ss.val «at».val
      simp only [maxSessions]; omega
    unfold othersKept
    refine ⟨fun s hs _ => mem_of_mem_eraseIdx hs, fun s hs hsid _ => ?_, others_eraseIdx _ _ _⟩
    left
    obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hs
    apply mem_eraseIdx_of_ne _ _ _ hj
    intro hja
    subst hja
    exact hsid (at_post2 hj)
  · -- Any other event.
    simp only [Protocol.Seen.deref_val] at at_post2 at_post3
    have hk : e.kind ≠ .SessionEnd := fun h => (by assumption : ¬ b = true) (b_post.mpr h)
    have hnext : sessionId next = bytes e.session.val := next_post.1
    have hafter : noticeAfter (ss.val.eraseIdx i1.val ++ [next]) e :=
      noticeAfter_next _ e hk next (by simp) next_post
    have hpart1 : ∀ s ∈ ss.val.eraseIdx i1.val ++ [next], sessionId s ≠ bytes e.session.val → s ∈ ss.val := by
      intro s hs hsid
      rcases List.mem_append.mp hs with hs | hs
      · exact mem_of_mem_eraseIdx hs
      · rw [List.mem_singleton] at hs
        subst hs
        exact absurd hnext hsid
    have hcount : (others ss.val e).length ≤ (others (ss.val.eraseIdx i1.val ++ [next]) e).length + 1 := by
      rw [others_append_self _ _ _ hnext]
      exact others_eraseIdx _ _ _
    rw [r_post]
    by_cases hlt : «at».val < ss.val.length
    · -- A known session: its old entry goes, the new one comes last.
      have hi1 : i1 = «at» := i1_post1 (Or.inl hlt)
      subst hi1
      have hgone := erased_id_gone ss.val hnodup i1.val hlt
      rw [at_post2 hlt] at hgone
      refine ⟨?_, nodup_snoc _ _ (nodup_eraseIdx _ _ hnodup) (fun t ht => by rw [hnext]; exact hgone t ht),
        hafter, ?_⟩
      · rw [List.length_append, List.length_eraseIdx_of_lt hlt]
        simp only [maxSessions, List.length_singleton]; omega
      unfold othersKept
      refine ⟨hpart1, fun s hs hsid _ => ?_, hcount⟩
      left
      obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hs
      apply List.mem_append_left
      apply mem_eraseIdx_of_ne _ _ _ hj
      intro hja
      subst hja
      exact hsid (at_post2 hj)
    · have hnone := at_post3 (by omega)
      have hsnoc : oneNoticeEach (ss.val.eraseIdx i1.val ++ [next]) :=
        nodup_snoc _ _ (nodup_eraseIdx _ _ hnodup)
          (fun t ht => by rw [hnext]; exact hnone t (mem_of_mem_eraseIdx ht))
      by_cases hroom : ss.val.length < 32
      · -- A new session in a table with room.
        have hi1 : i1 = «at» := i1_post1 (Or.inr hroom)
        subst hi1
        refine ⟨?_, hsnoc, hafter, ?_⟩
        · have := List.length_eraseIdx_le ss.val i1.val
          simp only [maxSessions, List.length_append, List.length_singleton]; omega
        unfold othersKept
        refine ⟨hpart1, fun s hs _ _ => ?_, hcount⟩
        left
        rw [List.eraseIdx_of_length_le (by omega)]
        exact List.mem_append_left _ hs
      · -- A new session in a full table: it takes the place of the session with the smallest key.
        obtain ⟨hv, hmin⟩ := i1_post2 (by omega) (by omega)
        refine ⟨?_, hsnoc, hafter, ?_⟩
        · rw [List.length_append, List.length_eraseIdx_of_lt hv]
          simp only [maxSessions, List.length_singleton]; omega
        unfold othersKept
        refine ⟨hpart1, fun s hs _ hsn => ?_, hcount⟩
        obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hs
        by_cases hjv : j = i1.val
        · right
          subst hjv
          have hks := key_notice _ hsn
          refine ⟨by simp only [maxSessions]; omega, fun t ht => ?_, fun t ht => ?_⟩
          · obtain ⟨m, hm, rfl⟩ := List.getElem_of_mem ht
            intro htn
            have := key_no_notice _ htn
            have := hmin m hm
            omega
          · obtain ⟨m, hm, rfl⟩ := List.getElem_of_mem ht
            have hkm := hmin m hm
            by_cases htn : ss.val[m].notice = none
            · have := key_no_notice _ htn; omega
            · have := key_notice _ htn; omega
        · left
          exact List.mem_append_left _ (mem_eraseIdx_of_ne _ _ _ hj hjv)

end Protocol.Sessions
