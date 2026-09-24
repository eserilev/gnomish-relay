import Protocol.Ascii
import Protocol.Spec.Frame

/-! # The frame encoder and decoder (C2, S1, S2) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Frame

@[simp, scalar_tac_simps, grind =, agrind =]
theorem header_len_val : frame.HEADER_LEN.val = 11 := by unfold frame.HEADER_LEN; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem checksum_len_val : frame.CHECKSUM_LEN.val = 2 := by unfold frame.CHECKSUM_LEN; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem tag_len_val : frame.TAG_LEN.val = 8 := by unfold frame.TAG_LEN; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem max_payload_val : frame.MAX_PAYLOAD.val = 3200 := by unfold frame.MAX_PAYLOAD; rfl
@[simp, scalar_tac_simps, grind =, agrind =]
theorem version_val : frame.VERSION.val = 1 := by unfold frame.VERSION; rfl

theorem magic_bytes : bytes (Array.to_slice frame.MAGIC).val = [0x6E, 0x52] := by
  unfold frame.MAGIC; rfl

@[simp, scalar_tac_simps]
theorem magic_length : (Array.to_slice frame.MAGIC).val.length = 2 := by unfold frame.MAGIC; rfl

theorem byte_bv (x : U8) (n : Nat) (h : x.val = n % 256) : x.bv = BitVec.ofNat 8 n := by
  rw [U8_bv_eq_ofNat, h]; apply BitVec.eq_of_toNat_eq; simp

@[step]
theorem push_be16_spec (out : alloc.vec.Vec U8) (n : U16) (hroom : out.val.length + 2 ≤ Usize.max) :
    frame.push_be16 out n ⦃ r =>
      bytes r.val = bytes out.val ++ be16 n.val ∧ r.val.length = out.val.length + 2 ⦄ := by
  unfold frame.push_be16
  step*
  all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
  have h1 := byte_bv i1 (n.val / 256) (by simp_all [UScalar.cast_val_eq]; omega)
  have h2 := byte_bv i2 n.val (by simp_all [UScalar.cast_val_eq])
  refine ⟨?_, by simp [*]⟩
  simp [r_post, out1_post, bytes, be16, h1, h2]

@[step]
theorem push_be32_spec (out : alloc.vec.Vec U8) (n : U32) (hroom : out.val.length + 4 ≤ Usize.max) :
    frame.push_be32 out n ⦃ r =>
      bytes r.val = bytes out.val ++ be32 n.val ∧ r.val.length = out.val.length + 4 ⦄ := by
  unfold frame.push_be32
  step*
  all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
  have h1 := byte_bv i1 (n.val / 2 ^ 24) (by simp_all [UScalar.cast_val_eq, Nat.shiftRight_eq_div_pow])
  have h3 := byte_bv i3 (n.val / 2 ^ 16) (by simp_all [UScalar.cast_val_eq, Nat.shiftRight_eq_div_pow])
  have h5 := byte_bv i5 (n.val / 256) (by simp_all [UScalar.cast_val_eq, Nat.shiftRight_eq_div_pow])
  have h6 := byte_bv i6 n.val (by simp_all [UScalar.cast_val_eq])
  refine ⟨?_, by simp [*]⟩
  simp [r_post, out3_post, out2_post, out1_post, bytes, be32, h1, h3, h5, h6]

end Protocol.Frame

namespace Protocol.Frame

theorem shift_or_add (a b i : Nat) (hb : b < 2 ^ i) : (a <<< i) ||| b = a * 2 ^ i + b := by
  rw [← Nat.shiftLeft_add_eq_or_of_lt hb, Nat.shiftLeft_eq]

/-- Two bytes, high byte first, as `read_be16` builds them. -/
theorem be16_value (a b : Nat) (ha : a < 256) (hb : b < 256) :
    (a % 2 ^ 16) <<< 8 % 2 ^ 16 ||| b % 2 ^ 16 = a * 256 + b := by
  rw [Nat.mod_eq_of_lt (by omega : a < 2 ^ 16), Nat.mod_eq_of_lt (by omega : b < 2 ^ 16),
    Nat.shiftLeft_eq, Nat.mod_eq_of_lt (by omega : a * 2 ^ 8 < 2 ^ 16)]
  have := shift_or_add a b 8 (by omega)
  rw [Nat.shiftLeft_eq] at this
  rw [this]
  norm_num

theorem or_add_of_mod (x y i : Nat) (hx : x % 2 ^ i = 0) (hy : y < 2 ^ i) : x ||| y = x + y := by
  have hx' : x = 2 ^ i * (x / 2 ^ i) := by
    have := Nat.div_add_mod x (2 ^ i); omega
  rw [hx', ← Nat.two_pow_add_eq_or_of_lt hy]

/-- Four bytes, high byte first, as `read_be32` builds them. -/
theorem be32_value (a b c d : Nat) (ha : a < 256) (hb : b < 256) (hc : c < 256) (hd : d < 256) :
    (a % 2 ^ 32) <<< 24 % 2 ^ 32 ||| (b % 2 ^ 32) <<< 16 % 2 ^ 32 |||
      (c % 2 ^ 32) <<< 8 % 2 ^ 32 ||| d % 2 ^ 32 =
      a * 2 ^ 24 + b * 2 ^ 16 + c * 2 ^ 8 + d := by
  simp only [Nat.shiftLeft_eq, Nat.mod_eq_of_lt (by omega : a < 2 ^ 32),
    Nat.mod_eq_of_lt (by omega : b < 2 ^ 32), Nat.mod_eq_of_lt (by omega : c < 2 ^ 32),
    Nat.mod_eq_of_lt (by omega : d < 2 ^ 32), Nat.mod_eq_of_lt (by omega : a * 2 ^ 24 < 2 ^ 32),
    Nat.mod_eq_of_lt (by omega : b * 2 ^ 16 < 2 ^ 32), Nat.mod_eq_of_lt (by omega : c * 2 ^ 8 < 2 ^ 32)]
  rw [or_add_of_mod (a * 2 ^ 24) (b * 2 ^ 16) 24 (by simp) (by omega),
    or_add_of_mod (a * 2 ^ 24 + b * 2 ^ 16) (c * 2 ^ 8) 16 (by omega) (by omega),
    or_add_of_mod (a * 2 ^ 24 + b * 2 ^ 16 + c * 2 ^ 8) d 8 (by omega) (by omega)]

theorem getElem!_eq_getElem' (l : List U8) (i : Nat) (h : i < l.length) : l[i]! = l[i] := by
  simp [getElem!_pos, h]

@[step]
theorem read_be16_spec (src : Slice U8) (at_ : Usize) (h : at_.val + 1 < src.val.length) :
    frame.read_be16 src at_ ⦃ r =>
      r.val = src.val[at_.val]!.val * 256 + src.val[at_.val + 1]!.val ⦄ := by
  unfold frame.read_be16
  step*
  have ha := i.hBounds
  have hb := i4.hBounds
  simp only [UScalarTy.U8_numBits_eq] at ha hb
  have e4 : src.val[at_.val + 1]'(by omega) = i4 := by
    rw [i4_post]; congr 1; simp [i3_post]
  rw [getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega), ← i_post, e4]
  simp only [UScalar.val_or, i2_post1, i1_post, i5_post, UScalar.cast_val_eq, UScalarTy.U16_numBits_eq,
    U16.size, U16.numBits]
  exact be16_value _ _ ha hb

@[step]
theorem read_be32_spec (src : Slice U8) (at_ : Usize) (h : at_.val + 3 < src.val.length) :
    frame.read_be32 src at_ ⦃ r =>
      r.val = src.val[at_.val]!.val * 2 ^ 24 + src.val[at_.val + 1]!.val * 2 ^ 16 +
        src.val[at_.val + 2]!.val * 2 ^ 8 + src.val[at_.val + 3]!.val ⦄ := by
  unfold frame.read_be32
  step*
  have h0 := i.hBounds
  have h1 := i4.hBounds
  have h2 := i9.hBounds
  have h3 := i14.hBounds
  simp only [UScalarTy.U8_numBits_eq] at h0 h1 h2 h3
  have e1 : src.val[at_.val + 1]'(by omega) = i4 := by rw [i4_post]; congr 1; simp [i3_post]
  have e2 : src.val[at_.val + 2]'(by omega) = i9 := by rw [i9_post]; congr 1; simp [i8_post]
  have e3 : src.val[at_.val + 3]'(by omega) = i14 := by rw [i14_post]; congr 1; simp [i13_post]
  rw [getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega),
    getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega), ← i_post, e1, e2, e3]
  simp only [UScalar.val_or, i12_post1, i7_post1, i2_post1, i6_post1, i11_post1, i1_post, i5_post,
    i10_post, i15_post, UScalar.cast_val_eq, UScalarTy.U32_numBits_eq, U32.size, U32.numBits]
  exact be32_value _ _ _ _ h0 h1 h2 h3

/-- The two running sums of Fletcher-16, as in `Spec.fletcher16`. -/
def fletcherSums (l : List Spec.Byte) : Nat × Nat :=
  l.foldl (fun (s : Nat × Nat) b =>
    let s1 := (s.1 + b.toNat) % 255
    (s1, (s.2 + s1) % 255)) (0, 0)

theorem fletcher16_eq (l : List Spec.Byte) :
    Spec.fletcher16 l = [BitVec.ofNat 8 (fletcherSums l).1, BitVec.ofNat 8 (fletcherSums l).2] := rfl

def FletcherInv (src : Slice U8) (start : Nat) (st : U16 × U16 × Usize) : Prop :=
  start ≤ st.2.2.val ∧ st.1.val < 255 ∧ st.2.1.val < 255 ∧
  (st.1.val, st.2.1.val) = fletcherSums (bytes ((src.val.drop start).take (st.2.2.val - start)))

theorem fletcher16_loop_spec (src : Slice U8) (start stop : Usize) (s1 s2 : U16) (i : Usize)
    (hstop : stop.val ≤ src.val.length) (hi : i.val ≤ stop.val)
    (hinv : FletcherInv src start.val (s1, s2, i)) :
    frame.fletcher16_loop src stop s1 s2 i ⦃ r =>
      r.1.val < 255 ∧ r.2.val < 255 ∧
      (r.1.val, r.2.val) = fletcherSums (bytes ((src.val.drop start.val).take (stop.val - start.val))) ⦄ := by
  unfold frame.fletcher16_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.2.val)
    (fun st => st.2.2.val ≤ stop.val ∧ FletcherInv src start.val st) _ _ _ _ ⟨hi, hinv⟩
  rintro ⟨s1, s2, i⟩ ⟨hi, hs, h1, h2, hsum⟩
  simp only at hi hs h1 h2 hsum
  unfold frame.fletcher16_loop.body
  step*
  · have hlt : i.val < src.val.length := by scalar_tac
    refine ⟨by scalar_tac, ⟨by scalar_tac, by scalar_tac, by scalar_tac, ?_⟩, by scalar_tac⟩
    rw [i5_post, show i.val + 1 - start.val = (i.val - start.val) + 1 by omega]
    have ht : bytes ((src.val.drop start.val).take ((i.val - start.val) + 1)) =
        bytes ((src.val.drop start.val).take (i.val - start.val)) ++ [src.val[i.val].bv] := by
      unfold bytes
      rw [List.take_add_one, List.getElem?_eq_getElem (by simp; omega), List.map_append]
      simp [Nat.add_sub_cancel' hs]
    rw [ht, fletcherSums, List.foldl_append, ← fletcherSums, ← hsum]
    have hb : (src.val[i.val]'hlt).val % 65536 = (src.val[i.val]'hlt).val := by
      have := (src.val[i.val]'hlt).hBounds; simp at this; omega
    simp [s11_post, s21_post, i3_post, i4_post, i2_post, i1_post, UScalar.cast_val_eq, hb]

@[step]
theorem fletcher16_spec (src : Slice U8) (start stop : Usize) (hle : start.val ≤ stop.val)
    (hstop : stop.val ≤ src.val.length) :
    frame.fletcher16 src start stop ⦃ r =>
      [r.1.bv, r.2.bv] = Spec.fletcher16 (bytes ((src.val.drop start.val).take (stop.val - start.val))) ⦄ := by
  unfold frame.fletcher16
  step with fletcher16_loop_spec src start stop 0#u16 0#u16 start hstop hle
    (by simp [FletcherInv, fletcherSums, bytes])
  step*
  rw [fletcher16_eq, ← s1_post3]
  simp only [List.cons.injEq, and_true]
  constructor
  · apply byte_bv; simp [i_post, UScalar.cast_val_eq]
  · apply byte_bv; simp [i1_post, UScalar.cast_val_eq]

@[step]
theorem payload_len_spec (payload : Slice U8) (h : payload.val.length ≤ 3200) :
    frame.payload_len payload ⦃ r => r.val = payload.val.length ⦄ := by
  unfold frame.payload_len
  step*

theorem bytes_append (a b : List U8) : bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]

/-- **C2, encoder.** -/
theorem encode_frame_spec (time : U32) (frameId : U16) (payload : Slice U8) (tag : Std.Array U8 8#usize)
    (h : payload.val.length ≤ maxPayload) :
    frame.encode_frame time frameId payload tag ⦃ r => ∃ v, r = some v ∧
      bytes v.val = frameBytes time.val frameId.val (bytes payload.val) (bytes tag.val) ⦄ := by
  unfold maxPayload at h
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold frame.encode_frame
  dsimp only
  split
  · exfalso; scalar_tac
  · step*
    -- The output grows 2, 3, 7, 9, 11, then 11 + payload, + 2, + 8.
    all_goals try have L0 : out.val.length = 2 := by rw [out_post2, s_post]; simp
    all_goals try have L1 : out1.val.length = 3 := by rw [out1_post]; simp [L0]
    all_goals try have L4 : out4.val.length = 11 := by omega
    all_goals try have L5 : out5.val.length = 11 + payload.val.length := by omega
    all_goals try have L6 : out6.val.length = 12 + payload.val.length := by rw [out6_post]; simp [L5]; omega
    all_goals try have L7 : out7.val.length = 13 + payload.val.length := by rw [out7_post]; simp [L6]; omega
    all_goals try (rw [s3_post]; simp; omega)
    all_goals try omega
    · simp [alloc.vec.Vec.deref]
    · refine ⟨out8, rfl, ?_⟩
      have hver : frame.VERSION.bv = 1 := by unfold frame.VERSION; rfl
      have hb5 : bytes out5.val = [0x6E, 0x52] ++ frameChecked time.val frameId.val (bytes payload.val) := by
        rw [out5_post1, bytes_append, out4_post1, i1_post, out3_post1, out2_post1, out1_post,
          out_post1, s_post, List.nil_append, bytes_append, magic_bytes]
        simp [frameChecked, bytes, hver]
      have hchecked : bytes ((out5.deref.val.drop 2).take (out5.len.val - 2)) =
          frameChecked time.val frameId.val (bytes payload.val) := by
        have : (out5.deref.val.drop 2).take (out5.len.val - 2) = out5.val.drop 2 := by
          simp [alloc.vec.Vec.deref]
        rw [this]
        have := congrArg (List.drop 2) hb5
        simpa [bytes, List.map_drop] using this
      rw [hchecked] at s11_post
      rw [out8_post1, bytes_append, out7_post, out6_post, s3_post]
      simp only [bytes_append, hb5]
      simp only [frameBytes, frameSigned]
      rw [← s11_post]
      simp [bytes, Array.to_slice]

/-- **C2, encoder limit.** -/
theorem encode_frame_too_long (time : U32) (frameId : U16) (payload : Slice U8)
    (tag : Std.Array U8 8#usize) (h : maxPayload < payload.val.length) :
    frame.encode_frame time frameId payload tag ⦃ r => r = none ⦄ := by
  unfold maxPayload at h
  unfold frame.encode_frame
  dsimp only
  split
  · simp
  · exfalso; scalar_tac

theorem be16_bytes (a b : U8) : be16 (a.val * 256 + b.val) = [a.bv, b.bv] := by
  have ha := a.hBounds; have hb := b.hBounds
  simp only [UScalarTy.U8_numBits_eq] at ha hb
  simp only [be16, List.cons.injEq, and_true]
  constructor
  · rw [U8_bv_eq_ofNat]; congr 1; omega
  · rw [U8_bv_eq_ofNat]; apply BitVec.eq_of_toNat_eq; simp

theorem be32_bytes (a b c d : U8) :
    be32 (a.val * 2 ^ 24 + b.val * 2 ^ 16 + c.val * 2 ^ 8 + d.val) = [a.bv, b.bv, c.bv, d.bv] := by
  have ha := a.hBounds; have hb := b.hBounds; have hc := c.hBounds; have hd := d.hBounds
  simp only [UScalarTy.U8_numBits_eq] at ha hb hc hd
  simp only [be32, List.cons.injEq, and_true]
  refine ⟨?_, ?_, ?_, ?_⟩ <;> (rw [U8_bv_eq_ofNat]; apply BitVec.eq_of_toNat_eq; simp; omega)

theorem take_eleven {α : Type} (l : List α) (h : 11 ≤ l.length) :
    l.take 11 = [l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10]] := by
  apply List.ext_getElem
  · simp; omega
  · intro k hk _
    simp only [List.length_take] at hk
    have : k < 11 := by omega
    interval_cases k <;> simp
    all_goals rfl

theorem drop_take_ten {α : Type} (l : List α) (a : Nat) (h : a + 10 ≤ l.length) :
    (l.drop a).take 10 =
      [l[a], l[a + 1], l[a + 2], l[a + 3], l[a + 4], l[a + 5], l[a + 6], l[a + 7], l[a + 8], l[a + 9]] := by
  apply List.ext_getElem
  · simp; omega
  · intro k hk _
    simp only [List.length_take, List.length_drop] at hk
    have : k < 10 := by omega
    interval_cases k <;> simp
    all_goals rfl

theorem drop_two_take_nine {α : Type} (l : List α) (h : 11 ≤ l.length) :
    (l.drop 2).take 9 = [l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10]] := by
  apply List.ext_getElem
  · simp; omega
  · intro k hk _
    simp only [List.length_take, List.length_drop] at hk
    have : k < 9 := by omega
    interval_cases k <;> simp
    all_goals rfl

/-- The frame layout splits an input into header, payload, and trailer. -/
theorem take_frame {α : Type} (l : List α) (len : Nat) :
    l.take (21 + len) = l.take 11 ++ (l.drop 11).take len ++ (l.drop (11 + len)).take 10 := by
  rw [show 21 + len = 11 + (len + 10) by omega, List.take_add, List.take_add, List.drop_drop,
    List.append_assoc]

theorem decode_frame_sound (input : Slice U8) :
    frame.decode_frame input ⦃ r => match r with
      | .Ok f => f.payload.val.length ≤ maxPayload ∧ ∃ extra,
          bytes input.val =
            frameBytes f.time.val f.frame_id.val (bytes f.payload.val) (bytes f.tag.val) ++ extra
      | .Err _ => True ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold frame.decode_frame
  step*
  · simp; scalar_tac
  · -- The checks passed, so the input starts with a well-formed frame.
    have hn : 21 + len.val ≤ input.val.length := by scalar_tac
    have hend : «end».val = 11 + len.val := by scalar_tac
    have hlen : len.val = i7.val := by
      simp only [len_post, UScalar.cast_val_eq, UScalarTy.Usize_numBits_eq]
      apply Nat.mod_eq_of_lt
      have := i7.hBounds
      rcases System.Platform.numBits_eq with hb | hb <;> simp [hb] at this ⊢ <;> omega
    -- The first 11 bytes and the last 10 bytes of the frame.
    have hx := take_eleven input.val (by omega)
    have hy := drop_take_ten input.val «end».val (by omega)
    -- What the checks and reads tell us about them.
    have c0 : ¬(i2 != i3) = true := by assumption
    have c1 : ¬(i4 != i5) = true := by assumption
    have c2 : ¬(i6 != frame.VERSION) = true := by assumption
    have c3 : ¬(i10 != s1) = true := by assumption
    have c4 : ¬(i12 != s2) = true := by assumption
    simp only [bne_iff_ne, ne_eq, not_not] at c0 c1 c2 c3 c4
    have m0 : (input.val[0]'(by omega)).bv = 0x6E := by
      rw [← i2_post, c0, i3_post]; unfold frame.MAGIC; rfl
    have m1 : (input.val[1]'(by omega)).bv = 0x52 := by
      rw [← i4_post, c1, i5_post]; unfold frame.MAGIC; rfl
    have m2 : (input.val[2]'(by omega)).bv = 1 := by
      rw [← i6_post, c2]; unfold frame.VERSION; rfl
    have ht : be32 i28.val = [(input.val[3]'(by omega)).bv, (input.val[4]'(by omega)).bv,
        (input.val[5]'(by omega)).bv, (input.val[6]'(by omega)).bv] := by
      rw [i28_post]
      simp only [getElem!_eq_getElem' _ _ (by omega : 3 < input.val.length),
        getElem!_eq_getElem' _ _ (by omega : 3 + 1 < input.val.length),
        getElem!_eq_getElem' _ _ (by omega : 3 + 2 < input.val.length),
        getElem!_eq_getElem' _ _ (by omega : 3 + 3 < input.val.length)]
      exact be32_bytes _ _ _ _
    have hid : be16 i29.val = [(input.val[7]'(by omega)).bv, (input.val[8]'(by omega)).bv] := by
      rw [i29_post]
      simp only [getElem!_eq_getElem' _ _ (by omega : 7 < input.val.length),
        getElem!_eq_getElem' _ _ (by omega : 7 + 1 < input.val.length)]
      exact be16_bytes _ _
    have hpay : payload.val = (input.val.drop 11).take len.val := by
      rw [payload_post, List.nil_append, header_len_val, hend]; simp
    have hplen : payload.val.length = len.val := by rw [hpay]; simp; omega
    have hl : be16 payload.val.length = [(input.val[9]'(by omega)).bv, (input.val[10]'(by omega)).bv] := by
      rw [hplen, hlen, i7_post]
      simp only [getElem!_eq_getElem' _ _ (by omega : 9 < input.val.length),
        getElem!_eq_getElem' _ _ (by omega : 9 + 1 < input.val.length)]
      exact be16_bytes _ _
    -- The checksum covers version to payload, and it matches the two bytes after the payload.
    have hchk : bytes ((input.val.drop 2).take («end».val - 2)) =
        frameChecked i28.val i29.val (bytes payload.val) := by
      rw [show «end».val - 2 = 9 + len.val by omega, List.take_add, List.drop_drop,
        drop_two_take_nine _ (by omega), ← hpay]
      simp only [frameChecked, ← m2, ht, hid, bytes, List.length_map, hl, List.map_cons,
        List.cons_append, List.nil_append]
      rfl
    rw [hchk] at s1_post
    have e10 : input.val[«end».val]'(by omega) = s1 := by rw [← c3, i10_post]
    have e11 : input.val[«end».val + 1]'(by omega) = s2 := by
      rw [← c4, i12_post]; congr 1; simp [i11_post]
    -- The tag is the eight bytes after the checksum.
    have hi8 : i8.val = «end».val + 2 := by simp [i8_post]
    have g0 : i13 = input.val[«end».val + 2]'(by omega) := by rw [i13_post]; congr 1
    have g1 : i15 = input.val[«end».val + 3]'(by omega) := by rw [i15_post]; congr 1; simp [i14_post, hi8]
    have g2 : i17 = input.val[«end».val + 4]'(by omega) := by rw [i17_post]; congr 1; simp [i16_post, hi8]
    have g3 : i19 = input.val[«end».val + 5]'(by omega) := by rw [i19_post]; congr 1; simp [i18_post, hi8]
    have g4 : i21 = input.val[«end».val + 6]'(by omega) := by rw [i21_post]; congr 1; simp [i20_post, hi8]
    have g5 : i23 = input.val[«end».val + 7]'(by omega) := by rw [i23_post]; congr 1; simp [i22_post, hi8]
    have g6 : i25 = input.val[«end».val + 8]'(by omega) := by rw [i25_post]; congr 1; simp [i24_post, hi8]
    have g7 : i27 = input.val[«end».val + 9]'(by omega) := by rw [i27_post]; congr 1; simp [i26_post, hi8]
    refine ⟨by unfold maxPayload; scalar_tac, (bytes input.val).drop (21 + len.val), ?_⟩
    conv_lhs => rw [← List.take_append_drop (21 + len.val) (bytes input.val)]
    congr 1
    have hf : fletcher16 (frameChecked i28.val i29.val (bytes payload.val)) =
        [(input.val[«end».val]'(by omega)).bv, (input.val[«end».val + 1]'(by omega)).bv] := by
      rw [← s1_post, e10, e11]
    rw [bytes, ← List.map_take, take_frame, hx, ← hend, hy, ← hpay]
    simp only [frameBytes, frameSigned]
    rw [hf]
    simp only [g0, g1, g2, g3, g4, g5, g6, g7, Array.make, bytes, List.map_append, List.map_cons,
      List.map_nil, frameChecked, ← m0, ← m1, ← m2, ht, hid, List.length_map, hl, List.append_assoc,
      List.cons_append, List.nil_append]
    rfl

theorem val_of_bv (x : U8) (m : Nat) (h : x.bv = BitVec.ofNat 8 m) : x.val = m % 256 := by
  have := congrArg BitVec.toNat h
  simpa using this

-- The proof is long, so it needs more than the default time budget.
set_option maxHeartbeats 2000000 in
/-- **C2, decoder.** -/
theorem decode_frame_complete (input : Slice U8) (time : U32) (frameId : U16)
    (pl tag extra : List Spec.Byte) (hp : pl.length ≤ maxPayload) (ht : tag.length = 8)
    (hin : bytes input.val = frameBytes time.val frameId.val pl tag ++ extra) :
    frame.decode_frame input ⦃ r => ∃ f, r = .Ok f ∧ f.time = time ∧ f.frame_id = frameId ∧
      bytes f.payload.val = pl ∧ bytes f.tag.val = tag ⦄ := by
  unfold maxPayload at hp
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hlen : input.val.length = 21 + pl.length + extra.length := by
    have := congrArg List.length hin
    simp [bytes, frameBytes, frameSigned, frameChecked, be16, be32, fletcher16, ht] at this
    omega
  -- The three parts of the input: header, payload, and checksum plus tag.
  have e1 : (bytes input.val).take 11 =
      [0x6E, 0x52, 1] ++ be32 time.val ++ be16 frameId.val ++ be16 pl.length := by
    rw [hin]; simp [frameBytes, frameSigned, frameChecked, be32, be16]
  have e2 : ((bytes input.val).drop 11).take pl.length = pl := by
    rw [hin]; simp [frameBytes, frameSigned, frameChecked, be32, be16]
  have e3 : ((bytes input.val).drop (11 + pl.length)).take 10 =
      fletcher16 (frameChecked time.val frameId.val pl) ++ tag := by
    rw [hin]; simp [frameBytes, frameSigned, frameChecked, be32, be16, fletcher16, ht]
  -- Single bytes of the header.
  have E1 := e1
  rw [bytes, ← List.map_take, take_eleven _ (by omega)] at E1
  simp only [be32, be16, List.map_cons, List.map_nil, List.cons_append, List.nil_append,
    List.cons.injEq] at E1
  obtain ⟨x0, x1, x2, x3, x4, x5, x6, x7, x8, x9, x10, -⟩ := E1
  have htime : time.val = (input.val[3]'(by omega)).val * 2 ^ 24 + (input.val[4]'(by omega)).val * 2 ^ 16 +
      (input.val[5]'(by omega)).val * 2 ^ 8 + (input.val[6]'(by omega)).val := by
    rw [val_of_bv _ _ x3, val_of_bv _ _ x4, val_of_bv _ _ x5, val_of_bv _ _ x6]
    have := time.hBounds; simp at this; omega
  have hid : frameId.val = (input.val[7]'(by omega)).val * 256 + (input.val[8]'(by omega)).val := by
    rw [val_of_bv _ _ x7, val_of_bv _ _ x8]
    have := frameId.hBounds; simp at this; omega
  have hplen : pl.length = (input.val[9]'(by omega)).val * 256 + (input.val[10]'(by omega)).val := by
    have h9 := val_of_bv _ _ x9
    have h10 := val_of_bv _ _ x10
    omega
  -- The checksummed region, and the ten bytes after the pl.
  have hC : bytes ((input.val.drop 2).take (9 + pl.length)) =
      frameChecked time.val frameId.val pl := by
    rw [bytes, List.map_take, List.map_drop, List.take_add, List.drop_drop]
    have h1 : ((List.map (·.bv) input.val).drop 2).take 9 =
        ((bytes input.val).take 11).drop 2 := by
      simp only [bytes, List.drop_take]
    rw [h1, e1, show 2 + 9 = 11 by rfl, ← bytes, e2]
    simp [frameChecked, be32, be16]
  have hy := drop_take_ten input.val (11 + pl.length) (by omega)
  have E3 := e3
  rw [bytes, ← List.map_drop, ← List.map_take, hy] at E3
  rw [fletcher16_eq] at E3
  simp only [List.map_cons, List.map_nil, List.cons_append, List.nil_append, List.cons.injEq] at E3
  obtain ⟨y0, y1, htag⟩ := E3
  unfold frame.decode_frame
  step*
  · -- The magic is right, so this check passes.
    exfalso; rename_i h; simp only [bne_iff_ne, ne_eq] at h; apply h
    rw [UScalar.eq_equiv_bv_eq, i2_post, i3_post]; exact x0.trans (by unfold frame.MAGIC; rfl)
  · exfalso; rename_i h; simp only [bne_iff_ne, ne_eq] at h; apply h
    rw [UScalar.eq_equiv_bv_eq, i4_post, i5_post]; exact x1.trans (by unfold frame.MAGIC; rfl)
  · exfalso; rename_i h; simp only [bne_iff_ne, ne_eq] at h; apply h
    rw [UScalar.eq_equiv_bv_eq, i6_post]; exact x2.trans (by unfold frame.VERSION; rfl)
  all_goals
    have hL : len.val = pl.length := by
      have hi7 : i7.val = pl.length := by
        simp only [Nat.reduceAdd] at i7_post
        rw [i7_post, getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega)]
        exact hplen.symm
      simp only [len_post, UScalar.cast_val_eq, UScalarTy.Usize_numBits_eq]
      rw [← hi7]
      apply Nat.mod_eq_of_lt
      have := i7.hBounds
      rcases System.Platform.numBits_eq with hb | hb <;> simp [hb] at this ⊢ <;> omega
  · exfalso; scalar_tac
  · exfalso; scalar_tac
  all_goals have hE : «end».val = 11 + pl.length := by rw [end_post, header_len_val, hL]
  all_goals
    have hfl : [s1.bv, s2.bv] = [BitVec.ofNat 8 (fletcherSums (frameChecked time.val frameId.val pl)).1,
        BitVec.ofNat 8 (fletcherSums (frameChecked time.val frameId.val pl)).2] := by
      rw [s1_post, show «end».val - 2 = 9 + pl.length by omega, hC, fletcher16_eq]
    simp only [List.cons.injEq, and_true] at hfl
  · -- The first checksum byte matches.
    exfalso; rename_i h; simp only [bne_iff_ne, ne_eq] at h; apply h
    rw [UScalar.eq_equiv_bv_eq, i10_post]
    have b1 : «end».val < input.val.length := by clear * - hE hlen; omega
    have b2 : 11 + pl.length < input.val.length := by clear * - hlen; omega
    have ie : input.val[«end».val]'b1 = input.val[11 + pl.length]'b2 := by congr 1
    exact (congrArg (·.bv) ie).trans (y0.trans hfl.1.symm)
  · -- The second checksum byte matches.
    exfalso; rename_i h; simp only [bne_iff_ne, ne_eq] at h; apply h
    rw [UScalar.eq_equiv_bv_eq, i12_post]
    have b1 : i11.val < input.val.length := by clear * - i11_post hE hlen; omega
    have b2 : 11 + pl.length + 1 < input.val.length := by clear * - hlen; omega
    have ie : input.val[i11.val]'b1 = input.val[11 + pl.length + 1]'b2 := by
      congr 1; simp [i11_post, hE]
    exact (congrArg (·.bv) ie).trans (y1.trans hfl.2.symm)
  · simp; scalar_tac
  · -- Every field comes back.
    refine ⟨_, rfl, ?_, ?_, ?_, ?_⟩
    · apply UScalar.eq_of_val_eq
      simp only [Nat.reduceAdd] at i28_post
      rw [i28_post, htime, getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega),
        getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega)]
    · apply UScalar.eq_of_val_eq
      simp only [Nat.reduceAdd] at i29_post
      rw [i29_post, hid, getElem!_eq_getElem' _ _ (by omega), getElem!_eq_getElem' _ _ (by omega)]
    · rw [payload_post, List.nil_append, header_len_val, show «end».val - 11 = pl.length by omega,
        bytes, List.map_take, List.map_drop]
      exact e2
    · have hi8 : i8.val = 11 + pl.length + 2 := by simp [i8_post, hE]
      have g0 : i13 = input.val[11 + pl.length + 2]'(by omega) := by rw [i13_post]; congr 1
      have g1 : i15 = input.val[11 + pl.length + 3]'(by omega) := by
        rw [i15_post]; congr 1; simp [i14_post, hi8]
      have g2 : i17 = input.val[11 + pl.length + 4]'(by omega) := by
        rw [i17_post]; congr 1; simp [i16_post, hi8]
      have g3 : i19 = input.val[11 + pl.length + 5]'(by omega) := by
        rw [i19_post]; congr 1; simp [i18_post, hi8]
      have g4 : i21 = input.val[11 + pl.length + 6]'(by omega) := by
        rw [i21_post]; congr 1; simp [i20_post, hi8]
      have g5 : i23 = input.val[11 + pl.length + 7]'(by omega) := by
        rw [i23_post]; congr 1; simp [i22_post, hi8]
      have g6 : i25 = input.val[11 + pl.length + 8]'(by omega) := by
        rw [i25_post]; congr 1; simp [i24_post, hi8]
      have g7 : i27 = input.val[11 + pl.length + 9]'(by omega) := by
        rw [i27_post]; congr 1; simp [i26_post, hi8]
      rw [← htag]
      simp only [Array.make, bytes, List.map_cons, List.map_nil, g0, g1, g2, g3, g4, g5, g6, g7]
      rfl

/-- **S2, tag coverage.** -/
theorem signed_len_spec (f : frame.Frame) (h : f.payload.val.length ≤ maxPayload) :
    frame.signed_len f ⦃ n =>
      n.val = (frameSigned f.time.val f.frame_id.val (bytes f.payload.val)).length ⦄ := by
  unfold maxPayload at h
  unfold frame.signed_len
  step*
  simp [frameSigned, frameChecked, be32, be16, fletcher16, bytes, *]
  omega

end Protocol.Frame
