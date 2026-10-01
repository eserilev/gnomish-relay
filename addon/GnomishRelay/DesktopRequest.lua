-- The desktop request of the selected chat, between the transcript and the input
-- (SPEC.md 6.6.3, "The request in the chat"). No addon can answer it, so the box only
-- says where to answer.

local _, ns = ...

local DesktopRequest = {}
ns.DesktopRequest = DesktopRequest

local WAITING_HEIGHT = 74
local ENDED_HEIGHT = 24
local GAP = 4
local PAD = 8
local COMMAND_WIDTH = 280
local ORANGE = "ff9f40"
local WAITING = "Waiting for your approval on your desktop"
local ENDED = {
	approved = "Approved on your desktop",
	denied = "Denied on your desktop",
	none = "No answer on your desktop",
}

local ui = { height = 0 }

-- The line cuts a long command, so the tooltip shows all of it.
local function ShowAsksTooltip(owner)
	if owner.text == "" then
		return
	end
	GameTooltip:SetOwner(owner, "ANCHOR_TOP")
	GameTooltip:SetText(owner.text, 1, 1, 1, 1, true)
	GameTooltip:Show()
end

local function Copy()
	ui.command:SetFocus()
	ui.command:HighlightText()
	ui.hint:Show()
end

-- A raise has no asks line: its notice names the permission.
local function AsksText(chat, notice)
	if notice.asks then
		return ns.Relay.Plain(notice.asks)
	elseif notice.raise then
		return string.format("Let %s work at %s.", ns.Relay.AgentName(chat.agent), notice.raise)
	end
	return ""
end

local function ShowWaiting(chat, notice)
	ui.title:SetText("|cff" .. ORANGE .. WAITING .. "|r")
	ui.asks.text = AsksText(chat, notice)
	ui.asks.label:SetText(ui.asks.text)
	ui.command.line = "gnomish-relay approve " .. notice.id
	if ui.command:GetText() ~= ui.command.line then
		ui.command:SetText(ui.command.line)
		ui.command:SetCursorPosition(0)
		ui.hint:Hide()
	end
	for _, part in ipairs(ui.waiting) do
		part:Show()
	end
	ui.height = WAITING_HEIGHT
end

local function ShowEnded(notice)
	ui.title:SetText(ENDED[notice.state])
	for _, part in ipairs(ui.waiting) do
		part:Hide()
	end
	ui.hint:Hide()
	ui.height = ENDED_HEIGHT
end

-- `chat` is nil when the chat does not show.
function DesktopRequest.Refresh(chat)
	local working = chat and ns.Transport.Working(chat.id)
	local notice = working and working.desktop
	ui.frame:SetShown(notice ~= nil)
	if not notice then
		ui.height = 0
	elseif notice.state == "wait" then
		ShowWaiting(chat, notice)
	else
		ShowEnded(notice)
	end
	ui.frame:SetHeight(math.max(ui.height, 1))
end

-- Puts the box right above `bottom`, and returns the room that it takes there.
function DesktopRequest.Place(left, bottom)
	ui.frame:ClearAllPoints()
	ui.frame:SetPoint("BOTTOMLEFT", ui.parent, "BOTTOMLEFT", left, bottom)
	ui.frame:SetPoint("BOTTOMRIGHT", ui.parent, "BOTTOMRIGHT", -left, bottom)
	if ui.height == 0 then
		return 0
	end
	return ui.height + GAP
end

-- The box keeps its line: typing puts the text back, and a click selects all of it.
local function BuildCommand()
	local box = CreateFrame("EditBox", "GnomishRelayDesktopCommand", ui.frame, "InputBoxTemplate")
	box:SetPoint("BOTTOMLEFT", ui.frame, "BOTTOMLEFT", PAD + 6, 6)
	box:SetSize(COMMAND_WIDTH, 22)
	box:SetFontObject(ChatFontNormal)
	box:SetAutoFocus(false)
	box:SetScript("OnTextChanged", function(self, typed)
		if typed then
			self:SetText(self.line)
			self:HighlightText()
		end
	end)
	box:SetScript("OnEditFocusGained", function(self)
		self:HighlightText()
	end)
	box:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
	end)
	ui.command = box
end

local function BuildAsks()
	ui.asks = CreateFrame("Button", "GnomishRelayDesktopAsks", ui.frame)
	ui.asks:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", PAD, -24)
	ui.asks:SetPoint("TOPRIGHT", ui.frame, "TOPRIGHT", -PAD, -24)
	ui.asks:SetHeight(16)
	ui.asks.text = ""
	ui.asks.label = ui.asks:CreateFontString("GnomishRelayDesktopAsksText", "OVERLAY", "GameFontHighlight")
	ui.asks.label:SetPoint("LEFT", ui.asks, "LEFT", 0, 0)
	ui.asks.label:SetPoint("RIGHT", ui.asks, "RIGHT", 0, 0)
	ui.asks.label:SetJustifyH("LEFT")
	ui.asks.label:SetWordWrap(false)
	ui.asks:SetScript("OnEnter", ShowAsksTooltip)
	ui.asks:SetScript("OnLeave", function()
		GameTooltip:Hide()
	end)
end

local function BuildCopy()
	local copy = CreateFrame("Button", "GnomishRelayDesktopCopy", ui.frame, "UIPanelButtonTemplate")
	copy:SetSize(70, 22)
	copy:SetPoint("LEFT", ui.command, "RIGHT", 8, 0)
	copy:SetText("Copy")
	copy:SetScript("OnClick", Copy)
	ui.hint = ui.frame:CreateFontString("GnomishRelayDesktopHint", "OVERLAY", "GameFontDisableSmall")
	ui.hint:SetPoint("LEFT", copy, "RIGHT", 8, 0)
	ui.hint:SetText("Press Ctrl+C to copy (Cmd+C on a Mac).")
	ui.hint:Hide()
	ui.copy = copy
end

-- `parent` is the window. Window.lua places the box with Place.
function DesktopRequest.Build(parent)
	ui.parent = parent
	ui.frame = CreateFrame("Frame", "GnomishRelayDesktop", parent)
	local background = ui.frame:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.1, 0.15, 0.2, 0.9)
	ui.title = ui.frame:CreateFontString("GnomishRelayDesktopTitle", "OVERLAY", "GameFontNormal")
	ui.title:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", PAD, -6)
	ui.title:SetJustifyH("LEFT")
	BuildAsks()
	BuildCommand()
	BuildCopy()
	ui.waiting = { ui.asks, ui.command, ui.copy }
	ui.frame:Hide()
end
