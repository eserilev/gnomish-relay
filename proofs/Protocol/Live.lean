import Protocol.Cut
import Protocol.Spec.Live

/-! # The live file (S20, S21) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii Protocol.Cut

namespace Protocol.Live

@[simp, scalar_tac_simps]
theorem max_progress_val : live.MAX_PROGRESS.val = 30 := by unfold live.MAX_PROGRESS; rfl

@[simp, scalar_tac_simps]
theorem max_lines_val : live.MAX_LINES.val = 5 := by unfold live.MAX_LINES; rfl

@[simp, scalar_tac_simps]
theorem max_line_val : live.MAX_LINE.val = 200 := by unfold live.MAX_LINE; rfl

@[simp, scalar_tac_simps]
theorem max_requests_val : live.MAX_REQUESTS.val = 4 := by unfold live.MAX_REQUESTS; rfl

@[simp, scalar_tac_simps]
theorem max_options_val : live.MAX_OPTIONS.val = 4 := by unfold live.MAX_OPTIONS; rfl

@[simp, scalar_tac_simps]
theorem max_popup_val : live.MAX_POPUP.val = 2000 := by unfold live.MAX_POPUP; rfl

@[simp, scalar_tac_simps]
theorem max_label_val : live.MAX_LABEL.val = 64 := by unfold live.MAX_LABEL; rfl

/-! ## Prepare -/

def LinesInv (lines : Slice (alloc.vec.Vec U8)) (s : Nat)
    (st : alloc.vec.Vec (alloc.vec.Vec U8) × Usize) : Prop :=
  s ≤ st.2.val ∧ st.2.val ≤ max s lines.val.length ∧ st.1.val.length = st.2.val - s ∧
    List.Forall₂ (fun o p => bytes p.val = (bytes o.val).take maxLine)
      ((lines.val.drop s).take (st.2.val - s)) st.1.val ∧
    ∀ l ∈ st.1.val, l.val.length ≤ maxLine

@[step]
theorem prepare_lines_spec (lines : Slice (alloc.vec.Vec U8)) :
    live.prepare_lines lines ⦃ ps =>
      ps.val.length ≤ maxLines ∧ (∀ l ∈ ps.val, l.val.length ≤ maxLine) ∧
      List.Forall₂ (fun o p => bytes p.val = (bytes o.val).take maxLine)
        (lines.val.drop (lines.val.length - maxLines)) ps.val ⦄ := by
  unfold live.prepare_lines
  step*
  unfold live.prepare_lines_loop
  apply loop.spec_decr_nat (fun st => lines.val.length - st.2.val) (LinesInv lines i1.val) _ _ _ _
    ⟨le_refl _, le_max_left _ _, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hlen, hf, hfit⟩
  simp only at hs hi hlen hf hfit
  unfold live.prepare_lines_loop.body
  step*
  · have hlt : i.val < lines.val.length := by scalar_tac
    unfold LinesInv
    dsimp only
    have htake : (lines.val.drop i1.val).take (i2.val - i1.val) =
        (lines.val.drop i1.val).take (i.val - i1.val) ++ [lines.val[i.val]] := by
      rw [i2_post, show i.val + 1 - i1.val = (i.val - i1.val) + 1 by omega, List.take_add_one,
        List.getElem?_drop, show i1.val + (i.val - i1.val) = i.val by omega,
        List.getElem?_eq_getElem hlt]
      rfl
    have hv : bytes v1.val = (bytes (lines.val[i.val]).val).take maxLine := by
      rw [← v_post]; simpa [maxLine, Protocol.Seen.deref_val] using v1_post1
    refine ⟨⟨by omega, by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons hv .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · simpa [maxLine] using v1_post2
  · have hs5 : i1.val = lines.val.length - maxLines := by simp [maxLines]; scalar_tac
    have hn : i.val = lines.val.length := by scalar_tac
    rw [hn, List.take_of_length_le (by simp)] at hf
    refine ⟨?_, hfit, by rw [← hs5]; exact hf⟩
    rw [hlen, hn, hs5, maxLines]
    omega

@[step]
theorem prepare_progress_one_spec (p : live.Progress) :
    live.prepare_progress_one p ⦃ r => progressFrom p r ∧ fitsProgress r ⦄ := by
  unfold live.prepare_progress_one
  step*
  simp only [Protocol.Seen.deref_val, Protocol.Slot.max_id_len_val] at *
  exact ⟨⟨rfl, v_post1, v1_post3⟩, v_post2, v1_post1, v1_post2⟩

def LastInv {α β : Type} (rel : α → β → Prop) (fits : β → Prop) (xs : List α) (s : Nat)
    (st : alloc.vec.Vec β × Usize) : Prop :=
  s ≤ st.2.val ∧ st.2.val ≤ max s xs.length ∧ st.1.val.length = st.2.val - s ∧
    List.Forall₂ rel ((xs.drop s).take (st.2.val - s)) st.1.val ∧ ∀ y ∈ st.1.val, fits y

/-- **S20, prepare progress.** -/
theorem prepare_progress_spec (progress : Slice live.Progress) :
    live.prepare_progress progress ⦃ ps =>
      ps.val.length ≤ maxProgress ∧ (∀ p ∈ ps.val, fitsProgress p) ∧
      List.Forall₂ progressFrom (progress.val.drop (progress.val.length - maxProgress)) ps.val ⦄ := by
  unfold live.prepare_progress
  step*
  unfold live.prepare_progress_loop
  apply loop.spec_decr_nat (fun st => progress.val.length - st.2.val)
    (LastInv progressFrom fitsProgress progress.val i1.val) _ _ _ _
    ⟨le_refl _, le_max_left _ _, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hlen, hf, hfit⟩
  simp only at hs hi hlen hf hfit
  unfold live.prepare_progress_loop.body
  step*
  · have hlt : i.val < progress.val.length := by scalar_tac
    unfold LastInv
    dsimp only
    have htake : (progress.val.drop i1.val).take (i2.val - i1.val) =
        (progress.val.drop i1.val).take (i.val - i1.val) ++ [progress.val[i.val]] := by
      rw [i2_post, show i.val + 1 - i1.val = (i.val - i1.val) + 1 by omega, List.take_add_one,
        List.getElem?_drop, show i1.val + (i.val - i1.val) = i.val by omega,
        List.getElem?_eq_getElem hlt]
      rfl
    refine ⟨⟨by omega, by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (p_post ▸ p1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact p1_post2
  · have hs30 : i1.val = progress.val.length - maxProgress := by simp [maxProgress]; scalar_tac
    have hn : i.val = progress.val.length := by scalar_tac
    rw [hn, List.take_of_length_le (by simp)] at hf
    refine ⟨?_, hfit, by rw [← hs30]; exact hf⟩
    rw [hlen, hn, hs30, maxProgress]
    omega

@[step]
theorem prepare_option_spec (o : live.PermOption) :
    live.prepare_option o ⦃ r => optionFrom o r ∧ fitsOption r ⦄ := by
  unfold live.prepare_option
  step*
  simp only [Protocol.Seen.deref_val, Protocol.Slot.max_id_len_val, max_label_val] at *
  exact ⟨⟨rfl, v_post1, by simpa [maxLabel] using v1_post1⟩, v_post2, by simpa [maxLabel] using v1_post2⟩

/-- The loop invariant of a loop that keeps the first `n` items. -/
def FirstInv {α β : Type} (rel : α → β → Prop) (fits : β → Prop) (xs : List α) (n : Nat)
    (st : alloc.vec.Vec β × Usize) : Prop :=
  st.2.val ≤ min xs.length n ∧ st.1.val.length = st.2.val ∧
    List.Forall₂ rel (xs.take st.2.val) st.1.val ∧ ∀ y ∈ st.1.val, fits y

@[step]
theorem prepare_options_spec (options : Slice live.PermOption) :
    live.prepare_options options ⦃ os =>
      os.val.length ≤ maxOptions ∧ (∀ o ∈ os.val, fitsOption o) ∧
      List.Forall₂ optionFrom (options.val.take maxOptions) os.val ⦄ := by
  unfold live.prepare_options live.prepare_options_loop
  apply loop.spec_decr_nat (fun st => min options.val.length 4 - st.2.val)
    (FirstInv optionFrom fitsOption options.val 4) _ _ _ _ ⟨by simp, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hi, hlen, hf, hfit⟩
  simp only at hi hlen hf hfit
  unfold live.prepare_options_loop.body
  step*
  · have hmin : i.val < min options.val.length 4 := by simp at i2_post; scalar_tac
    have hlt : i.val < options.val.length := lt_of_lt_of_le hmin (min_le_left _ _)
    have htake : options.val.take i3.val = options.val.take i.val ++ [options.val[i.val]] := by
      rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt]
      rfl
    unfold FirstInv
    dsimp only
    refine ⟨⟨by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (po_post ▸ po1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact po1_post2
  · have hn : i.val = min options.val.length 4 := by simp at i2_post; scalar_tac
    have htake : options.val.take i.val = options.val.take 4 := by
      rw [hn]
      rcases le_total options.val.length 4 with h | h
      · rw [min_eq_left h, List.take_length, List.take_of_length_le h]
      · rw [min_eq_right h]
    rw [htake] at hf
    exact ⟨by rw [hlen, hn, maxOptions]; omega, hfit, hf⟩

@[step]
theorem prepare_request_spec (r : live.Request) :
    live.prepare_request r ⦃ q => requestFrom r q ∧ fitsRequest q ⦄ := by
  unfold live.prepare_request
  step*
  simp only [Protocol.Seen.deref_val, Protocol.Slot.max_id_len_val, max_popup_val] at *
  exact ⟨⟨rfl, v_post1, v1_post1, by simpa [maxPopup] using v2_post1, v3_post3⟩,
    v_post2, v1_post2, by simpa [maxPopup] using v2_post2, v3_post1, v3_post2⟩

/-- **S20, prepare requests.** -/
theorem prepare_requests_spec (requests : Slice live.Request) :
    live.prepare_requests requests ⦃ rs =>
      rs.val.length ≤ maxRequests ∧ (∀ r ∈ rs.val, fitsRequest r) ∧
      List.Forall₂ requestFrom (requests.val.take maxRequests) rs.val ⦄ := by
  unfold live.prepare_requests live.prepare_requests_loop
  apply loop.spec_decr_nat (fun st => min requests.val.length 4 - st.2.val)
    (FirstInv requestFrom fitsRequest requests.val 4) _ _ _ _ ⟨by simp, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hi, hlen, hf, hfit⟩
  simp only at hi hlen hf hfit
  unfold live.prepare_requests_loop.body
  step*
  · have hmin : i.val < min requests.val.length 4 := by simp at i2_post; scalar_tac
    have hlt : i.val < requests.val.length := lt_of_lt_of_le hmin (min_le_left _ _)
    have htake : requests.val.take i3.val = requests.val.take i.val ++ [requests.val[i.val]] := by
      rw [i3_post, List.take_add_one, List.getElem?_eq_getElem hlt]
      rfl
    unfold FirstInv
    dsimp only
    refine ⟨⟨by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (r_post ▸ r1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact r1_post2
  · have hn : i.val = min requests.val.length 4 := by simp at i2_post; scalar_tac
    have htake : requests.val.take i.val = requests.val.take 4 := by
      rw [hn]
      rcases le_total requests.val.length 4 with h | h
      · rw [min_eq_left h, List.take_length, List.take_of_length_le h]
      · rw [min_eq_right h]
    rw [htake] at hf
    exact ⟨by rw [hlen, hn, maxRequests]; omega, hfit, hf⟩

/-! ## Size bound (pure) -/

theorem kind_literal_length (k : live.OptionKind) : (luaLiteral (ascii (kindWord k))).length ≤ 15 := by
  cases k <;> decide

theorem lineItem_length (l : alloc.vec.Vec U8) (h : l.val.length ≤ maxLine) : (lineItem l).length ≤ 804 := by
  have hl := Protocol.Slot.literal_length_le (bytes l.val)
  rw [bytes_length] at hl
  simp only [maxLine] at h
  simp only [lineItem, List.length_append]
  have : (ascii ", ").length = 2 := rfl
  omega

theorem progressLine_length (p : live.Progress) (h : fitsProgress p) : (progressLine p).length ≤ 4190 := by
  obtain ⟨hchat, hn, hall⟩ := h
  have hc := Protocol.Slot.literal_length_le (bytes p.chat.val)
  have hd := decimal_u32_length p.id
  have hl := sum_le lineItem 804 p.lines.val (fun l hl => lineItem_length l (hall l hl))
  rw [bytes_length] at hc
  simp only [maxLines] at hn
  simp only [progressLine, List.length_append]
  have : (ascii "{chat = ").length = 8 := rfl
  have : (ascii ", id = ").length = 7 := rfl
  have : (ascii ", lines = {").length = 11 := rfl
  have : (ascii "}},\n").length = 4 := rfl
  have : 804 * p.lines.val.length ≤ 4020 := by omega
  omega

theorem optionLine_length (o : live.PermOption) (h : fitsOption o) : (optionLine o).length ≤ 431 := by
  obtain ⟨hid, hlabel⟩ := h
  have hi := Protocol.Slot.literal_length_le (bytes o.id.val)
  have hl := Protocol.Slot.literal_length_le (bytes o.label.val)
  have hk := kind_literal_length o.kind
  rw [bytes_length] at hi hl
  simp only [maxLabel] at hlabel
  simp only [optionLine, List.length_append]
  have : (ascii "{id = ").length = 6 := rfl
  have : (ascii ", kind = ").length = 9 := rfl
  have : (ascii ", label = ").length = 10 := rfl
  have : (ascii "},\n").length = 3 := rfl
  omega

theorem requestLine_length (r : live.Request) (h : fitsRequest r) : (requestLine r).length ≤ 10050 := by
  obtain ⟨hreq, hchat, htext, hn, hall⟩ := h
  have hr := Protocol.Slot.literal_length_le (bytes r.request.val)
  have hc := Protocol.Slot.literal_length_le (bytes r.chat.val)
  have ht := Protocol.Slot.literal_length_le (bytes r.text.val)
  have hd := decimal_u32_length r.id
  have ho := sum_le optionLine 431 r.options.val (fun o ho => optionLine_length o (hall o ho))
  rw [bytes_length] at hr hc ht
  simp only [maxPopup, maxOptions] at htext hn
  simp only [requestLine, List.length_append]
  have : (ascii "{request = ").length = 11 := rfl
  have : (ascii ", chat = ").length = 9 := rfl
  have : (ascii ", id = ").length = 7 := rfl
  have : (ascii ", text = ").length = 9 := rfl
  have : (ascii ", options = {\n").length = 14 := rfl
  have : (ascii "}},\n").length = 4 := rfl
  have : 431 * r.options.val.length ≤ 1724 := by omega
  omega

/-- **S21.** -/
theorem live_bound (progress : List live.Progress) (requests : List live.Request)
    (h : fitsLive progress requests) : (liveBytes progress requests).length ≤ liveLimit := by
  obtain ⟨hp, hpall, hr, hrall⟩ := h
  have hpl := sum_le progressLine 4190 progress (fun p hp => progressLine_length p (hpall p hp))
  have hrl := sum_le requestLine 10050 requests (fun r hr => requestLine_length r (hrall r hr))
  simp only [maxProgress, maxRequests] at hp hr
  simp only [liveBytes, liveLimit, List.length_append]
  have : (ascii "GnomishRelay_Live = {progress = {\n").length = 34 := rfl
  have : (ascii "}, permissions = {\n").length = 19 := rfl
  have : (ascii "}}\n").length = 3 := rfl
  have : 4190 * progress.length ≤ 125700 := by omega
  have : 10050 * requests.length ≤ 40200 := by omega
  omega

/-! ## The template (S20) -/

theorem head_bytes : bytes (Array.to_slice live.HEAD).val = ascii "GnomishRelay_Live = {progress = {\n" := by
  unfold live.HEAD; rfl

@[simp, scalar_tac_simps]
theorem head_length : (Array.to_slice live.HEAD).val.length = 34 := by unfold live.HEAD; rfl

theorem permissions_bytes : bytes (Array.to_slice live.PERMISSIONS).val = ascii "}, permissions = {\n" := by
  unfold live.PERMISSIONS; rfl

@[simp, scalar_tac_simps]
theorem permissions_length : (Array.to_slice live.PERMISSIONS).val.length = 19 := by unfold live.PERMISSIONS; rfl

theorem tail_bytes : bytes (Array.to_slice live.TAIL).val = ascii "}}\n" := by
  unfold live.TAIL; rfl

@[simp, scalar_tac_simps]
theorem tail_length : (Array.to_slice live.TAIL).val.length = 3 := by unfold live.TAIL; rfl

theorem chat_bytes : bytes (Array.to_slice live.CHAT).val = ascii "{chat = " := by
  unfold live.CHAT; rfl

@[simp, scalar_tac_simps]
theorem chat_length : (Array.to_slice live.CHAT).val.length = 8 := by unfold live.CHAT; rfl

theorem id_bytes : bytes (Array.to_slice live.ID).val = ascii ", id = " := by
  unfold live.ID; rfl

@[simp, scalar_tac_simps]
theorem id_length : (Array.to_slice live.ID).val.length = 7 := by unfold live.ID; rfl

theorem lines_bytes : bytes (Array.to_slice live.LINES).val = ascii ", lines = {" := by
  unfold live.LINES; rfl

@[simp, scalar_tac_simps]
theorem lines_length : (Array.to_slice live.LINES).val.length = 11 := by unfold live.LINES; rfl

theorem line_end_bytes : bytes (Array.to_slice live.LINE_END).val = ascii ", " := by
  unfold live.LINE_END; rfl

@[simp, scalar_tac_simps]
theorem line_end_length : (Array.to_slice live.LINE_END).val.length = 2 := by unfold live.LINE_END; rfl

theorem progress_end_bytes : bytes (Array.to_slice live.PROGRESS_END).val = ascii "}},\n" := by
  unfold live.PROGRESS_END; rfl

@[simp, scalar_tac_simps]
theorem progress_end_length : (Array.to_slice live.PROGRESS_END).val.length = 4 := by unfold live.PROGRESS_END; rfl

theorem request_bytes : bytes (Array.to_slice live.REQUEST).val = ascii "{request = " := by
  unfold live.REQUEST; rfl

@[simp, scalar_tac_simps]
theorem request_length : (Array.to_slice live.REQUEST).val.length = 11 := by unfold live.REQUEST; rfl

theorem request_chat_bytes : bytes (Array.to_slice live.REQUEST_CHAT).val = ascii ", chat = " := by
  unfold live.REQUEST_CHAT; rfl

@[simp, scalar_tac_simps]
theorem request_chat_length : (Array.to_slice live.REQUEST_CHAT).val.length = 9 := by unfold live.REQUEST_CHAT; rfl

theorem text_bytes : bytes (Array.to_slice live.TEXT).val = ascii ", text = " := by
  unfold live.TEXT; rfl

@[simp, scalar_tac_simps]
theorem text_length : (Array.to_slice live.TEXT).val.length = 9 := by unfold live.TEXT; rfl

theorem options_bytes : bytes (Array.to_slice live.OPTIONS).val = ascii ", options = {\n" := by
  unfold live.OPTIONS; rfl

@[simp, scalar_tac_simps]
theorem options_length : (Array.to_slice live.OPTIONS).val.length = 14 := by unfold live.OPTIONS; rfl

theorem request_end_bytes : bytes (Array.to_slice live.REQUEST_END).val = ascii "}},\n" := by
  unfold live.REQUEST_END; rfl

@[simp, scalar_tac_simps]
theorem request_end_length : (Array.to_slice live.REQUEST_END).val.length = 4 := by unfold live.REQUEST_END; rfl

theorem option_bytes : bytes (Array.to_slice live.OPTION).val = ascii "{id = " := by
  unfold live.OPTION; rfl

@[simp, scalar_tac_simps]
theorem option_length : (Array.to_slice live.OPTION).val.length = 6 := by unfold live.OPTION; rfl

theorem kind_bytes : bytes (Array.to_slice live.KIND).val = ascii ", kind = " := by
  unfold live.KIND; rfl

@[simp, scalar_tac_simps]
theorem kind_length : (Array.to_slice live.KIND).val.length = 9 := by unfold live.KIND; rfl

theorem label_bytes : bytes (Array.to_slice live.LABEL).val = ascii ", label = " := by
  unfold live.LABEL; rfl

@[simp, scalar_tac_simps]
theorem label_length : (Array.to_slice live.LABEL).val.length = 10 := by unfold live.LABEL; rfl

theorem option_end_bytes : bytes (Array.to_slice live.OPTION_END).val = ascii "},\n" := by
  unfold live.OPTION_END; rfl

@[simp, scalar_tac_simps]
theorem option_end_length : (Array.to_slice live.OPTION_END).val.length = 3 := by unfold live.OPTION_END; rfl

theorem allow_once_bytes : bytes (Array.to_slice live.ALLOW_ONCE).val = luaLiteral (ascii "allow_once") := by
  unfold live.ALLOW_ONCE; decide

@[simp, scalar_tac_simps]
theorem allow_once_length : (Array.to_slice live.ALLOW_ONCE).val.length = 12 := by unfold live.ALLOW_ONCE; rfl

theorem allow_always_bytes : bytes (Array.to_slice live.ALLOW_ALWAYS).val = luaLiteral (ascii "allow_always") := by
  unfold live.ALLOW_ALWAYS; decide

@[simp, scalar_tac_simps]
theorem allow_always_length : (Array.to_slice live.ALLOW_ALWAYS).val.length = 14 := by unfold live.ALLOW_ALWAYS; rfl

theorem reject_once_bytes : bytes (Array.to_slice live.REJECT_ONCE).val = luaLiteral (ascii "reject_once") := by
  unfold live.REJECT_ONCE; decide

@[simp, scalar_tac_simps]
theorem reject_once_length : (Array.to_slice live.REJECT_ONCE).val.length = 13 := by unfold live.REJECT_ONCE; rfl

theorem reject_always_bytes : bytes (Array.to_slice live.REJECT_ALWAYS).val = luaLiteral (ascii "reject_always") := by
  unfold live.REJECT_ALWAYS; decide

@[simp, scalar_tac_simps]
theorem reject_always_length : (Array.to_slice live.REJECT_ALWAYS).val.length = 15 := by unfold live.REJECT_ALWAYS; rfl

@[step]
theorem push_kind_spec (out : alloc.vec.Vec U8) (k : live.OptionKind) (hroom : out.val.length + 15 ≤ Usize.max) :
    live.push_kind out k ⦃ r =>
      bytes r.val = bytes out.val ++ luaLiteral (ascii (kindWord k)) ∧ r.val.length ≤ out.val.length + 15 ⦄ := by
  unfold live.push_kind
  induction k
  all_goals
    step*
    subst s_post
    refine ⟨?_, by simp at r_post2; omega⟩
    rw [r_post1, bytes, List.map_append]
    congr 1
  · exact allow_once_bytes
  · exact allow_always_bytes
  · exact reject_once_bytes
  · exact reject_always_bytes

theorem happ (a b : List U8) : bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]

def LinesBodyInv (lines : Slice (alloc.vec.Vec U8)) (base : Nat) (pre : List Spec.Byte)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ lines.val.length ∧
    bytes st.1.val = pre ++ (lines.val.take st.2.val).flatMap lineItem ∧
    st.1.val.length ≤ base + 804 * st.2.val

@[step]
theorem push_lines_spec (out : alloc.vec.Vec U8) (lines : Slice (alloc.vec.Vec U8))
    (hroom : out.val.length ≤ 2 ^ 30) (hn : lines.val.length ≤ maxLines)
    (hall : ∀ l ∈ lines.val, l.val.length ≤ maxLine) :
    live.push_lines out lines ⦃ r =>
      bytes r.val = bytes out.val ++ lines.val.flatMap lineItem ∧ r.val.length ≤ out.val.length + 4020 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxLines, maxLine] at hn hall
  unfold live.push_lines live.push_lines_loop
  apply loop.spec_decr_nat (fun st => lines.val.length - st.2.val)
    (LinesBodyInv lines out.val.length (bytes out.val)) _ _ _ _ ⟨by simp, by simp, by simp⟩
  have hline : ∀ (j : Nat) (hj : j < lines.val.length), (lines.val[j]).val.length ≤ 200 :=
    fun j hj => hall _ (List.getElem_mem hj)
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold live.push_lines_loop.body
  step*
  all_goals try (subst_vars; have := hline i.val (by scalar_tac); simp only [Protocol.Seen.deref_val, line_end_length] at *; scalar_tac)
  · have hlt : i.val < lines.val.length := by scalar_tac
    have hv := hline i.val hlt
    rw [← v_post] at hv
    subst s2_post
    simp only [Protocol.Seen.deref_val, line_end_length] at *
    unfold LinesBodyInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out2_post1, happ, out1_post1, happ, hout, v1_post1, line_end_bytes, i2_post,
      List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append, ← v_post]
    simp [lineItem]
  · have : i.val = lines.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

@[step]
theorem push_progress_spec (out : alloc.vec.Vec U8) (p : live.Progress)
    (hroom : out.val.length ≤ 2 ^ 29) (hfit : fitsProgress p) :
    live.push_progress out p ⦃ r =>
      bytes r.val = bytes out.val ++ progressLine p ∧ r.val.length ≤ out.val.length + 4190 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hfit' := hfit
  obtain ⟨hchat, hn, hall⟩ := hfit'
  unfold live.push_progress
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, chat_length, id_length, lines_length, progress_end_length] at *; scalar_tac)
  subst s_post s3_post s4_post s6_post
  have hb : bytes r.val = bytes out.val ++ progressLine p := by
    simp only [r_post1, out6_post1, out5_post1, out4_post1, out3_post1, out2_post1, out1_post1, happ,
      v_post1, chat_bytes, id_bytes, lines_bytes, progress_end_bytes, progressLine, List.append_assoc,
      Protocol.Seen.deref_val]
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := progressLine_length p hfit
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def ProgressAllInv (progress : Slice live.Progress) (pre : List Spec.Byte)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ progress.val.length ∧
    bytes st.1.val = pre ++ (progress.val.take st.2.val).flatMap progressLine ∧
    st.1.val.length ≤ 100 + 4190 * st.2.val

@[step]
theorem push_progress_all_spec (out : alloc.vec.Vec U8) (progress : Slice live.Progress)
    (hroom : out.val.length ≤ 100) (hn : progress.val.length ≤ maxProgress)
    (hall : ∀ p ∈ progress.val, fitsProgress p) :
    live.push_progress_all out progress ⦃ r =>
      bytes r.val = bytes out.val ++ progress.val.flatMap progressLine ∧
      r.val.length ≤ 100 + 4190 * 30 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxProgress] at hn
  unfold live.push_progress_all live.push_progress_all_loop
  apply loop.spec_decr_nat (fun st => progress.val.length - st.2.val)
    (ProgressAllInv progress (bytes out.val)) _ _ _ _ ⟨by simp, by simp, by simp; omega⟩
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold live.push_progress_all_loop.body
  step*
  · rw [p_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < progress.val.length := by scalar_tac
    unfold ProgressAllInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← p_post]
    simp
  · have : i.val = progress.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

@[step]
theorem push_option_spec (out : alloc.vec.Vec U8) (o : live.PermOption)
    (hroom : out.val.length ≤ 2 ^ 29) (hfit : fitsOption o) :
    live.push_option out o ⦃ r =>
      bytes r.val = bytes out.val ++ optionLine o ∧ r.val.length ≤ out.val.length + 431 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hfit' := hfit
  obtain ⟨hid, hlabel⟩ := hfit'
  simp only [maxLabel] at hlabel
  unfold live.push_option
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, option_length, kind_length, label_length, option_end_length] at *; scalar_tac)
  subst s_post s3_post s4_post s7_post
  have hb : bytes r.val = bytes out.val ++ optionLine o := by
    simp only [r_post1, out6_post1, out5_post1, out4_post1, out3_post1, out2_post1, out1_post1, happ,
      v_post1, v1_post1, option_bytes, kind_bytes, label_bytes, option_end_bytes, optionLine,
      List.append_assoc, Protocol.Seen.deref_val]
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := optionLine_length o hfit
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def OptionsInv (options : Slice live.PermOption) (base : Nat) (pre : List Spec.Byte)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ options.val.length ∧
    bytes st.1.val = pre ++ (options.val.take st.2.val).flatMap optionLine ∧
    st.1.val.length ≤ base + 431 * st.2.val

@[step]
theorem push_options_spec (out : alloc.vec.Vec U8) (options : Slice live.PermOption)
    (hroom : out.val.length ≤ 2 ^ 28) (hn : options.val.length ≤ maxOptions)
    (hall : ∀ o ∈ options.val, fitsOption o) :
    live.push_options out options ⦃ r =>
      bytes r.val = bytes out.val ++ options.val.flatMap optionLine ∧
      r.val.length ≤ out.val.length + 1724 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxOptions] at hn
  unfold live.push_options live.push_options_loop
  apply loop.spec_decr_nat (fun st => options.val.length - st.2.val)
    (OptionsInv options out.val.length (bytes out.val)) _ _ _ _ ⟨by simp, by simp, by simp⟩
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold live.push_options_loop.body
  step*
  · rw [po_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < options.val.length := by scalar_tac
    unfold OptionsInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← po_post]
    simp
  · have : i.val = options.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

@[step]
theorem push_request_spec (out : alloc.vec.Vec U8) (r : live.Request)
    (hroom : out.val.length ≤ 2 ^ 27) (hfit : fitsRequest r) :
    live.push_request out r ⦃ w =>
      bytes w.val = bytes out.val ++ requestLine r ∧ w.val.length ≤ out.val.length + 10050 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hfit' := hfit
  obtain ⟨hreq, hchat, htext, hn, hall⟩ := hfit'
  simp only [maxPopup] at htext
  unfold live.push_request
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, request_length, request_chat_length, id_length, text_length, options_length, request_end_length] at *; scalar_tac)
  subst s_post s3_post s6_post s7_post s10_post s12_post
  have hb : bytes w.val = bytes out.val ++ requestLine r := by
    simp only [w_post1, out10_post1, out9_post1, out8_post1, out7_post1, out6_post1, out5_post1,
      out4_post1, out3_post1, out2_post1, out1_post1, happ, v_post1, v1_post1, v2_post1, request_bytes,
      request_chat_bytes, id_bytes, text_bytes, options_bytes, request_end_bytes, requestLine,
      List.append_assoc, Protocol.Seen.deref_val]
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := requestLine_length r hfit
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def RequestsInv (requests : Slice live.Request) (base : Nat) (pre : List Spec.Byte)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ requests.val.length ∧
    bytes st.1.val = pre ++ (requests.val.take st.2.val).flatMap requestLine ∧
    st.1.val.length ≤ base + 10050 * st.2.val

@[step]
theorem push_requests_spec (out : alloc.vec.Vec U8) (requests : Slice live.Request)
    (hroom : out.val.length ≤ 2 ^ 26) (hn : requests.val.length ≤ maxRequests)
    (hall : ∀ r ∈ requests.val, fitsRequest r) :
    live.push_requests out requests ⦃ w =>
      bytes w.val = bytes out.val ++ requests.val.flatMap requestLine ∧
      w.val.length ≤ out.val.length + 40200 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxRequests] at hn
  unfold live.push_requests live.push_requests_loop
  apply loop.spec_decr_nat (fun st => requests.val.length - st.2.val)
    (RequestsInv requests out.val.length (bytes out.val)) _ _ _ _ ⟨by simp, by simp, by simp⟩
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold live.push_requests_loop.body
  step*
  · rw [r_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < requests.val.length := by scalar_tac
    unfold RequestsInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← r_post]
    simp
  · have : i.val = requests.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

/-- **S20.** -/
theorem live_body_spec (progress : Slice live.Progress) (requests : Slice live.Request)
    (hfits : fitsLive progress.val requests.val) :
    live.live_body progress requests ⦃ v => bytes v.val = liveBytes progress.val requests.val ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  obtain ⟨hp, hpall, hr, hrall⟩ := hfits
  unfold live.live_body
  step*
  subst s_post s1_post s2_post
  rw [v_post1, happ, out3_post1, out2_post1, happ, out1_post1, out_post1, List.nil_append, head_bytes,
    permissions_bytes, tail_bytes]
  simp [liveBytes]

end Protocol.Live
