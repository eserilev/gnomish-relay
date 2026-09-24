import Protocol.Slot
import Protocol.Spec.Restore

/-! # The restore bundle (S18, S19) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Restore

@[simp, scalar_tac_simps]
theorem max_chats_val : restore.MAX_CHATS.val = 16 := by unfold restore.MAX_CHATS; rfl

@[simp, scalar_tac_simps]
theorem max_history_val : restore.MAX_HISTORY.val = 10 := by unfold restore.MAX_HISTORY; rfl

@[simp, scalar_tac_simps]
theorem max_name_val : restore.MAX_NAME.val = 64 := by unfold restore.MAX_NAME; rfl

@[simp, scalar_tac_simps]
theorem max_cwd_val : restore.MAX_CWD.val = 1024 := by unfold restore.MAX_CWD; rfl

@[simp, scalar_tac_simps]
theorem max_entry_text_val : restore.MAX_ENTRY_TEXT.val = 500 := by unfold restore.MAX_ENTRY_TEXT; rfl

theorem take_min_bytes (l : List U8) (n : Nat) : bytes (l.take (min l.length n)) = (bytes l).take n := by
  rcases le_total l.length n with h | h
  · rw [min_eq_left h, List.take_length, List.take_of_length_le (by simp [bytes, h])]
  · rw [min_eq_right h, bytes, bytes, List.map_take]

@[step]
theorem cut_spec (s : Slice U8) (max : Usize) :
    restore.cut s max ⦃ v => bytes v.val = (bytes s.val).take max.val ∧ v.val.length ≤ max.val ⦄ := by
  unfold restore.cut
  step*
  have hv : v.val = s.val.take (min s.val.length max.val) := by rw [v_post, i1_post]; simp
  exact ⟨by rw [hv]; exact take_min_bytes _ _, by rw [hv]; simp⟩

@[step]
theorem keep_from_spec (len max : Usize) : restore.keep_from len max ⦃ r => r.val = len.val - max.val ⦄ := by
  unfold restore.keep_from
  step*
  scalar_tac

@[step]
theorem prepare_entry_spec (e : restore.Entry) :
    restore.prepare_entry e ⦃ r => entryFrom e r ∧ r.text.val.length ≤ maxEntryText ⦄ := by
  unfold restore.prepare_entry
  step*
  simp only [Protocol.Seen.deref_val] at v_post1
  exact ⟨⟨rfl, rfl, by simpa [maxEntryText] using v_post1⟩, by simpa [maxEntryText] using v_post2⟩

def HistInv (history : Slice restore.Entry) (s : Nat) (st : alloc.vec.Vec restore.Entry × Usize) : Prop :=
  s ≤ st.2.val ∧ st.2.val ≤ max s history.val.length ∧ st.1.val.length = st.2.val - s ∧
    List.Forall₂ entryFrom ((history.val.drop s).take (st.2.val - s)) st.1.val ∧
    ∀ e ∈ st.1.val, e.text.val.length ≤ maxEntryText

@[step]
theorem prepare_history_spec (history : Slice restore.Entry) :
    restore.prepare_history history ⦃ ps =>
      ps.val.length ≤ maxHistory ∧ (∀ e ∈ ps.val, e.text.val.length ≤ maxEntryText) ∧
      List.Forall₂ entryFrom (history.val.drop (history.val.length - maxHistory)) ps.val ⦄ := by
  unfold restore.prepare_history
  step*
  unfold restore.prepare_history_loop
  apply loop.spec_decr_nat (fun st => history.val.length - st.2.val) (HistInv history i1.val) _ _ _ _
    ⟨le_refl _, le_max_left _ _, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hlen, hf, hfit⟩
  simp only at hs hi hlen hf hfit
  unfold restore.prepare_history_loop.body
  step*
  · have hlt : i.val < history.val.length := by scalar_tac
    unfold HistInv
    dsimp only
    have htake : (history.val.drop i1.val).take (i2.val - i1.val) =
        (history.val.drop i1.val).take (i.val - i1.val) ++ [history.val[i.val]] := by
      rw [i2_post, show i.val + 1 - i1.val = (i.val - i1.val) + 1 by omega, List.take_add_one,
        List.getElem?_drop, show i1.val + (i.val - i1.val) = i.val by omega,
        List.getElem?_eq_getElem hlt]
      rfl
    refine ⟨⟨by omega, by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (e_post ▸ e1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact e1_post2
  · have hs10 : i1.val = history.val.length - maxHistory := by simp [maxHistory]; scalar_tac
    have hn : i.val = history.val.length := by scalar_tac
    rw [hn, List.take_of_length_le (by simp)] at hf
    refine ⟨?_, hfit, by rw [← hs10]; exact hf⟩
    rw [hlen, hn, hs10, maxHistory]
    omega

@[step]
theorem prepare_chat_spec (c : restore.Chat) :
    restore.prepare_chat c ⦃ r => chatFrom c r ∧ fitsChat r ⦄ := by
  unfold restore.prepare_chat
  step*
  simp only [Protocol.Seen.deref_val, Protocol.Slot.max_id_len_val, max_name_val, max_cwd_val] at *
  exact ⟨⟨v_post1, v1_post1, v2_post1, v3_post1, v4_post3⟩,
    v_post2, v1_post2, v2_post2, v3_post2, v4_post1, v4_post2⟩

def ChatsInv (chats : Slice restore.Chat) (s : Nat) (st : alloc.vec.Vec restore.Chat × Usize) : Prop :=
  s ≤ st.2.val ∧ st.2.val ≤ max s chats.val.length ∧ st.1.val.length = st.2.val - s ∧
    List.Forall₂ chatFrom ((chats.val.drop s).take (st.2.val - s)) st.1.val ∧
    ∀ c ∈ st.1.val, fitsChat c

/-- **S18, prepare.** -/
theorem prepare_restore_spec (chats : Slice restore.Chat) :
    restore.prepare_restore chats ⦃ ps =>
      ps.val.length ≤ maxChats ∧ (∀ c ∈ ps.val, fitsChat c) ∧
      List.Forall₂ chatFrom (chats.val.drop (chats.val.length - maxChats)) ps.val ⦄ := by
  unfold restore.prepare_restore
  step*
  unfold restore.prepare_restore_loop
  apply loop.spec_decr_nat (fun st => chats.val.length - st.2.val) (ChatsInv chats i1.val) _ _ _ _
    ⟨le_refl _, le_max_left _ _, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hlen, hf, hfit⟩
  simp only at hs hi hlen hf hfit
  unfold restore.prepare_restore_loop.body
  step*
  · have hlt : i.val < chats.val.length := by scalar_tac
    unfold ChatsInv
    dsimp only
    have htake : (chats.val.drop i1.val).take (i2.val - i1.val) =
        (chats.val.drop i1.val).take (i.val - i1.val) ++ [chats.val[i.val]] := by
      rw [i2_post, show i.val + 1 - i1.val = (i.val - i1.val) + 1 by omega, List.take_add_one,
        List.getElem?_drop, show i1.val + (i.val - i1.val) = i.val by omega,
        List.getElem?_eq_getElem hlt]
      rfl
    refine ⟨⟨by omega, by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (c_post ▸ c1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact c1_post2
  · have hs16 : i1.val = chats.val.length - maxChats := by simp [maxChats]; scalar_tac
    have hn : i.val = chats.val.length := by scalar_tac
    rw [hn, List.take_of_length_le (by simp)] at hf
    refine ⟨?_, hfit, by rw [← hs16]; exact hf⟩
    rw [hlen, hn, hs16, maxChats]
    omega

/-! ## Size bound (pure) -/

theorem role_literal_length (r : restore.Role) : (luaLiteral (ascii (roleWord r))).length ≤ 7 := by
  cases r <;> decide

theorem bytes_length (l : List U8) : (bytes l).length = l.length := by simp [bytes]

theorem entryLine_length (e : restore.Entry) (h : e.text.val.length ≤ maxEntryText) :
    (entryLine e).length ≤ 2046 := by
  have ht := Protocol.Slot.literal_length_le (bytes e.text.val)
  have hd := decimal_u32_length e.id
  have hr := role_literal_length e.role
  rw [bytes_length] at ht
  simp only [maxEntryText] at h
  simp only [entryLine, List.length_append]
  have : (ascii "{role = ").length = 8 := rfl
  have : (ascii ", id = ").length = 7 := rfl
  have : (ascii ", text = ").length = 9 := rfl
  have : (ascii "},\n").length = 3 := rfl
  omega

theorem sum_le {α : Type} (f : α → List Spec.Byte) (k : Nat) (l : List α)
    (h : ∀ a ∈ l, (f a).length ≤ k) : (l.flatMap f).length ≤ k * l.length := by
  rw [List.length_flatMap]
  have : ∀ x ∈ l.map (fun a => (f a).length), x ≤ k := by
    intro x hx
    obtain ⟨a, ha, rfl⟩ := List.mem_map.mp hx
    exact h a ha
  have := List.sum_le_card_nsmul _ _ this
  simpa [mul_comm] using this

theorem chatLine_length (c : restore.Chat) (h : fitsChat c) : (chatLine c).length ≤ 25127 := by
  obtain ⟨hid, hname, hagent, hcwd, hn, hall⟩ := h
  have hi := Protocol.Slot.literal_length_le (bytes c.id.val)
  have hm := Protocol.Slot.literal_length_le (bytes c.name.val)
  have ha := Protocol.Slot.literal_length_le (bytes c.agent.val)
  have hc := Protocol.Slot.literal_length_le (bytes c.cwd.val)
  have hh := sum_le entryLine 2046 c.history.val (fun e he => entryLine_length e (hall e he))
  rw [bytes_length] at hi hm ha hc
  simp only [maxName, maxCwd, maxHistory] at hname hcwd hn
  simp only [chatLine, List.length_append]
  have : (ascii "{id = ").length = 6 := rfl
  have : (ascii ", name = ").length = 9 := rfl
  have : (ascii ", agent = ").length = 10 := rfl
  have : (ascii ", cwd = ").length = 8 := rfl
  have : (ascii ", history = {\n").length = 14 := rfl
  have : (ascii "}},\n").length = 4 := rfl
  have : 2046 * c.history.val.length ≤ 20460 := by omega
  omega

/-- **S19.** -/
theorem restore_bound (token : List Spec.Byte) (chats : List restore.Chat) (h : fitsRestore token chats) :
    (restoreBytes token chats).length ≤ restoreLimit := by
  obtain ⟨htok, hn, hall⟩ := h
  have ht := Protocol.Slot.literal_length_le token
  have hc := sum_le chatLine 25127 chats (fun c hc => chatLine_length c (hall c hc))
  simp only [maxChats] at hn
  simp only [restoreBytes, restoreLimit, List.length_append]
  have : (ascii "GnomishRelay_Restore = {token = ").length = 32 := rfl
  have : (ascii ", chats = {\n").length = 12 := rfl
  have : (ascii "}}\n").length = 3 := rfl
  have : 25127 * chats.length ≤ 402032 := by omega
  omega

/-! ## The template (S18) -/

theorem head_bytes : bytes (Array.to_slice restore.HEAD).val = ascii "GnomishRelay_Restore = {token = " := by
  unfold restore.HEAD; rfl

@[simp, scalar_tac_simps]
theorem head_length : (Array.to_slice restore.HEAD).val.length = 32 := by unfold restore.HEAD; rfl

theorem chats_bytes : bytes (Array.to_slice restore.CHATS).val = ascii ", chats = {\n" := by
  unfold restore.CHATS; rfl

@[simp, scalar_tac_simps]
theorem chats_length : (Array.to_slice restore.CHATS).val.length = 12 := by unfold restore.CHATS; rfl

theorem tail_bytes : bytes (Array.to_slice restore.TAIL).val = ascii "}}\n" := by
  unfold restore.TAIL; rfl

@[simp, scalar_tac_simps]
theorem tail_length : (Array.to_slice restore.TAIL).val.length = 3 := by unfold restore.TAIL; rfl

theorem id_bytes : bytes (Array.to_slice restore.ID).val = ascii "{id = " := by
  unfold restore.ID; rfl

@[simp, scalar_tac_simps]
theorem id_length : (Array.to_slice restore.ID).val.length = 6 := by unfold restore.ID; rfl

theorem name_bytes : bytes (Array.to_slice restore.NAME).val = ascii ", name = " := by
  unfold restore.NAME; rfl

@[simp, scalar_tac_simps]
theorem name_length : (Array.to_slice restore.NAME).val.length = 9 := by unfold restore.NAME; rfl

theorem agent_bytes : bytes (Array.to_slice restore.AGENT).val = ascii ", agent = " := by
  unfold restore.AGENT; rfl

@[simp, scalar_tac_simps]
theorem agent_length : (Array.to_slice restore.AGENT).val.length = 10 := by unfold restore.AGENT; rfl

theorem cwd_bytes : bytes (Array.to_slice restore.CWD).val = ascii ", cwd = " := by
  unfold restore.CWD; rfl

@[simp, scalar_tac_simps]
theorem cwd_length : (Array.to_slice restore.CWD).val.length = 8 := by unfold restore.CWD; rfl

theorem history_bytes : bytes (Array.to_slice restore.HISTORY).val = ascii ", history = {\n" := by
  unfold restore.HISTORY; rfl

@[simp, scalar_tac_simps]
theorem history_length : (Array.to_slice restore.HISTORY).val.length = 14 := by unfold restore.HISTORY; rfl

theorem chat_end_bytes : bytes (Array.to_slice restore.CHAT_END).val = ascii "}},\n" := by
  unfold restore.CHAT_END; rfl

@[simp, scalar_tac_simps]
theorem chat_end_length : (Array.to_slice restore.CHAT_END).val.length = 4 := by unfold restore.CHAT_END; rfl

theorem role_bytes : bytes (Array.to_slice restore.ROLE).val = ascii "{role = " := by
  unfold restore.ROLE; rfl

@[simp, scalar_tac_simps]
theorem role_length : (Array.to_slice restore.ROLE).val.length = 8 := by unfold restore.ROLE; rfl

theorem entry_id_bytes : bytes (Array.to_slice restore.ENTRY_ID).val = ascii ", id = " := by
  unfold restore.ENTRY_ID; rfl

@[simp, scalar_tac_simps]
theorem entry_id_length : (Array.to_slice restore.ENTRY_ID).val.length = 7 := by unfold restore.ENTRY_ID; rfl

theorem text_bytes : bytes (Array.to_slice restore.TEXT).val = ascii ", text = " := by
  unfold restore.TEXT; rfl

@[simp, scalar_tac_simps]
theorem text_length : (Array.to_slice restore.TEXT).val.length = 9 := by unfold restore.TEXT; rfl

theorem entry_end_bytes : bytes (Array.to_slice restore.ENTRY_END).val = ascii "},\n" := by
  unfold restore.ENTRY_END; rfl

@[simp, scalar_tac_simps]
theorem entry_end_length : (Array.to_slice restore.ENTRY_END).val.length = 3 := by unfold restore.ENTRY_END; rfl

theorem user_bytes : bytes (Array.to_slice restore.USER).val = luaLiteral (ascii "user") := by
  unfold restore.USER; decide

@[simp, scalar_tac_simps]
theorem user_length : (Array.to_slice restore.USER).val.length = 6 := by unfold restore.USER; rfl

theorem agent_role_bytes : bytes (Array.to_slice restore.AGENT_ROLE).val = luaLiteral (ascii "agent") := by
  unfold restore.AGENT_ROLE; decide

@[simp, scalar_tac_simps]
theorem agent_role_length : (Array.to_slice restore.AGENT_ROLE).val.length = 7 := by unfold restore.AGENT_ROLE; rfl

theorem error_bytes : bytes (Array.to_slice restore.ERROR).val = luaLiteral (ascii "error") := by
  unfold restore.ERROR; decide

@[simp, scalar_tac_simps]
theorem error_length : (Array.to_slice restore.ERROR).val.length = 7 := by unfold restore.ERROR; rfl

@[step]
theorem push_role_spec (out : alloc.vec.Vec U8) (role : restore.Role) (hroom : out.val.length + 7 ≤ Usize.max) :
    restore.push_role out role ⦃ r =>
      bytes r.val = bytes out.val ++ luaLiteral (ascii (roleWord role)) ∧ r.val.length ≤ out.val.length + 7 ⦄ := by
  unfold restore.push_role
  induction role
  all_goals
    step*
    subst s_post
    refine ⟨?_, by simp at r_post2; omega⟩
    rw [r_post1, bytes, List.map_append]
    congr 1
  · exact user_bytes
  · exact agent_role_bytes
  · exact error_bytes

@[step]
theorem push_entry_spec (out : alloc.vec.Vec U8) (e : restore.Entry)
    (hroom : out.val.length ≤ 2 ^ 31) (hfit : e.text.val.length ≤ maxEntryText) :
    restore.push_entry out e ⦃ r =>
      bytes r.val = bytes out.val ++ entryLine e ∧ r.val.length ≤ out.val.length + 2046 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxEntryText] at hfit
  unfold restore.push_entry
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, role_length, entry_id_length,
    text_length, entry_end_length] at *; scalar_tac)
  have happ : ∀ a b : List U8, bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]
  have hderef : ∀ w : alloc.vec.Vec U8, bytes (alloc.vec.Vec.deref w).val = bytes w.val := fun _ => rfl
  subst s_post s1_post s2_post s5_post
  have hb : bytes r.val = bytes out.val ++ entryLine e := by
    rw [r_post1, happ, out6_post1, happ, out5_post1, happ, out4_post1, out3_post1, happ, out2_post1,
      out1_post1, happ, hderef, v_post1, role_bytes, entry_id_bytes, text_bytes, entry_end_bytes]
    simp only [entryLine, List.append_assoc]
    rfl
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := entryLine_length e (by simpa [maxEntryText] using hfit)
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def HistoryInv (history : Slice restore.Entry) (base : Nat) (pre : List Spec.Byte)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ history.val.length ∧
    bytes st.1.val = pre ++ (history.val.take st.2.val).flatMap entryLine ∧
    st.1.val.length ≤ base + 2046 * st.2.val

@[step]
theorem push_history_spec (out : alloc.vec.Vec U8) (history : Slice restore.Entry)
    (hroom : out.val.length + 2 ^ 16 ≤ 2 ^ 31) (hn : history.val.length ≤ maxHistory)
    (hall : ∀ e ∈ history.val, e.text.val.length ≤ maxEntryText) :
    restore.push_history out history ⦃ r =>
      bytes r.val = bytes out.val ++ history.val.flatMap entryLine ∧
      r.val.length ≤ out.val.length + 20460 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxHistory] at hn
  unfold restore.push_history restore.push_history_loop
  apply loop.spec_decr_nat (fun st => history.val.length - st.2.val)
    (HistoryInv history out.val.length (bytes out.val)) _ _ _ _ ⟨by simp, by simp, by simp⟩
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold restore.push_history_loop.body
  step*
  · rw [e_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < history.val.length := by scalar_tac
    unfold HistoryInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← e_post]
    simp
  · have : i.val = history.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

@[step]
theorem push_chat_spec (out : alloc.vec.Vec U8) (c : restore.Chat)
    (hroom : out.val.length ≤ 2 ^ 30) (hfit : fitsChat c) :
    restore.push_chat out c ⦃ r =>
      bytes r.val = bytes out.val ++ chatLine c ∧ r.val.length ≤ out.val.length + 25127 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  obtain ⟨hid, hname, hagent, hcwd, hn, hall⟩ := hfit
  simp only [maxName, maxCwd] at hname hcwd
  unfold restore.push_chat
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, id_length, name_length, agent_length,
    cwd_length, history_length, chat_end_length] at *; scalar_tac)
  have happ : ∀ a b : List U8, bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]
  subst s_post s3_post s6_post s9_post s12_post s14_post
  have hb : bytes r.val = bytes out.val ++ chatLine c := by
    simp only [r_post1, out10_post1, out9_post1, out8_post1, out7_post1, out6_post1, out5_post1,
      out4_post1, out3_post1, out2_post1, out1_post1, happ, v_post1, v1_post1, v2_post1,
      v3_post1, id_bytes, name_bytes, agent_bytes, cwd_bytes, history_bytes, chat_end_bytes, chatLine,
      List.append_assoc, Protocol.Seen.deref_val]
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := chatLine_length c ⟨hid, by simpa [maxName] using hname, hagent, by simpa [maxCwd] using hcwd, hn, hall⟩
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def ChatsBodyInv (chats : Slice restore.Chat) (pre : List Spec.Byte) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ chats.val.length ∧
    bytes st.1.val = pre ++ (chats.val.take st.2.val).flatMap chatLine ∧
    st.1.val.length ≤ 200 + 25127 * st.2.val

theorem restore_body_loop_spec (chats : Slice restore.Chat) (pre : List Spec.Byte)
    (out : alloc.vec.Vec U8) (hn : chats.val.length ≤ maxChats) (hall : ∀ c ∈ chats.val, fitsChat c)
    (hinv : ChatsBodyInv chats pre (out, 0#usize)) :
    restore.restore_body_loop chats out 0#usize ⦃ r =>
      bytes r.val = pre ++ chats.val.flatMap chatLine ∧ r.val.length ≤ 200 + 25127 * 16 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  simp only [maxChats] at hn
  unfold restore.restore_body_loop
  apply loop.spec_decr_nat (fun st => chats.val.length - st.2.val) (ChatsBodyInv chats pre) _ _ _ _ hinv
  rintro ⟨o, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold restore.restore_body_loop.body
  step*
  · rw [c_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < chats.val.length := by scalar_tac
    unfold ChatsBodyInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← c_post]
    simp
  · have : i.val = chats.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

/-- **S18.** -/
theorem restore_body_spec (token : Slice U8) (chats : Slice restore.Chat)
    (hfits : fitsRestore (bytes token.val) chats.val) :
    restore.restore_body token chats ⦃ v => bytes v.val = restoreBytes (bytes token.val) chats.val ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have happ : ∀ a b : List U8, bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]
  obtain ⟨htok, hn, hall⟩ := hfits
  rw [bytes_length] at htok
  unfold restore.restore_body
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, head_length, chats_length] at *; scalar_tac)
  have hderef : ∀ w : alloc.vec.Vec U8, bytes (alloc.vec.Vec.deref w).val = bytes w.val := fun _ => rfl
  subst s_post s2_post
  have hpre : bytes out2.val = ascii "GnomishRelay_Restore = {token = " ++ luaLiteral (bytes token.val) ++
      ascii ", chats = {\n" := by
    simp only [out2_post1, out1_post1, out_post1, happ, hderef, v_post1, head_bytes, chats_bytes,
      List.nil_append]
  have hlen : out2.val.length ≤ 200 := by
    simp only [Protocol.Seen.deref_val, head_length, chats_length, List.length_nil] at *
    omega
  step with restore_body_loop_spec chats (ascii "GnomishRelay_Restore = {token = " ++
      luaLiteral (bytes token.val) ++ ascii ", chats = {\n") out2 hn hall
      ⟨by simp, by simp [hpre], by simp [hlen]⟩ as ⟨out3, h3, h3len⟩
  step*
  subst s3_post
  simp only [v_post1, happ, h3, tail_bytes, restoreBytes, List.append_assoc]

end Protocol.Restore
