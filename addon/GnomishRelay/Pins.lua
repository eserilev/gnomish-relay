-- Pinned replies (SPEC.md 13.1): the Pinned button of the header and its list. A pin is
-- the `pinned` field of a reply in the history, so each chat has its own.

local _, ns = ...

local Pins = {}
ns.Pins = Pins

local ROWS = 12
local ROW_HEIGHT = 20
local WIDTH = 320

local ui = { rows = {}, offset = 0 }

-- The pinned replies of the chat on screen, oldest first.
local function Pinned()
	local chat = ns.Window.SelectedChat()
	local pinned = {}
	for _, entry in ipairs(chat and chat.history or {}) do
		if entry.pinned then
			table.insert(pinned, entry)
		end
	end
	return pinned
end

local function RefreshList()
	local pinned = Pinned()
	ui.offset = math.max(0, math.min(ui.offset, #pinned - ROWS))
	for i, row in ipairs(ui.rows) do
		local entry = pinned[ui.offset + i]
		row.entry = entry
		row.text:SetText(entry and ns.Relay.Snippet(entry.text) or "")
		row:SetShown(entry ~= nil)
	end
	ui.empty:SetShown(#pinned == 0)
	ui.list:SetHeight(math.max(1, math.min(ROWS, #pinned)) * ROW_HEIGHT + 8)
end

function Pins.Refresh()
	if not ui.button then
		return
	end
	ui.button.label:SetText("Pinned " .. #Pinned())
	ui.button:SetWidth(ui.button.label:GetUnboundedStringWidth() + 8)
	if ui.list:IsShown() then
		RefreshList()
	end
end

function Pins.Toggle(entry)
	entry.pinned = not entry.pinned or nil
	Pins.Refresh()
end

local function Jump(row)
	ui.list:Hide()
	if row.entry then
		ns.Transcript.JumpTo(row.entry)
	end
end

local function NewRow(i)
	local row = CreateFrame("Button", "GnomishRelayPinnedRow" .. i, ui.list)
	row:SetSize(WIDTH - 8, ROW_HEIGHT)
	row:SetPoint("TOPLEFT", ui.list, "TOPLEFT", 4, -4 - (i - 1) * ROW_HEIGHT)
	row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	row.text = row:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	row.text:SetPoint("LEFT", row, "LEFT", 6, 0)
	row.text:SetWidth(WIDTH - 20)
	row.text:SetJustifyH("LEFT")
	row.text:SetWordWrap(false)
	row:SetScript("OnClick", Jump)
	return row
end

-- A click outside closes the list, as a menu of the game does.
local function CloseOnClickOutside()
	if not (ui.list:IsMouseOver() or ui.button:IsMouseOver()) then
		ui.list:Hide()
	end
end

local function BuildList(frame)
	ui.list = CreateFrame("Frame", "GnomishRelayPinnedList", frame)
	ui.list:SetFrameStrata("DIALOG")
	ui.list:SetPoint("TOPRIGHT", ui.button, "BOTTOMRIGHT", 0, -2)
	ui.list:SetWidth(WIDTH)
	local background = ui.list:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.95)
	ui.empty = ui.list:CreateFontString("GnomishRelayPinnedEmpty", "OVERLAY", "GameFontHighlightSmall")
	ui.empty:SetPoint("TOPLEFT", ui.list, "TOPLEFT", 10, -8)
	ui.empty:SetWidth(WIDTH - 20)
	ui.empty:SetJustifyH("LEFT")
	ui.empty:SetText("No pinned replies yet. Click Pin on a reply to keep it here.")
	ui.empty:SetTextColor(0.55, 0.53, 0.47)
	for i = 1, ROWS do
		ui.rows[i] = NewRow(i)
	end
	ui.list:EnableMouseWheel(true)
	ui.list:SetScript("OnMouseWheel", function(_, delta)
		ui.offset = ui.offset - delta
		RefreshList()
	end)
	ui.list:RegisterEvent("GLOBAL_MOUSE_DOWN")
	ui.list:SetScript("OnEvent", CloseOnClickOutside)
	frame:HookScript("OnHide", function()
		ui.list:Hide()
	end)
	ui.list:Hide()
end

local function ToggleList()
	ui.list:SetShown(not ui.list:IsShown())
	ui.offset = 0
	RefreshList()
end

-- The button sits at the right end of the header of the chat, `right` in from the edge.
function Pins.Build(frame, right, y)
	ui.button = CreateFrame("Button", "GnomishRelayPinnedButton", frame)
	ui.button:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -right, y)
	ui.button:SetHeight(18)
	ui.button.label = ui.button:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
	ui.button.label:SetPoint("RIGHT", ui.button, "RIGHT", 0, 0)
	ui.button:SetScript("OnClick", ToggleList)
	BuildList(frame)
	Pins.Refresh()
	return ui.button
end
