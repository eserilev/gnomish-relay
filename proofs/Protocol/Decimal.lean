import Protocol.Ascii

/-!
# Canonical decimal numbers

`parse_decimal` accepts exactly the output of `decimal`. The key fact is that a
digit string with no leading zero is the `decimal` of its value, so a parsed
number rebuilds the exact input bytes.
-/

open Protocol.Spec Protocol.Ascii

namespace Protocol.Decimal

def isDigitB (b : Spec.Byte) : Bool := 48 ≤ b.toNat ∧ b.toNat ≤ 57

/-- The value of a digit string, or `none` for a non-digit. -/
def digitsVal (l : List Spec.Byte) : Option Nat :=
  l.foldl (fun acc b => acc.bind fun v => if isDigitB b then some (10 * v + (b.toNat - 48)) else none)
    (some 0)

theorem digitsVal_append_one (g : List Spec.Byte) (d : Spec.Byte) :
    digitsVal (g ++ [d]) =
      (digitsVal g).bind fun v => if isDigitB d then some (10 * v + (d.toNat - 48)) else none := by
  simp [digitsVal, List.foldl_append]

theorem digitsVal_nil : digitsVal [] = some 0 := rfl

theorem digit_byte_toNat (d : Nat) (h : d < 10) : (BitVec.ofNat 8 (48 + d)).toNat = 48 + d := by
  simp; omega

theorem digitsVal_decimal (m : Nat) : digitsVal (decimal m) = some m := by
  induction m using Nat.strong_induction_on with
  | _ m ih =>
    rw [decimal_eq, digitsVal_append_one]
    have hd := digit_byte_toNat (m % 10) (Nat.mod_lt _ (by omega))
    split
    · rename_i hm
      simp [digitsVal_nil, isDigitB, hd]
      omega
    · rename_i hm
      rw [ih (m / 10) (by omega)]
      simp [isDigitB, hd]
      omega

theorem decimal_ne_nil (m : Nat) : decimal m ≠ [] := by
  rw [decimal_eq]; simp

theorem decimal_head_eq (m : Nat) (h : 10 ≤ m) : (decimal m).head? = (decimal (m / 10)).head? := by
  rw [decimal_eq m, if_neg (by omega), List.head?_append]
  cases hd : (decimal (m / 10)).head? with
  | none => simp at hd; exact absurd hd (decimal_ne_nil _)
  | some x => rfl

theorem decimal_head_ne_zero (m : Nat) (h : 1 ≤ m) : (decimal m).head? ≠ some (ch '0') := by
  induction m using Nat.strong_induction_on with
  | _ m ih =>
    by_cases h10 : m < 10
    · rw [decimal_eq, if_pos h10]
      simp only [List.nil_append, List.head?_cons, ne_eq, Option.some.injEq]
      intro heq
      have := congrArg BitVec.toNat heq
      simp [ch] at this
      omega
    · rw [decimal_head_eq m (by omega)]
      exact ih (m / 10) (by omega) (by omega)

theorem decimal_length_one (m : Nat) (h : m < 10) : (decimal m).length = 1 := by
  rw [decimal_eq, if_pos h]; simp

/-- A decimal with two or more digits does not start with `0`. -/
theorem decimal_head (m : Nat) (h : 1 < (decimal m).length) : (decimal m).head? ≠ some (ch '0') := by
  apply decimal_head_ne_zero
  by_contra hm
  rw [decimal_length_one m (by omega)] at h
  omega

theorem decimal_zero : decimal 0 = [ch '0'] := rfl

/-- **Uniqueness.** A digit string with no leading zero is the `decimal` of its value. -/
theorem decimal_digitsVal (g : List Spec.Byte) (hne : g ≠ [])
    (hlead : 1 < g.length → g.head? ≠ some (ch '0')) (v : Nat) (hv : digitsVal g = some v) :
    decimal v = g := by
  induction g using List.reverseRecOn generalizing v with
  | nil => exact absurd rfl hne
  | append_singleton h d ih =>
    rw [digitsVal_append_one] at hv
    cases hh : digitsVal h with
    | none => simp [hh] at hv
    | some vh =>
      simp only [hh, Option.bind_some] at hv
      split at hv
      · rename_i hd
        simp only [Option.some.injEq] at hv
        simp only [isDigitB, decide_eq_true_eq] at hd
        have hdig : d = BitVec.ofNat 8 (48 + (d.toNat - 48)) := by
          apply BitVec.eq_of_toNat_eq; simp; omega
        by_cases hnil : h = []
        · -- One digit.
          subst hnil
          simp [digitsVal_nil] at hh
          subst hh
          rw [decimal_eq, if_pos (by omega)]
          simp only [List.nil_append, List.cons.injEq, and_true]
          rw [hdig]; congr 1; omega
        · -- More digits: the prefix has no leading zero either.
          have hlead' : 1 < h.length → h.head? ≠ some (ch '0') := by
            intro hl
            have := hlead (by simp; omega)
            rwa [List.head?_append_of_ne_nil _ hnil] at this
          have hdec := ih hnil hlead' vh hh
          have hvh : 1 ≤ vh := by
            by_contra h0
            have : vh = 0 := by omega
            subst this
            rw [decimal_zero] at hdec
            have := hlead (by rw [← hdec]; simp)
            rw [← hdec] at this
            simp at this
          rw [decimal_eq, if_neg (by omega), ← hv]
          rw [show (10 * vh + (d.toNat - 48)) / 10 = vh by omega, hdec]
          congr 1
          simp only [List.cons.injEq, and_true]
          apply BitVec.eq_of_toNat_eq
          simp
          omega
      · simp at hv

theorem digitsVal_none_append (g t : List Spec.Byte) (h : digitsVal g = none) :
    digitsVal (g ++ t) = none := by
  induction t using List.reverseRecOn with
  | nil => simpa using h
  | append_singleton t d ih =>
    rw [← List.append_assoc, digitsVal_append_one, ih]; rfl

theorem digitsVal_lt (g : List Spec.Byte) (v : Nat) (h : digitsVal g = some v) : v < 10 ^ g.length := by
  induction g using List.reverseRecOn generalizing v with
  | nil => simp [digitsVal_nil] at h; subst h; simp
  | append_singleton g d ih =>
    rw [digitsVal_append_one] at h
    cases hg : digitsVal g with
    | none => simp [hg] at h
    | some vg =>
      simp only [hg, Option.bind_some] at h
      split at h
      · rename_i hd
        simp only [Option.some.injEq] at h
        simp only [isDigitB, decide_eq_true_eq] at hd
        have := ih vg hg
        simp only [List.length_append, List.length_singleton, pow_succ]
        omega
      · simp at h

end Protocol.Decimal
