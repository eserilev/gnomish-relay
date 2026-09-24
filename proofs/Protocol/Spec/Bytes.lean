import Aeneas

/-!
# Byte helpers for the specs

A byte is a `BitVec 8`, the same as the `bv` field of an Aeneas `U8`.
-/

open Aeneas Aeneas.Std

namespace Protocol.Spec

abbrev Byte := BitVec 8

/-- The bytes of a Rust `&[u8]` or `Vec<u8>`. -/
def bytes (l : List U8) : List Byte := l.map (·.bv)

def ch (c : Char) : Byte := BitVec.ofNat 8 c.toNat

def ascii (s : String) : List Byte := s.toList.map ch

/-- Decimal digits, no sign, no leading zero. `decimal 0 = "0"`. -/
def decimal (n : Nat) : List Byte := (Nat.toDigits 10 n).map ch

def be16 (n : Nat) : List Byte := [BitVec.ofNat 8 (n / 256), BitVec.ofNat 8 n]

def be32 (n : Nat) : List Byte :=
  [BitVec.ofNat 8 (n / 2 ^ 24), BitVec.ofNat 8 (n / 2 ^ 16), BitVec.ofNat 8 (n / 256),
    BitVec.ofNat 8 n]

/-- Fletcher-16, as in `wow-claude`'s `Codec.lua`: two sums mod 255, low sum first. -/
def fletcher16 (l : List Byte) : List Byte :=
  let (s1, s2) := l.foldl (fun (s : Nat × Nat) b =>
    let s1 := (s.1 + b.toNat) % 255
    (s1, (s.2 + s1) % 255)) (0, 0)
  [BitVec.ofNat 8 s1, BitVec.ofNat 8 s2]

end Protocol.Spec
