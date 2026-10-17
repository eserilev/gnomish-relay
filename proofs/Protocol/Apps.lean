import Protocol.Ascii
import Protocol.Spec.Apps

/-! # The app of a strip (S29), and the global names of each app -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Apps

theorem verifiedApps_take_succ (checks : List apps.TagCheck) (i : Nat) (hi : i < checks.length) :
    verifiedApps (checks.take (i + 1)) =
      verifiedApps (checks.take i) ++ (if checks[i].tag_ok then [checks[i].app] else []) := by
  rw [List.take_add_one, List.getElem?_eq_getElem hi]
  unfold verifiedApps
  rw [List.filterMap_append]
  by_cases hb : checks[i].tag_ok = true <;> simp [hb]

def RouteInv (checks : Slice apps.TagCheck) (st : Usize × Option apps.App × Usize) : Prop :=
  st.2.2.val ≤ checks.val.length ∧ st.1.val ≤ st.2.2.val ∧
  st.1.val = (verifiedApps (checks.val.take st.2.2.val)).length ∧
  st.2.1 = (verifiedApps (checks.val.take st.2.2.val)).getLast?

theorem route_loop_spec (checks : Slice apps.TagCheck) (verified : Usize) (found : Option apps.App)
    (i : Usize) (hinv : RouteInv checks (verified, found, i)) :
    apps.route_loop checks verified found i ⦃ r =>
      r.1.val = (verifiedApps checks.val).length ∧ r.2 = (verifiedApps checks.val).getLast? ⦄ := by
  unfold apps.route_loop
  apply loop.spec_decr_nat (fun st => checks.val.length - st.2.2.val) (RouteInv checks)
    _ _ _ _ hinv
  rintro ⟨verified, found, i⟩ ⟨hi, hle, hcount, hfound⟩
  simp only at hi hle hcount hfound
  unfold apps.route_loop.body
  step*
  · have hlt : i.val < checks.val.length := by scalar_tac
    have hstep := verifiedApps_take_succ checks.val i.val hlt
    rw [← tc_post] at hstep
    by_cases hok : tc.tag_ok = true
    · -- The key of this app verifies the tag.
      simp only [hok, if_true]
      rw [if_pos hok] at hstep
      step*
      refine ⟨⟨by scalar_tac, by scalar_tac, ?_, ?_⟩, by scalar_tac⟩
      · simp only [i2_post, hstep, List.length_append, ← hcount, x_post,
          List.length_singleton]
      · simp [i2_post, hstep]
    · -- It does not.
      simp only [hok, if_false, Bool.false_eq_true]
      rw [if_neg hok, List.append_nil] at hstep
      step*
      refine ⟨⟨by scalar_tac, by scalar_tac, ?_, ?_⟩, by scalar_tac⟩
      · simp only [i2_post, hstep, hcount]
      · simp only [i2_post, hstep, hfound]
  · -- Checked every key.
    have hn : i.val = checks.val.length := by scalar_tac
    rw [hn, List.take_length] at hcount hfound
    exact ⟨hcount, hfound⟩

/-- **S29.** -/
theorem route_spec (checks : Slice apps.TagCheck) :
    apps.route checks ⦃ r => match verifiedApps checks.val with
      | [] => r = .Err .BadTag
      | [app] => r = .Ok app
      | _ :: _ :: _ => r = .Err .Ambiguous ⦄ := by
  unfold apps.route
  step with route_loop_spec checks 0#usize none 0#usize (by simp [RouteInv, verifiedApps])
    as ⟨verified, found, hcount, hfound⟩
  unfold apps.verdict
  rcases h : verifiedApps checks.val with _ | ⟨app, _ | ⟨other, rest⟩⟩
  · -- No key verifies the tag.
    rw [h] at hfound
    simp [hfound]
  · -- One key verifies the tag.
    rw [h] at hcount hfound
    have hone : verified = 1#usize := UScalar.eq_of_val_eq (by simpa using hcount)
    simp [hfound, hone]
  · -- Two or more keys verify the tag.
    rw [h] at hcount hfound
    have hne : verified ≠ 1#usize := by
      intro hone
      simp [hone] at hcount
    obtain ⟨last, hlast⟩ : ∃ last, found = some last :=
      Option.isSome_iff_exists.mp (by rw [hfound]; simp)
    simp [hlast, hne]

/-- A fixed name goes out as its bytes. -/
theorem push_name_spec {n : Usize} (out : alloc.vec.Vec U8) (arr : Std.Array U8 n) (s : String)
    (hname : bytes (Array.to_slice arr).val = ascii s)
    (hroom : out.val.length + (Array.to_slice arr).val.length ≤ Usize.max) :
    ascii.push_bytes out (Array.to_slice arr) ⦃ r =>
      bytes r.val = bytes out.val ++ ascii s ∧ r.val.length = out.val.length + (ascii s).length ⦄ := by
  have hlen : (Array.to_slice arr).val.length = (ascii s).length := by
    have := congrArg List.length hname
    simpa [bytes] using this
  apply WP.spec_mono (push_bytes_spec out (Array.to_slice arr) hroom)
  intro r ⟨hr, hrlen⟩
  refine ⟨?_, by omega⟩
  rw [hr, ← hname]
  simp [bytes]

theorem slot_global_length (app : apps.App) : (ascii (slotGlobal app)).length ≤ 21 := by
  induction app <;> decide

theorem restore_global_length (app : apps.App) : (ascii (restoreGlobal app)).length ≤ 20 := by
  induction app <;> decide

theorem live_global_length (app : apps.App) : (ascii (liveGlobal app)).length ≤ 17 := by
  induction app <;> decide

@[step]
theorem push_slot_global_spec (out : alloc.vec.Vec U8) (app : apps.App)
    (hroom : out.val.length + 21 ≤ Usize.max) :
    apps.push_slot_global out app ⦃ r =>
      bytes r.val = bytes out.val ++ ascii (slotGlobal app) ∧
      r.val.length = out.val.length + (ascii (slotGlobal app)).length ⦄ := by
  unfold apps.push_slot_global
  induction app
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.RELAY_SLOT_DATA; rfl)
      (by unfold apps.RELAY_SLOT_DATA; simp; omega)
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.TIMEWAYS_SLOT_DATA; rfl)
      (by unfold apps.TIMEWAYS_SLOT_DATA; simp; omega)

@[step]
theorem push_restore_global_spec (out : alloc.vec.Vec U8) (app : apps.App)
    (hroom : out.val.length + 20 ≤ Usize.max) :
    apps.push_restore_global out app ⦃ r =>
      bytes r.val = bytes out.val ++ ascii (restoreGlobal app) ∧
      r.val.length = out.val.length + (ascii (restoreGlobal app)).length ⦄ := by
  unfold apps.push_restore_global
  induction app
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.RELAY_RESTORE; rfl)
      (by unfold apps.RELAY_RESTORE; simp; omega)
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.TIMEWAYS_RESTORE; rfl)
      (by unfold apps.TIMEWAYS_RESTORE; simp; omega)

@[step]
theorem push_live_global_spec (out : alloc.vec.Vec U8) (app : apps.App)
    (hroom : out.val.length + 17 ≤ Usize.max) :
    apps.push_live_global out app ⦃ r =>
      bytes r.val = bytes out.val ++ ascii (liveGlobal app) ∧
      r.val.length = out.val.length + (ascii (liveGlobal app)).length ⦄ := by
  unfold apps.push_live_global
  induction app
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.RELAY_LIVE; rfl)
      (by unfold apps.RELAY_LIVE; simp; omega)
  · simp only [lift, bind_tc_ok]
    exact push_name_spec out _ _ (by unfold apps.TIMEWAYS_LIVE; rfl)
      (by unfold apps.TIMEWAYS_LIVE; simp; omega)

end Protocol.Apps
