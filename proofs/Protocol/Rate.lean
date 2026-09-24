import Protocol.Seen
import Protocol.Spec.Rate

/-! # Rate limit and chat queue (S14) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Rate

@[simp, scalar_tac_simps]
theorem max_messages_val : rate.MAX_MESSAGES.val = 10 := by unfold rate.MAX_MESSAGES; rfl

@[simp, scalar_tac_simps]
theorem max_queue_val : rate.MAX_QUEUE.val = 20 := by unfold rate.MAX_QUEUE; rfl

@[simp, scalar_tac_simps]
theorem window_val : rate.WINDOW_SECONDS.val = 60 := by unfold rate.WINDOW_SECONDS; rfl

@[step]
theorem in_window_spec (t now : U32) :
    rate.in_window t now ⦃ r => (r = true ↔ t.val ≤ now.val ∧ now.val < t.val + 60) ⦄ := by
  unfold rate.in_window
  step*

def vals (l : List U32) : List Nat := l.map (·.val)

theorem still_counting_loop_spec (times : Slice U32) (now : U32) (kept : alloc.vec.Vec U32)
    (i : Usize) (hi : i.val ≤ times.val.length) (hk : kept.val.length ≤ i.val)
    (hkept : vals kept.val = inWindow now.val (vals (times.val.take i.val))) :
    rate.still_counting_loop times now kept i ⦃ out =>
      vals out.val = inWindow now.val (vals times.val) ∧ out.val.length ≤ times.val.length ⦄ := by
  unfold rate.still_counting_loop
  apply loop.spec_decr_nat (fun st => times.val.length - st.2.val)
    (fun st => st.2.val ≤ times.val.length ∧ st.1.val.length ≤ st.2.val ∧
      vals st.1.val = inWindow now.val (vals (times.val.take st.2.val)))
    _ _ _ _ ⟨hi, hk, hkept⟩
  rintro ⟨kept, i⟩ ⟨hi, hk, hkept⟩
  simp only at hi hk hkept
  unfold rate.still_counting_loop.body
  step*
  · have hlt : i.val < times.val.length := by scalar_tac
    have hstep : inWindow now.val (vals (times.val.take (i.val + 1))) =
        inWindow now.val (vals (times.val.take i.val)) ++
          (if i2.val ≤ now.val ∧ now.val < i2.val + 60 then [i2.val] else []) := by
      rw [List.take_add_one, List.getElem?_eq_getElem hlt, ← i2_post]
      simp [vals, inWindow, List.filter_append, windowSeconds, List.filter_cons]
    by_cases hb : b = true
    · simp only [hb, if_true]
      step*
      refine ⟨by scalar_tac, by scalar_tac, ?_, by scalar_tac⟩
      rw [i3_post, hstep, if_pos (b_post.mp hb), kept1_post, ← hkept]
      simp [vals]
    · simp only [hb, if_false, Bool.false_eq_true]
      step*
  · have : i.val = times.val.length := by scalar_tac
    rw [this, List.take_length] at hkept
    exact ⟨hkept, by omega⟩

@[step]
theorem still_counting_spec (times : Slice U32) (now : U32) :
    rate.still_counting times now ⦃ out =>
      vals out.val = inWindow now.val (vals times.val) ∧ out.val.length ≤ times.val.length ⦄ := by
  unfold rate.still_counting
  exact still_counting_loop_spec times now _ 0#usize (by simp) (by simp) (by simp [vals, inWindow])

/-- **S14, one step.** -/
theorem admit_message_spec (limiter : rate.RateLimiter) (now : U32)
    (_ : limiter.times.val.length ≤ maxMessages) :
    rate.admit_message limiter now ⦃ res =>
      (res.1, res.2.times.val.map (·.val)) = admitSpec (limiter.times.val.map (·.val)) now.val ⦄ := by
  unfold rate.admit_message
  step*
  · have hlen : (vals kept.val).length < maxMessages := by simp [vals, maxMessages]; scalar_tac
    simp only [Protocol.Seen.deref_val] at kept_post1
    rw [kept_post1] at hlen
    simp only [admitSpec, kept1_post]
    rw [if_pos (by simpa [vals] using hlen)]
    simp only [List.map_append, List.map_singleton, Prod.mk.injEq, true_and]
    rw [show List.map (fun x : U32 => x.val) kept.val = vals kept.val from rfl, kept_post1]
    rfl
  · have hlen : ¬ (vals kept.val).length < maxMessages := by simp [vals, maxMessages]; scalar_tac
    simp only [Protocol.Seen.deref_val] at kept_post1
    rw [kept_post1] at hlen
    simp only [admitSpec]
    rw [if_neg (by simpa [vals] using hlen)]
    rw [show List.map (fun x : U32 => x.val) kept.val = vals kept.val from rfl, kept_post1]
    rfl

/-- **S14, queue.** -/
theorem enqueue_spec (queue : rate.ChatQueue) (id : U32) (_ : queue.ids.val.length ≤ maxQueue) :
    rate.enqueue queue id ⦃ r => match r with
      | some q => q.ids.val = queue.ids.val ++ [id] ∧ q.ids.val.length ≤ maxQueue
      | none => queue.ids.val.length = maxQueue ⦄ := by
  unfold rate.enqueue
  step*
  all_goals simp_all [maxQueue]
  all_goals scalar_tac

/-! ## Any window (pure) -/

def step (acc : List Bool × List Nat) (now : Nat) : List Bool × List Nat :=
  let (admitted, times) := admitSpec acc.2 now
  (acc.1 ++ [admitted], times)

def state (nows : List Nat) : List Bool × List Nat := nows.foldl step ([], [])

theorem decisions_eq (nows : List Nat) : decisions nows = (state nows).1 := rfl

theorem state_snoc (p : List Nat) (m : Nat) :
    state (p ++ [m]) = ((state p).1 ++ [(admitSpec (state p).2 m).1], (admitSpec (state p).2 m).2) := by
  simp only [state, List.foldl_append, List.foldl_cons, List.foldl_nil]
  rfl

theorem state_length (p : List Nat) : (state p).1.length = p.length := by
  induction p using List.reverseRecOn with
  | nil => rfl
  | append_singleton p m ih => simp [state_snoc, ih]

theorem admitted_snoc (p : List Nat) (m : Nat) :
    admittedTimes (p ++ [m]) =
      admittedTimes p ++ (if (admitSpec (state p).2 m).1 then [m] else []) := by
  unfold admittedTimes
  rw [decisions_eq, decisions_eq, state_snoc, List.zip_append (by simp [state_length])]
  cases (admitSpec (state p).2 m).1 <;> simp

theorem admitted_mem (p : List Nat) : ∀ a ∈ admittedTimes p, a ∈ p := by
  induction p using List.reverseRecOn with
  | nil => intro a ha; simp [admittedTimes] at ha
  | append_singleton p m ih =>
    intro a ha
    rw [admitted_snoc, List.mem_append] at ha
    rcases ha with ha | ha
    · exact List.mem_append_left _ (ih a ha)
    · split at ha <;> simp_all

theorem inWindow_append (n : Nat) (l1 l2 : List Nat) :
    inWindow n (l1 ++ l2) = inWindow n l1 ++ inWindow n l2 := List.filter_append ..

/-- The limiter keeps exactly the admitted messages that still count. -/
theorem kept_eq (p : List Nat) (hp : p.Pairwise (· ≤ ·)) :
    ∀ n, (∀ x ∈ p, x ≤ n) →
      inWindow n (state p).2 = (admittedTimes p).filter fun a => n < a + windowSeconds := by
  induction p using List.reverseRecOn with
  | nil => intro n _; simp [inWindow, state, admittedTimes, decisions]
  | append_singleton p m ih =>
    intro n hn
    rw [List.pairwise_append] at hp
    have hm : ∀ x ∈ p, x ≤ m := fun x hx => hp.2.2 x hx m (by simp)
    have hmn : m ≤ n := hn m (by simp)
    have hkept := ih hp.1 m hm
    -- Everything admitted before `m` is at most `n`, and still counts at `m` if it counts at `n`.
    have hcollapse : ∀ l : List Nat, (∀ a ∈ l, a ≤ n) →
        inWindow n (l.filter fun a => m < a + windowSeconds) = l.filter fun a => n < a + windowSeconds := by
      intro l hl
      unfold inWindow
      rw [List.filter_filter]
      apply List.filter_congr
      intro a ha
      have := hl a ha
      apply Bool.eq_iff_iff.mpr
      simp only [windowSeconds, Bool.and_eq_true, decide_eq_true_eq]
      omega
    have hle : ∀ a ∈ admittedTimes p, a ≤ n := fun a ha => le_trans (hm a (admitted_mem p a ha)) hmn
    rw [state_snoc, admitted_snoc]
    simp only [admitSpec]
    split <;> rename_i h
    · simp only [if_true]
      rw [inWindow_append, hkept, hcollapse _ hle, List.filter_append]
      simp only [inWindow, List.filter_cons, List.filter_nil, hmn, true_and]
    · simp only [Bool.false_eq_true, if_false, List.append_nil]
      rw [hkept, hcollapse _ hle]

/-- **S14, any time window.** -/
theorem window_spec (nows : List Nat) (hs : nows.Pairwise (· ≤ ·)) (t : Nat) :
    ((admittedTimes nows).filter fun a => a ≤ t ∧ t < a + windowSeconds).length ≤ maxMessages := by
  induction nows using List.reverseRecOn generalizing t with
  | nil => simp [admittedTimes, decisions, maxMessages]
  | append_singleton p m ih =>
    have hp := hs
    rw [List.pairwise_append] at hp
    have hm : ∀ x ∈ p, x ≤ m := fun x hx => hp.2.2 x hx m (by simp)
    rw [admitted_snoc]
    split <;> rename_i hok
    · rw [List.filter_append]
      by_cases hmt : m ≤ t ∧ t < m + windowSeconds
      · -- The window at `t` holds `m`, and each earlier message in it still counted at `m`.
        have hlen : (inWindow m (state p).2).length < maxMessages := by
          simp only [admitSpec] at hok; split at hok <;> simp_all
        rw [kept_eq p hp.1 m hm] at hlen
        have hsub : ((admittedTimes p).filter fun a => a ≤ t ∧ t < a + windowSeconds).length ≤
            ((admittedTimes p).filter fun a => m < a + windowSeconds).length := by
          apply List.Sublist.length_le
          apply List.monotone_filter_right
          intro a ha
          simp only [decide_eq_true_eq, windowSeconds] at *
          omega
        rw [List.filter_cons_of_pos (by simpa using hmt), List.filter_nil, List.length_append,
          List.length_singleton]
        have := ih hp.1 t
        omega
      · rw [List.filter_cons_of_neg (by simpa using hmt), List.filter_nil, List.append_nil]
        exact ih hp.1 t
    · rw [List.append_nil]
      exact ih hp.1 t

end Protocol.Rate
