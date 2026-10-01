-- Up and Down in the input go through the earlier messages of the chat, as in a shell
-- (SPEC.md 13.1).
local _, ns = ...

local InputHistory = {}
ns.InputHistory = InputHistory

-- `back` counts the steps back from the draft. At 0 the input shows the draft.
local state = { chat = nil, back = 0, draft = "" }

local function IsTyped(entry)
	return entry.role == "user" and not entry.attach and not entry.git and entry.text ~= ""
end

-- Newest first. Two same messages in a row count once.
local function SentTexts(chat)
	local texts = {}
	for i = #chat.history, 1, -1 do
		local entry = chat.history[i]
		if IsTyped(entry) and entry.text ~= texts[#texts] then
			table.insert(texts, entry.text)
		end
	end
	return texts
end

function InputHistory.Reset()
	state.chat = nil
	state.back = 0
	state.draft = ""
end

-- Returns nil at the oldest message, and the input then keeps its text.
function InputHistory.Older(chat, typed)
	if state.chat ~= chat.id then
		InputHistory.Reset()
		state.chat = chat.id
	end
	local texts = SentTexts(chat)
	if state.back >= #texts then
		return nil
	end
	if state.back == 0 then
		state.draft = typed
	end
	state.back = state.back + 1
	return texts[state.back]
end

-- Returns nil at the draft, and the input then keeps its text.
function InputHistory.Newer(chat)
	if state.chat ~= chat.id or state.back == 0 then
		return nil
	end
	state.back = state.back - 1
	if state.back == 0 then
		return state.draft
	end
	return SentTexts(chat)[state.back]
end
