-- The search of the chat on screen (SPEC.md 13.1): the bar above the input, the Search
-- button of the header, and the matches. One entry is one match.

local _, ns = ...

local Search = {}
ns.Search = Search

local ui = { open = false, query = "" }
-- The lowercase plain words of each entry. An entry never changes its text.
local words = setmetatable({}, { __mode = "k" })

local function WordsOf(entry)
	if entry.attach then
		return ""
	end
	if not words[entry] then
		local text = tostring(entry.text or "")
		if ns.Blocks.IsRendered(text) then
			text = ns.Blocks.Plain(text)
		end
		words[entry] = text:lower()
	end
	return words[entry]
end

local function History()
	local chat = ns.Window.SelectedChat()
	return chat and chat.history or {}
end

-- The indexes of the entries that hold the query, top to bottom.
local function Matches(history)
	local matches = {}
	for i, entry in ipairs(history) do
		if WordsOf(entry):find(ui.query, 1, true) then
			table.insert(matches, i)
		end
	end
	return matches
end

local function PositionOf(matches, history)
	for position, i in ipairs(matches) do
		if history[i] == ui.current then
			return position
		end
	end
end

-- `step` is -1 for the match above, 1 for the match below, and 0 for the newest match.
local function Go(step)
	local history = History()
	local matches = ui.query ~= "" and Matches(history) or {}
	if #matches == 0 then
		ui.current = nil
		ns.Transcript.Unmark()
		ui.count:SetText(ui.query ~= "" and "No matches" or "")
		return
	end
	local position = step ~= 0 and PositionOf(matches, history)
	if position then
		position = (position - 1 + step) % #matches + 1
	else
		position = #matches
	end
	ui.current = history[matches[position]]
	ns.Transcript.JumpTo(ui.current)
	ui.count:SetText(string.format("%d of %d", position, #matches))
end

function Search.IsOpen()
	return ui.open
end

function Search.Open()
	ui.open = true
	ns.Window.Refresh()
	ui.box:SetFocus()
end

-- The caller draws the window again.
function Search.Close()
	if not ui.open then
		return
	end
	ui.open = false
	ui.box:SetText("")
	ui.box:ClearFocus()
	ns.Transcript.Unmark()
end

local function Dismiss()
	Search.Close()
	ns.Window.Refresh()
end

-- The window shows the bar only on the Chats tab, with the transcript.
function Search.Show(shown)
	ui.bar:SetShown(shown and ui.open)
end

local function OnTextChanged(box)
	ui.query = strtrim(box:GetText() or ""):lower()
	ui.hint:SetShown(ui.query == "")
	Go(0)
end

local function NewButton(name, text, width, onClick)
	local button = CreateFrame("Button", name, ui.bar, "UIPanelButtonTemplate")
	button:SetSize(width, 20)
	button:SetText(text)
	button:SetScript("OnClick", onClick)
	return button
end

local function BuildBox()
	ui.box = CreateFrame("EditBox", "GnomishRelaySearchBox", ui.bar, "InputBoxTemplate")
	ui.box:SetPoint("LEFT", ui.bar, "LEFT", 6, 0)
	ui.box:SetSize(150, 20)
	ui.box:SetAutoFocus(false)
	ui.box:SetMaxBytes(200)
	ui.hint = ui.box:CreateFontString("GnomishRelaySearchHint", "OVERLAY", "GameFontDisableSmall")
	ui.hint:SetPoint("LEFT", ui.box, "LEFT", 2, 0)
	ui.hint:SetText("Search this chat")
	ui.box:SetScript("OnTextChanged", OnTextChanged)
	-- Enter gives the keys back to the game. Previous and Next still work.
	ui.box:SetScript("OnEnterPressed", function(self)
		self:ClearFocus()
	end)
	ui.box:SetScript("OnEscapePressed", Dismiss)
end

local function BuildBar(frame, left, bottom)
	ui.bar = CreateFrame("Frame", "GnomishRelaySearch", frame)
	ui.bar:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left, bottom)
	ui.bar:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -left, bottom)
	ui.bar:SetHeight(20)
	BuildBox()
	ui.count = ui.bar:CreateFontString("GnomishRelaySearchCount", "OVERLAY", "GameFontHighlightSmall")
	ui.count:SetPoint("LEFT", ui.box, "RIGHT", 10, 0)
	local close = NewButton("GnomishRelaySearchClose", "Close", 60, Dismiss)
	close:SetPoint("RIGHT", ui.bar, "RIGHT", 0, 0)
	local nextMatch = NewButton("GnomishRelaySearchNext", "Next", 60, function()
		Go(1)
	end)
	nextMatch:SetPoint("RIGHT", close, "LEFT", -4, 0)
	local previous = NewButton("GnomishRelaySearchPrevious", "Previous", 76, function()
		Go(-1)
	end)
	previous:SetPoint("RIGHT", nextMatch, "LEFT", -4, 0)
	ui.bar:Hide()
end

-- `anchor` is the Pinned button: Search sits at its left in the header.
local function BuildButton(frame, anchor)
	local button = CreateFrame("Button", "GnomishRelaySearchButton", frame)
	button:SetPoint("RIGHT", anchor, "LEFT", -14, 0)
	button:SetHeight(18)
	button.label = button:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
	button.label:SetPoint("RIGHT", button, "RIGHT", 0, 0)
	button.label:SetText("Search")
	button:SetWidth(button.label:GetUnboundedStringWidth() + 4)
	button:SetScript("OnClick", Search.Open)
	return button
end

-- The bar sits at `bottom` above the lower edge of `frame`, `left` in from each side.
-- Returns the Search button of the header.
function Search.Build(frame, left, bottom, anchor)
	BuildBar(frame, left, bottom)
	return BuildButton(frame, anchor)
end
