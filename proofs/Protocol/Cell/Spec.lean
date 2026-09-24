import Aeneas
import Mathlib.Tactic.IntervalCases

/-!
# What the cell codec means

Pure definitions, with no Rust in them. `Proofs.lean` shows that the Rust code
computes exactly these. Bytes and cells are `BitVec 8` here, the same as the
`bv` field of an Aeneas `U8`.
-/

open Aeneas Aeneas.Std

namespace Protocol.Cell

/-! ## One group: three bytes, eight cells -/

/-- The 24 bits of a group: `a` on top, `c` at the bottom. -/
def groupBits (a b c : BitVec 8) : BitVec 32 :=
  a.setWidth 32 <<< 16 ||| b.setWidth 32 <<< 8 ||| c.setWidth 32

def cellAt (bits : BitVec 32) (shift : Nat) : BitVec 8 :=
  ((bits >>> shift) &&& 7).setWidth 8

def encodeGroupBv (a b c : BitVec 8) : List (BitVec 8) :=
  [21, 18, 15, 12, 9, 6, 3, 0].map (cellAt (groupBits a b c))

/-- Appends cells to the low end of `acc`, 3 bits each. -/
def appendCells (acc : BitVec 32) (cells : List (BitVec 8)) : BitVec 32 :=
  cells.foldl (fun acc c => acc <<< 3 ||| c.setWidth 32) acc

def decodeGroupBv (cells : List (BitVec 8)) : List (BitVec 8) :=
  let bits := appendCells 0 cells
  [(bits >>> 16).setWidth 8, (bits >>> 8).setWidth 8, bits.setWidth 8]

/-- A cell from a real image is 0 to 7. Anything else is a bug or a forgery. -/
def cellsValid (cells : List U8) : Prop := ∀ c ∈ cells, c.val ≤ 7

theorem group_bits_round_trip (a b c : BitVec 8) :
    decodeGroupBv (encodeGroupBv a b c) = [a, b, c] := by
  simp only [decodeGroupBv, encodeGroupBv, appendCells, cellAt, groupBits, List.map_cons,
    List.map_nil, List.foldl_cons, List.foldl_nil, List.cons.injEq, and_true]
  -- Compare bit by bit, so the kernel checks it and we need no SAT solver.
  refine ⟨?_, ?_, ?_⟩ <;>
  · ext i hi
    interval_cases i <;> simp

theorem cellAt_le_seven (bits : BitVec 32) (shift : Nat) : (cellAt bits shift).toNat ≤ 7 := by
  simp only [cellAt, BitVec.toNat_setWidth, BitVec.toNat_and]
  exact le_trans (Nat.mod_le _ _) (by simpa using Nat.and_le_right)

/-! ## Many groups -/

/-- Three bytes at a time. The last group is padded with zero bytes. -/
def encodeBytesBv : List (BitVec 8) → List (BitVec 8)
  | [] => []
  | [a] => encodeGroupBv a 0 0
  | [a, b] => encodeGroupBv a b 0
  | a :: b :: c :: rest => encodeGroupBv a b c ++ encodeBytesBv rest

/-- Eight cells at a time. A partial group at the end is ignored. -/
def decodeAllBv : List (BitVec 8) → List (BitVec 8)
  | c0 :: c1 :: c2 :: c3 :: c4 :: c5 :: c6 :: c7 :: rest =>
    decodeGroupBv [c0, c1, c2, c3, c4, c5, c6, c7] ++ decodeAllBv rest
  | _ => []

/-- Zero bytes that fill up the last group. -/
def padLength (n : Nat) : Nat := (3 - n % 3) % 3

theorem encodeBytesBv_of_ne_nil (l : List (BitVec 8)) (h : l ≠ []) :
    encodeBytesBv l =
      encodeGroupBv (l.getD 0 0) (l.getD 1 0) (l.getD 2 0) ++ encodeBytesBv (l.drop 3) := by
  match l, h with
  | [a], _ => simp [encodeBytesBv]
  | [a, b], _ => simp [encodeBytesBv]
  | a :: b :: c :: rest, _ => simp [encodeBytesBv]

/-- One step of the encoder loop. -/
theorem encodeBytesBv_drop (l : List (BitVec 8)) (i : Nat) (h : i < l.length) :
    encodeBytesBv (l.drop i) =
      encodeGroupBv l[i] (l.getD (i + 1) 0) (l.getD (i + 2) 0) ++
        encodeBytesBv (l.drop (i + 3)) := by
  rw [encodeBytesBv_of_ne_nil _ (by simp; omega)]
  simp [List.getD_eq_getElem?_getD, List.drop_drop, List.getElem?_eq_getElem h]

theorem decodeAllBv_of_length (d : List (BitVec 8)) (h : 8 ≤ d.length) :
    decodeAllBv d = decodeGroupBv (d.take 8) ++ decodeAllBv (d.drop 8) := by
  rcases d with _ | ⟨c0, _ | ⟨c1, _ | ⟨c2, _ | ⟨c3, _ | ⟨c4, _ | ⟨c5, _ | ⟨c6, _ | ⟨c7, rest⟩⟩⟩⟩⟩⟩⟩⟩
  all_goals simp at h
  simp [decodeAllBv]

/-- One step of the decoder loop. -/
theorem decodeAllBv_drop (l : List (BitVec 8)) (i : Nat) (h : i + 8 ≤ l.length) :
    decodeAllBv (l.drop i) =
      decodeGroupBv ((l.drop i).take 8) ++ decodeAllBv (l.drop (i + 8)) := by
  rw [decodeAllBv_of_length _ (by simp; omega), List.drop_drop, Nat.add_comm]

theorem decodeAllBv_group_append (a b c : BitVec 8) (rest : List (BitVec 8)) :
    decodeAllBv (encodeGroupBv a b c ++ rest) = [a, b, c] ++ decodeAllBv rest := by
  rw [← group_bits_round_trip a b c]
  rfl

theorem decode_encode_bytes (l : List (BitVec 8)) :
    decodeAllBv (encodeBytesBv l) = l ++ List.replicate (padLength l.length) 0 := by
  induction l using encodeBytesBv.induct with
  | case1 => rfl
  | case2 a =>
    simpa [encodeBytesBv, padLength, decodeAllBv] using decodeAllBv_group_append a 0 0 []
  | case3 a b =>
    simpa [encodeBytesBv, padLength, decodeAllBv] using decodeAllBv_group_append a b 0 []
  | case4 a b c rest ih =>
    rw [encodeBytesBv, decodeAllBv_group_append, ih]
    simp [padLength]
    omega

theorem encodeBytesBv_valid (l : List (BitVec 8)) : ∀ x ∈ encodeBytesBv l, x.toNat ≤ 7 := by
  induction l using encodeBytesBv.induct with
  | case1 => simp [encodeBytesBv]
  | case2 a | case3 a b =>
    intro x hx
    simp only [encodeBytesBv, encodeGroupBv, List.mem_map] at hx
    obtain ⟨s, -, rfl⟩ := hx
    exact cellAt_le_seven _ _
  | case4 a b c rest ih =>
    intro x hx
    simp only [encodeBytesBv, List.mem_append] at hx
    rcases hx with hx | hx
    · simp only [encodeGroupBv, List.mem_map] at hx
      obtain ⟨s, -, rfl⟩ := hx
      exact cellAt_le_seven _ _
    · exact ih x hx

theorem encodeBytesBv_length (l : List (BitVec 8)) :
    (encodeBytesBv l).length = 8 * ((l.length + 2) / 3) := by
  induction l using encodeBytesBv.induct with
  | case1 => rfl
  | case2 a => simp [encodeBytesBv, encodeGroupBv]
  | case3 a b => simp [encodeBytesBv, encodeGroupBv]
  | case4 a b c rest ih => simp [encodeBytesBv, encodeGroupBv, ih]; omega

/-! ## List helpers -/

theorem take_eight_drop {α : Type} (l : List α) (i : Nat) (h : i + 8 ≤ l.length) :
    (l.drop i).take 8 =
      [l[i], l[i + 1], l[i + 2], l[i + 3], l[i + 4], l[i + 5], l[i + 6], l[i + 7]] := by
  apply List.ext_getElem
  · simp; omega
  · intro k hk _
    simp only [List.length_take, List.length_drop] at hk
    have : k < 8 := by omega
    interval_cases k <;> simp
    -- `simp` leaves `l[i + 7] = l[i + 7]`, with the 7 built two different ways.
    rfl

end Protocol.Cell
