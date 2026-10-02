-- The permission mode of the chat in the header, as in Claude Code: the mode shows all
-- the time, a click opens the list, and Shift+Tab in the message box moves to the next
-- mode (SPEC.md 13.1). The bridge decides what the chat gets (SPEC.md 9.3). The window
-- and each mini chat have their own list.

local _, ns = ...

local LevelMenu = {}
ns.LevelMenu = LevelMenu

-- Full-auto asks nothing, so it stands out wherever it shows.
local WARNING = "ff5a1f"
LevelMenu.WARNING = WARNING
local ROW = 20
local WIDTH = 110

-- The level as the header shows it.
function LevelMenu.Text(level)
	if level == "full-auto" then
		return "|cff" .. WARNING .. level .. "|r"
	end
	return level
end

-- `menu.selected` gives the chat, and `menu.refresh` draws its window again.
local function Pick(menu, mode)
	menu.list:Hide()
	local chat = menu.selected()
	if not chat then
		return
	end
	ns.Store.SetMode(chat, mode)
	menu.refresh()
end

local function BuildList(menu, parent, button, prefix)
	local list = CreateFrame("Frame", prefix .. "LevelList", parent)
	list:SetFrameStrata("DIALOG")
	list:SetPoint("TOPLEFT", button, "BOTTOMLEFT", 0, -2)
	list:SetSize(WIDTH, #ns.Store.Modes() * ROW)
	local background = list:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.95)
	for i, mode in ipairs(ns.Store.Modes()) do
		local choice = CreateFrame("Button", prefix .. "LevelChoice" .. i, list)
		choice:SetSize(WIDTH, ROW)
		choice:SetPoint("TOPLEFT", list, "TOPLEFT", 0, -(i - 1) * ROW)
		choice:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
		choice.text = choice:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
		choice.text:SetPoint("LEFT", choice, "LEFT", 8, 0)
		choice.text:SetText(LevelMenu.Text(mode))
		choice:SetScript("OnClick", function()
			Pick(menu, mode)
		end)
	end
	list:Hide()
	return list
end

-- An open list closes at a click outside it, as a menu of the game does.
local function WatchClicks(menu, button)
	button:HookScript("OnHide", function()
		menu.list:Hide()
	end)
	local clicks = CreateFrame("Frame")
	clicks:RegisterEvent("GLOBAL_MOUSE_DOWN")
	clicks:SetScript("OnEvent", function()
		if not (menu.list:IsMouseOver() or button:IsMouseOver()) then
			menu.list:Hide()
		end
	end)
end

-- A button over `label`, with frame names that start with `prefix`. `selected` gives
-- the chat, and `refresh` draws the window again.
function LevelMenu.Build(parent, label, selected, refresh, prefix)
	local menu = { selected = selected, refresh = refresh }
	local button = CreateFrame("Button", prefix .. "LevelButton", parent)
	button:SetAllPoints(label)
	menu.list = BuildList(menu, parent, button, prefix)
	button:SetScript("OnClick", function()
		menu.list:SetShown(not menu.list:IsShown())
	end)
	WatchClicks(menu, button)
	return button
end

-- Shift+Tab moves the chat that `selected` gives to the next mode, as in Claude Code.
function LevelMenu.Bind(input, selected, refresh)
	input:SetScript("OnTabPressed", function()
		local chat = selected()
		if chat and IsShiftKeyDown() then
			ns.Store.SetMode(chat, ns.Store.NextMode(chat.mode))
			refresh()
		end
	end)
end
