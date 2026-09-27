import Protocol.Code.Funs

/-!
# Public addresses (S34)

An address is a number: 32 bits for IPv4, 128 bits for IPv6. A range is a CIDR block,
kept as its first and its last address. The tables follow the special-purpose
registries of IANA, as SPEC 6.6.4 lists them.
-/

open Aeneas Aeneas.Std

namespace Protocol.Spec

/-- The block of `w`-bit numbers that starts at `net` and shares its first `bits` bits:
its first and its last number. -/
def cidr (w net bits : Nat) : Nat × Nat := (net, net + 2 ^ (w - bits) - 1)

def inRanges (rs : List (Nat × Nat)) (x : Nat) : Prop := ∃ r ∈ rs, r.1 ≤ x ∧ x ≤ r.2

/-- `a.b.c.d` as a number. -/
def v4 (a b c d : Nat) : Nat := a * 2 ^ 24 + b * 2 ^ 16 + c * 2 ^ 8 + d

/-- The eight segments of IPv6, the first one on top, as a number. -/
def v6 (s0 s1 s2 s3 s4 s5 s6 s7 : Nat) : Nat :=
  s0 * 2 ^ 112 + s1 * 2 ^ 96 + s2 * 2 ^ 80 + s3 * 2 ^ 64 + s4 * 2 ^ 48 + s5 * 2 ^ 32 +
    s6 * 2 ^ 16 + s7

def v4NotPublic : List (Nat × Nat) := [
  cidr 32 (v4 0 0 0 0) 8,
  cidr 32 (v4 10 0 0 0) 8,
  cidr 32 (v4 100 64 0 0) 10,
  cidr 32 (v4 127 0 0 0) 8,
  cidr 32 (v4 169 254 0 0) 16,
  cidr 32 (v4 172 16 0 0) 12,
  cidr 32 (v4 192 0 0 0) 24,
  cidr 32 (v4 192 0 2 0) 24,
  cidr 32 (v4 192 88 99 0) 24,
  cidr 32 (v4 192 168 0 0) 16,
  cidr 32 (v4 198 18 0 0) 15,
  cidr 32 (v4 198 51 100 0) 24,
  cidr 32 (v4 203 0 113 0) 24,
  cidr 32 (v4 224 0 0 0) 3]

def v6NotPublic : List (Nat × Nat) := [
  cidr 128 (v6 0 0 0 0 0 0 0 0) 16,
  cidr 128 (v6 0x100 0 0 0 0 0 0 0) 16,
  cidr 128 (v6 0x2001 0 0 0 0 0 0 0) 23,
  cidr 128 (v6 0x2001 0xdb8 0 0 0 0 0 0) 32,
  cidr 128 (v6 0x64 0xff9b 0 0 0 0 0 0) 32,
  cidr 128 (v6 0xfc00 0 0 0 0 0 0 0) 7,
  cidr 128 (v6 0xfe80 0 0 0 0 0 0 0) 10,
  cidr 128 (v6 0xfec0 0 0 0 0 0 0 0) 10,
  cidr 128 (v6 0xff00 0 0 0 0 0 0 0) 8]

/-- The IPv4 address that an IPv6 form holds: `::ffff:0:0/96` (IPv4-mapped) and
`64:ff9b::/96` (NAT64) in their last 32 bits, and `2002::/16` (6to4) in the 32 bits after
its first 16. -/
def embeddedV4 (x : Nat) : Option Nat :=
  if x / 2 ^ 32 = 0xffff then some (x % 2 ^ 32)
  else if x / 2 ^ 32 = v6 0x64 0xff9b 0 0 0 0 0 0 / 2 ^ 32 then some (x % 2 ^ 32)
  else if x / 2 ^ 112 = 0x2002 then some (x / 2 ^ 80 % 2 ^ 32)
  else none

def v4Nat (o : Array U8 4#usize) : Nat :=
  v4 (o.val.getD 0 0#u8).val (o.val.getD 1 0#u8).val (o.val.getD 2 0#u8).val
    (o.val.getD 3 0#u8).val

def v6Nat (s : Array U16 8#usize) : Nat :=
  v6 (s.val.getD 0 0#u16).val (s.val.getD 1 0#u16).val (s.val.getD 2 0#u16).val
    (s.val.getD 3 0#u16).val (s.val.getD 4 0#u16).val (s.val.getD 5 0#u16).val
    (s.val.getD 6 0#u16).val (s.val.getD 7 0#u16).val

end Protocol.Spec
