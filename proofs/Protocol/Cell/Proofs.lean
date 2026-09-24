import Protocol.Code.Funs
import Protocol.Cell.Spec

/-!
# The Rust cell codec computes `Spec.lean`

One spec per Rust function in `crates/protocol/src/cell.rs`, then C1.
-/

open Aeneas Aeneas.Std Result protocol

namespace Protocol.Cell

-- Aeneas keeps Rust constants opaque. These let `scalar_tac` see their values.
@[simp, scalar_tac_simps, grind =, agrind =]
theorem cells_per_group_val : cell.CELLS_PER_GROUP.val = 8 := by
  unfold cell.CELLS_PER_GROUP; rfl

@[simp, scalar_tac_simps, grind =, agrind =]
theorem bytes_per_group_val : cell.BYTES_PER_GROUP.val = 3 := by
  unfold cell.BYTES_PER_GROUP; rfl

/-! ## Small helpers -/

@[step]
theorem group_bits_spec (a b c : U8) :
    cell.group_bits a b c ⦃ r =>
      r.bv = a.bv.setWidth 32 <<< 16 ||| b.bv.setWidth 32 <<< 8 ||| c.bv.setWidth 32 ⦄ := by
  unfold cell.group_bits
  step*
  simp only [*, UScalar.cast_bv_eq, UScalarTy.U32_numBits_eq, UScalar.bv_or]

@[step]
theorem cell_at_spec (bits shift : U32) (h : shift.val < 32) :
    cell.cell_at bits shift ⦃ r => r.bv = ((bits.bv >>> shift.val) &&& 7).setWidth 8 ⦄ := by
  unfold cell.cell_at
  step*
  simp only [*, UScalar.cast_bv_eq, UScalarTy.U8_numBits_eq]
  rfl

@[step]
theorem append_cell_spec (bits : U32) (c : U8) :
    cell.append_cell bits c ⦃ r => r.bv = bits.bv <<< 3 ||| c.bv.setWidth 32 ⦄ := by
  unfold cell.append_cell
  step*
  simp only [*, UScalar.cast_bv_eq, UScalarTy.U32_numBits_eq, UScalar.bv_or]

@[step]
theorem byte_at_spec (bits shift : U32) (h : shift.val < 32) :
    cell.byte_at bits shift ⦃ r => r.bv = (bits.bv >>> shift.val).setWidth 8 ⦄ := by
  unfold cell.byte_at
  step*

@[step]
theorem byte_or_zero_spec (bytes : Slice U8) (i : Usize) :
    cell.byte_or_zero bytes i ⦃ r => r.bv = (bytes.val.map (·.bv)).getD i.val 0 ⦄ := by
  unfold cell.byte_or_zero
  step*
  · have hlt : i.val < bytes.val.length := by scalar_tac
    simp [r_post, List.getD_eq_getElem?_getD, List.getElem?_map, List.getElem?_eq_getElem hlt]
  · simp only [List.getD_eq_getElem?_getD, List.getElem?_map]
    rw [List.getElem?_eq_none (by simp at *; scalar_tac)]
    rfl

/-! ## One group -/

@[step]
theorem encode_group_spec (a b c : U8) :
    cell.encode_group a b c ⦃ g => g.val.map (·.bv) = encodeGroupBv a.bv b.bv c.bv ⦄ := by
  unfold cell.encode_group
  step*
  simp only [Array.make, List.map_cons, List.map_nil, encodeGroupBv, cellAt, groupBits, *]

/-- `decode_group` has two outcomes, and the spec pins down both. -/
def DecodeGroupPost (g : Std.Array U8 8#usize) (r : Option (Std.Array U8 3#usize)) : Prop :=
  match r with
  | some out => cellsValid g.val ∧ out.val.map (·.bv) = decodeGroupBv (g.val.map (·.bv))
  | none => ¬ cellsValid g.val

def DecodeGroupInv (g : Std.Array U8 8#usize) (st : U32 × Usize) : Prop :=
  st.2.val ≤ 8 ∧
  st.1.bv = appendCells 0 ((g.val.take st.2.val).map (·.bv)) ∧
  cellsValid (g.val.take st.2.val)

theorem decode_group_loop_spec (g : Std.Array U8 8#usize) (bits : U32) (i : Usize)
    (hinv : DecodeGroupInv g (bits, i)) :
    cell.decode_group_loop g bits i ⦃ r => DecodeGroupPost g r ⦄ := by
  unfold cell.decode_group_loop
  apply loop.spec_decr_nat (fun st => 8 - st.2.val) (DecodeGroupInv g) _ _ _ _ hinv
  rintro ⟨bits, i⟩ ⟨hi, hbits, hvalid⟩
  simp only at hi hbits hvalid
  unfold cell.decode_group_loop.body
  step*
  case h1 =>
    -- Cell i is above 7.
    intro hall
    have := hall i1 (by rw [i1_post]; exact List.getElem_mem _)
    scalar_tac
  · -- Cell i is fine: the invariant holds for i + 1.
    have hlt : i.val < (↑g : List U8).length := by simp; scalar_tac
    have htake : (↑g : List U8).take i2.val = (↑g : List U8).take i.val ++ [i1] := by
      rw [i2_post, i1_post, List.take_add_one, List.getElem?_eq_getElem hlt]; rfl
    refine ⟨⟨by scalar_tac, ?_, ?_⟩, by scalar_tac⟩
    · simp only [htake, List.map_append, appendCells, List.foldl_append] at hbits ⊢
      simp [bits1_post, hbits]
    · intro c hc
      rw [htake, List.mem_append, List.mem_singleton] at hc
      rcases hc with hc | rfl
      · exact hvalid c hc
      · scalar_tac
  · -- All eight cells were fine.
    have h8 : i.val = 8 := by simp at *; scalar_tac
    have hfull : (↑g : List U8).take i.val = ↑g := by simp [h8]
    rw [hfull] at hbits hvalid
    refine ⟨hvalid, ?_⟩
    simp [Array.make, decodeGroupBv, *]

@[step]
theorem decode_group_spec (g : Std.Array U8 8#usize) :
    cell.decode_group g ⦃ r => DecodeGroupPost g r ⦄ := by
  unfold cell.decode_group
  apply decode_group_loop_spec
  simp [DecodeGroupInv, appendCells, cellsValid]

/-! ## The encoder -/

theorem push_group_loop_spec (cells0 : alloc.vec.Vec U8) (group : Std.Array U8 8#usize)
    (cells : alloc.vec.Vec U8) (i : Usize)
    (hroom : cells0.val.length + 8 ≤ Usize.max)
    (hi : i.val ≤ 8) (hcells : cells.val = cells0.val ++ group.val.take i.val) :
    cell.push_group_loop cells group i ⦃ r => r.val = cells0.val ++ group.val ⦄ := by
  unfold cell.push_group_loop
  apply loop.spec_decr_nat (fun st => 8 - st.2.val)
    (fun st => st.2.val ≤ 8 ∧ st.1.val = cells0.val ++ group.val.take st.2.val) _ _ _ _
    ⟨hi, hcells⟩
  rintro ⟨cells, i⟩ ⟨hi, hcells⟩
  simp only at hi hcells
  unfold cell.push_group_loop.body
  step*
  · rw [hcells]; simp; scalar_tac
  · have hlt : i.val < (↑group : List U8).length := by simp; scalar_tac
    refine ⟨by scalar_tac, ?_, by scalar_tac⟩
    rw [cells1_post, hcells, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, i1_post]
    simp
  · have h8 : i.val = 8 := by simp at *; scalar_tac
    rw [hcells, h8]
    simp

@[step]
theorem push_group_spec (cells : alloc.vec.Vec U8) (group : Std.Array U8 8#usize)
    (hroom : cells.val.length + 8 ≤ Usize.max) :
    cell.push_group cells group ⦃ r => r.val = cells.val ++ group.val ⦄ := by
  unfold cell.push_group
  apply push_group_loop_spec cells group cells 0#usize hroom <;> simp

/-- The largest input that the encoder proof covers. A frame is about 3200 bytes. -/
def maxEncodeInput : Nat := 65536

/-- Cells so far, plus the cells of the bytes left, are the cells of all bytes. -/
def EncodeInv (bytes : Slice U8) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val % 3 = 0 ∧ st.2.val ≤ bytes.val.length + 2 ∧
  st.1.val.length = 8 * (st.2.val / 3) ∧
  st.1.val.map (·.bv) ++ encodeBytesBv ((bytes.val.map (·.bv)).drop st.2.val) =
    encodeBytesBv (bytes.val.map (·.bv))

theorem encode_cells_loop_spec (bytes : Slice U8) (cells : alloc.vec.Vec U8) (i : Usize)
    (hmax : bytes.val.length ≤ maxEncodeInput) (hinv : EncodeInv bytes (cells, i)) :
    cell.encode_cells_loop bytes cells i ⦃ r =>
      r.val.map (·.bv) = encodeBytesBv (bytes.val.map (·.bv)) ⦄ := by
  unfold cell.encode_cells_loop
  apply loop.spec_decr_nat (fun st => bytes.val.length - st.2.val) (EncodeInv bytes) _ _ _ _ hinv
  rintro ⟨cells, i⟩ ⟨hmod, hi, hlen, hcells⟩
  simp only at hmod hi hlen hcells
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  unfold maxEncodeInput at hmax
  unfold cell.encode_cells_loop.body
  step*
  · -- One more group: the invariant holds for i + 3.
    have hlt : i.val < bytes.val.length := by scalar_tac
    have hi7 : i7.val = i.val + 3 := by simp [i7_post]
    simp only [EncodeInv, hi7]
    refine ⟨⟨by omega, by omega, ?_, ?_⟩, by omega⟩
    · rw [cells1_post, List.length_append, hlen]
      simp; omega
    · rw [← hcells]
      conv_rhs => rw [encodeBytesBv_drop _ _ (by simpa using hlt)]
      simp [cells1_post, group_post, i2_post, i4_post, i6_post, i3_post, i5_post]
  · -- Past the end: nothing is left to encode.
    have hdrop : (bytes.val.map (·.bv)).drop i.val = [] := by simp; scalar_tac
    simpa [hdrop, encodeBytesBv] using hcells

@[step]
theorem encode_cells_spec (bytes : Slice U8) (hmax : bytes.val.length ≤ maxEncodeInput) :
    cell.encode_cells bytes ⦃ cells =>
      cells.val.map (·.bv) = encodeBytesBv (bytes.val.map (·.bv)) ⦄ := by
  unfold cell.encode_cells
  apply encode_cells_loop_spec bytes _ _ hmax
  simp [EncodeInv]

/-! ## The decoder

Its input comes from an untrusted image, so the spec covers every input and
has no precondition.
-/

def DecodeCellsInv (cells : Slice U8) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val % 8 = 0 ∧ st.2.val ≤ cells.val.length ∧
  cellsValid (cells.val.take st.2.val) ∧
  st.1.val.length = 3 * (st.2.val / 8) ∧
  st.1.val.map (·.bv) ++ decodeAllBv ((cells.val.map (·.bv)).drop st.2.val) =
    decodeAllBv (cells.val.map (·.bv))

def DecodeCellsLoopPost (cells : Slice U8) (r : Option (alloc.vec.Vec U8)) : Prop :=
  match r with
  | some bytes => cellsValid cells.val ∧
      bytes.val.map (·.bv) = decodeAllBv (cells.val.map (·.bv))
  | none => ¬ cellsValid cells.val

theorem decode_cells_loop_spec (cells : Slice U8) (bytes : alloc.vec.Vec U8) (i : Usize)
    (hwhole : cells.val.length % 8 = 0) (hinv : DecodeCellsInv cells (bytes, i)) :
    cell.decode_cells_loop cells bytes i ⦃ r => DecodeCellsLoopPost cells r ⦄ := by
  unfold cell.decode_cells_loop
  apply loop.spec_decr_nat (fun st => cells.val.length - st.2.val) (DecodeCellsInv cells)
    _ _ _ _ hinv
  rintro ⟨bytes, i⟩ ⟨hmod, hi, hvalid, hlen, hbytes⟩
  simp only at hmod hi hvalid hlen hbytes
  unfold cell.decode_cells_loop.body
  step*
  -- Every goal but the last reads one group of eight cells.
  all_goals try (
    have hlt : i.val < cells.val.length := by scalar_tac
    have h8 : i.val + 8 ≤ cells.val.length := by omega
    have hg : [i2, i4, i6, i8, i10, i12, i14, i16] = (cells.val.drop i.val).take 8 := by
      rw [take_eight_drop _ _ h8]
      simp [i2_post, i4_post, i6_post, i8_post, i10_post, i12_post, i14_post, i16_post,
        i3_post, i5_post, i7_post, i9_post, i11_post, i13_post, i15_post]
      rfl)
  · -- A bad cell in this group.
    obtain rfl : o = none := by assumption
    intro hall
    apply o_post
    intro c hc
    simp only [Array.make, hg] at hc
    exact hall c (List.mem_of_mem_drop (List.mem_of_mem_take hc))
  · have := cells.property
    simp [bytes1_post]; omega
  · have := cells.property
    simp [bytes2_post, bytes1_post]; omega
  · -- A good group: the invariant holds for i + 8.
    obtain rfl : o = some a := by assumption
    obtain ⟨gvalid, gbytes⟩ := o_post
    simp only [Array.make, hg] at gvalid gbytes
    have hi17 : i17.val = i.val + 8 := by simp [i17_post]
    have ha : a.val = [a1, b, c] := by
      have : a.val.length = 3 := by simp
      match ha : a.val, this with
      | [x, y, z], _ => simp_all
    simp only [DecodeCellsInv, hi17]
    refine ⟨⟨by omega, h8, ?_, ?_, ?_⟩, by omega⟩
    · intro x hx
      rw [List.take_add, List.mem_append] at hx
      rcases hx with hx | hx
      · exact hvalid x hx
      · exact gvalid x hx
    · simp [bytes3_post, bytes2_post, bytes1_post, hlen]; omega
    · rw [decodeAllBv_drop _ _ (by simpa using h8)] at hbytes
      simp only [List.map_take, List.map_drop] at gbytes
      rw [← hbytes, ← gbytes, ha]
      simp [bytes3_post, bytes2_post, bytes1_post]
  · -- Past the end: every group was good.
    have hn : i.val = cells.val.length := by scalar_tac
    rw [hn, List.take_length] at hvalid
    have hdrop : (cells.val.map (·.bv)).drop cells.val.length = [] := by simp
    rw [hn, hdrop] at hbytes
    exact ⟨hvalid, by simpa [decodeAllBv] using hbytes⟩

/-- `decode_cells` returns bytes exactly when the input is whole groups of valid cells. -/
def DecodeCellsPost (cells : Slice U8) (r : Option (alloc.vec.Vec U8)) : Prop :=
  match r with
  | some bytes => cells.val.length % 8 = 0 ∧ cellsValid cells.val ∧
      bytes.val.map (·.bv) = decodeAllBv (cells.val.map (·.bv))
  | none => cells.val.length % 8 ≠ 0 ∨ ¬ cellsValid cells.val

@[step]
theorem decode_cells_spec (cells : Slice U8) :
    cell.decode_cells cells ⦃ r => DecodeCellsPost cells r ⦄ := by
  unfold cell.decode_cells
  step*
  · have hwhole : cells.val.length % 8 = 0 := by
      rename_i hb; simp [hb] at b_post; simpa using b_post
    apply WP.spec_mono
      (decode_cells_loop_spec cells _ _ hwhole (by simp [DecodeCellsInv, cellsValid]))
    intro r hr
    cases r <;> simp_all [DecodeCellsLoopPost, DecodeCellsPost]
  · left
    rename_i hb; simp [hb] at b_post; simpa using b_post

/-! ## C1 -/

/-- **C1.** Encoding bytes into cells and decoding them gives the same bytes,
followed by the zero padding of the last group. -/
theorem cells_round_trip (bytes : Slice U8) (hmax : bytes.val.length ≤ maxEncodeInput) :
    (do
      let cells ← cell.encode_cells bytes
      cell.decode_cells (alloc.vec.Vec.deref cells))
    ⦃ r => ∃ out, r = some out ∧
      out.val = bytes.val ++ List.replicate (padLength bytes.val.length) 0#u8 ⦄ := by
  step*
  have hval : (alloc.vec.Vec.deref cells).val = cells.val := rfl
  have hwhole : cells.val.length % 8 = 0 := by
    have := congrArg List.length cells_post
    simp only [List.length_map, encodeBytesBv_length] at this
    omega
  have hvalid : cellsValid cells.val := by
    intro x hx
    have := encodeBytesBv_valid _ x.bv (cells_post ▸ List.mem_map_of_mem hx)
    rwa [U8.bv_toNat] at this
  cases r with
  | none =>
    simp only [DecodeCellsPost, hval] at r_post
    rcases r_post with h | h
    · exact absurd hwhole h
    · exact absurd hvalid h
  | some out =>
    obtain ⟨-, -, hout⟩ := r_post
    refine ⟨out, rfl, ?_⟩
    rw [hval, cells_post, decode_encode_bytes] at hout
    apply List.map_injective_iff.mpr (fun x y h => (UScalar.eq_equiv_bv_eq x y).mpr h)
    simpa using hout

end Protocol.Cell
