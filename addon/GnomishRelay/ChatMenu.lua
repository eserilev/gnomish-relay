-- The right-click menu of a chat tile (SPEC.md 13.1, "Mini chats"): the name of the
-- chat, then Pop out and Delete.

local _, ns = ...

local ChatMenu = {}
ns.ChatMenu = ChatMenu

local WIDTH = 160
local ROW = 20
local GOLD = "ffd100"
local RED = "ff6a5a"

local ui = {}

function ChatMenu.Close()
	if ui.menu then
		ui.menu:Hide()
	end
end

local function Choose(action)
	local chatId = ui.chatId
	ChatMenu.Close()
	if chatId then
		action(chatId)
	end
end

-- A popped-out chat goes back to the window with the words of its own button.
local function SwitchChat(chatId)
	if ns.MiniChat.IsOpen(chatId) then
		ns.MiniChat.Dock(chatId)
	else
		ns.MiniChat.PopOut(chatId)
	end
end

local function Row(i, name, action)
	local row = CreateFrame("Button", "GnomishRelayChatMenu" .. name, ui.menu)
	row:SetSize(WIDTH, ROW)
	row:SetPoint("TOPLEFT", ui.menu, "TOPLEFT", 0, -4 - i * ROW)
	row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	row.text = row:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	row.text:SetPoint("LEFT", row, "LEFT", 12, 0)
	row:SetScript("OnClick", function()
		Choose(action)
	end)
	return row
end

-- The menu closes at a click outside it, as a menu of the game does.
local function WatchClicks()
	local clicks = CreateFrame("Frame")
	clicks:RegisterEvent("GLOBAL_MOUSE_DOWN")
	clicks:SetScript("OnEvent", function()
		if ui.menu:IsShown() and not ui.menu:IsMouseOver() then
			ui.menu:Hide()
		end
	end)
end

local function Build()
	ui.menu = CreateFrame("Frame", "GnomishRelayChatMenu", UIParent)
	ui.menu:SetFrameStrata("DIALOG")
	ui.menu:SetSize(WIDTH, 3 * ROW + 8)
	ui.menu:EnableMouse(true)
	local background = ui.menu:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.95)
	ui.title = ui.menu:CreateFontString("GnomishRelayChatMenuTitle", "OVERLAY", "GameFontNormalSmall")
	ui.title:SetPoint("TOPLEFT", ui.menu, "TOPLEFT", 12, -8)
	ui.title:SetPoint("TOPRIGHT", ui.menu, "TOPRIGHT", -12, -8)
	ui.title:SetJustifyH("LEFT")
	ui.title:SetWordWrap(false)
	ui.popOut = Row(1, "PopOut", SwitchChat)
	ui.delete = Row(2, "Delete", ns.Window.AskDelete)
	ui.delete.text:SetText("|cff" .. RED .. "Delete|r")
	WatchClicks()
	ui.menu:Hide()
end

-- Opens the menu of `chatId` at the right of its tile.
function ChatMenu.Open(tile, chatId)
	local chat = ns.Store.Chat(chatId)
	if not chat then
		return
	end
	if not ui.menu then
		Build()
	end
	ui.chatId = chatId
	ui.title:SetText("|cff" .. GOLD .. ns.Relay.Plain(chat.name) .. "|r")
	local docked = not ns.MiniChat.IsOpen(chatId)
	ui.popOut.text:SetText(docked and "Pop out" or "Open in main window")
	ui.popOut:SetAlpha(docked and ns.MiniChat.Full() and 0.45 or 1)
	ui.menu:ClearAllPoints()
	ui.menu:SetPoint("TOPLEFT", tile, "TOPRIGHT", -20, -8)
	ui.menu:Show()
end
