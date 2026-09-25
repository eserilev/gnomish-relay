import Protocol.Ascii
import Protocol.Spec.Apps

/-! # The global names of each app -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Apps

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
