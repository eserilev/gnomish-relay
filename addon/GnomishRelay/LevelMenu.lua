-- The permission mode of the chat in the header, as in Claude Code: the mode shows all
-- the time, a click opens the list, and Shift+Tab in the message box moves to the next
-- mode (SPEC.md 13.1). The bridge decides what the chat gets (SPEC.md 9.3).

local _, ns = ...

local LevelMenu = {}
ns.LevelMenu = LevelMenu

-- Full-auto asks nothing, so it stands out wherever it shows.
local WARNING = "ff5a1f"
local ROW = 20
local WIDTH = 110

local ui = {}

-- The level as the header shows it.
function LevelMenu.Text(level)
	if level == "full-auto" then
		return "|cff" .. WARNING .. level .. "|r"
	end
	return level
end

local function Pick(mode)
	ui.list:Hide()
	local chat = ui.selected()
	if not chat then
		return
	end
	ns.Store.SetMode(chat, mode)
	ui.refresh()
end

function LevelMenu.Next()
	local chat = ui.selected()
	if chat then
		Pick(ns.Store.NextMode(chat.mode))
	end
end

local function BuildList(parent, button)
	local list = CreateFrame("Frame", "GnomishRelayLevelList", parent)
	list:SetFrameStrata("DIALOG")
	list:SetPoint("TOPLEFT", button, "BOTTOMLEFT", 0, -2)
	list:SetSize(WIDTH, #ns.Store.Modes() * ROW)
	local background = list:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.95)
	for i, mode in ipairs(ns.Store.Modes()) do
		local choice = CreateFrame("Button", "GnomishRelayLevelChoice" .. i, list)
		choice:SetSize(WIDTH, ROW)
		choice:SetPoint("TOPLEFT", list, "TOPLEFT", 0, -(i - 1) * ROW)
		choice:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
		choice.text = choice:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
		choice.text:SetPoint("LEFT", choice, "LEFT", 8, 0)
		choice.text:SetText(LevelMenu.Text(mode))
		choice:SetScript("OnClick", function()
			Pick(mode)
		end)
	end
	list:Hide()
	return list
end

-- An open list closes at a click outside it, as a menu of the game does.
local function WatchClicks(button)
	button:HookScript("OnHide", function()
		ui.list:Hide()
	end)
	local clicks = CreateFrame("Frame")
	clicks:RegisterEvent("GLOBAL_MOUSE_DOWN")
	clicks:SetScript("OnEvent", function()
		if not (ui.list:IsMouseOver() or button:IsMouseOver()) then
			ui.list:Hide()
		end
	end)
end

-- A button over `label`. `selected` gives the chat, and `refresh` draws the window again.
function LevelMenu.Build(parent, label, selected, refresh)
	ui.selected, ui.refresh = selected, refresh
	local button = CreateFrame("Button", "GnomishRelayLevelButton", parent)
	button:SetAllPoints(label)
	ui.list = BuildList(parent, button)
	button:SetScript("OnClick", function()
		ui.list:SetShown(not ui.list:IsShown())
	end)
	WatchClicks(button)
	return button
end

-- Shift+Tab moves to the next mode, as in Claude Code.
function LevelMenu.Bind(input)
	input:SetScript("OnTabPressed", function()
		if IsShiftKeyDown() then
			LevelMenu.Next()
		end
	end)
end
