-- The quick actions as suggestions in the center of an empty chat (SPEC.md 13.1).

local _, ns = ...

local Suggestions = {}
ns.Suggestions = Suggestions

local ROW_HEIGHT = 26
local ROW_GAP = 6
local HINT_HEIGHT = 24
local MARGIN = 24
local PAD = 10

local ui = { rows = {} }

-- The row cuts a long message, so the tooltip shows all of it.
local function ShowTooltip(row)
	if not row.action then
		return
	end
	GameTooltip:SetOwner(row, "ANCHOR_TOP")
	GameTooltip:SetText(row.action.message, 1, 1, 1, 1, true)
	GameTooltip:Show()
end

-- A click is a typed message: Window.Send signs it and checks its size.
local function Send(row)
	if row.action then
		ns.Window.Send(row.action.message)
	end
end

local function NewRow(i)
	local row = CreateFrame("Button", "GnomishRelaySuggestion" .. i, ui.frame)
	row:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 0, -HINT_HEIGHT - (i - 1) * (ROW_HEIGHT + ROW_GAP))
	row:SetHeight(ROW_HEIGHT)
	local background = row:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.1, 0.15, 0.2, 0.9)
	row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	row.label = row:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	row.label:SetPoint("LEFT", row, "LEFT", PAD, 0)
	row.label:SetJustifyH("LEFT")
	row.label:SetWordWrap(false)
	row:SetScript("OnClick", Send)
	row:SetScript("OnEnter", ShowTooltip)
	row:SetScript("OnLeave", function()
		GameTooltip:Hide()
	end)
	return row
end

local function Layout(actions)
	for i, row in ipairs(ui.rows) do
		local action = actions[i]
		row.action = action
		row:SetShown(action ~= nil)
		if action then
			row.label:SetText(action.message)
		end
	end
	ui.frame:SetHeight(HINT_HEIGHT + #actions * (ROW_HEIGHT + ROW_GAP))
end

local function IsEmpty(chat)
	return not chat or #chat.history == 0
end

function Suggestions.Refresh(chat)
	if not ui.frame then
		return
	end
	local actions = ns.QuickActions.Ready()
	local shown = IsEmpty(chat) and #actions > 0
	ui.frame:SetShown(shown)
	if shown then
		Layout(actions)
	end
end

-- `width` is the width of the transcript.
function Suggestions.Resize(width)
	local rowWidth = width - 2 * MARGIN
	ui.frame:SetWidth(rowWidth)
	for _, row in ipairs(ui.rows) do
		row:SetWidth(rowWidth)
		row.label:SetWidth(rowWidth - 2 * PAD)
	end
end

-- `parent` is the inset of the transcript.
function Suggestions.Build(parent, width)
	ui.frame = CreateFrame("Frame", "GnomishRelaySuggestions", parent)
	ui.frame:SetPoint("CENTER", parent, "CENTER", 0, 0)
	local hint = ui.frame:CreateFontString("GnomishRelaySuggestionsHint", "OVERLAY", "GameFontDisable")
	hint:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 2, 0)
	hint:SetText("Try one of these:")
	for i = 1, ns.QuickActions.MOST do
		ui.rows[i] = NewRow(i)
	end
	Suggestions.Resize(width)
	ui.frame:Hide()
end
