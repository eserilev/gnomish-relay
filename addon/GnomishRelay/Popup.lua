-- The honest permission popup (SPEC.md 6.4). The text comes from the bridge, written
-- by `popup_text` (S15): the raw command first, and the words of the agent below it.

local _, ns = ...

local Popup = {}
ns.Popup = Popup

local WIDTH = 460
-- Room above the text for the chat name, and below it for the buttons.
local TEXT_TOP = 32
local BUTTONS_ROOM = 56
local MAX_HEIGHT = 600
local BUTTON_WIDTH = 104
-- A popup that comes up under the mouse takes no click at first: the click was aimed
-- at the game.
local ARM_AFTER = 1
local REJECTS = { reject_once = true, reject_always = true }
-- Each button names the kind of its option, never the label of the agent: an agent
-- can call "allow" "Reject".
local LABELS = {
	allow_once = "Allow once",
	allow_always = "Always allow",
	reject_once = "Reject",
	reject_always = "Always reject",
}

local frame
local ui = { buttons = {} }
local shown -- the request on screen
local armedAt = 0

-- The rule line comes from the bridge, never from the agent (SPEC.md 6.6.5).
local function AlwaysOption(request)
	for _, option in ipairs(request.options) do
		if option.kind == "allow_always" then
			return option
		end
	end
end

local function Button(index)
	local button = ui.buttons[index]
	if button then
		return button
	end
	button = CreateFrame("Button", "GnomishRelayPopupButton" .. index, frame, "UIPanelButtonTemplate")
	button:SetSize(BUTTON_WIDTH, 22)
	button:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", 12 + (index - 1) * (BUTTON_WIDTH + 6), 12)
	button:SetScript("OnClick", function(self)
		if GetTime() < armedAt then
			return
		end
		if shown and self.option then
			local always = AlwaysOption(shown)
			if always and always.id == self.option then
				ns.Relay.RuleAdded(shown.chat, always.label)
			end
			ns.Transport.Answer(shown, self.option)
		end
	end)
	ui.buttons[index] = button
	return button
end

local function Build()
	frame = CreateFrame("Frame", "GnomishRelayPopup", UIParent)
	frame:SetFrameStrata("DIALOG")
	frame:SetSize(WIDTH, 160)
	frame:SetPoint("TOP", UIParent, "TOP", 0, -120)
	frame:EnableMouse(true)
	local border = CreateFrame("Frame", nil, frame, "DialogBorderDarkTemplate")
	border:SetAllPoints()
	ui.chat = frame:CreateFontString(nil, "OVERLAY", "GameFontNormal")
	ui.chat:SetPoint("TOPLEFT", frame, "TOPLEFT", 12, -10)
	ui.count = frame:CreateFontString("GnomishRelayPopupCount", "OVERLAY", "GameFontDisableSmall")
	ui.count:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -12, -12)
	ui.text = frame:CreateFontString("GnomishRelayPopupText", "OVERLAY", "GameFontHighlight")
	ui.text:SetPoint("TOPLEFT", frame, "TOPLEFT", 12, -TEXT_TOP)
	ui.text:SetWidth(WIDTH - 24)
	ui.text:SetJustifyH("LEFT")
	ui.text:SetWordWrap(true)
	-- A command reads best in the mono font. WoW finds a new font file only at launch.
	if not ui.text:SetFont(ns.Transcript.MONO, 12, "") then
		ui.text:SetFont(ns.Transcript.MONO_FALLBACK, 13, "")
	end
	ui.rule = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	ui.rule:SetPoint("TOPLEFT", ui.text, "BOTTOMLEFT", 0, -8)
	ui.rule:SetWidth(WIDTH - 24)
	ui.rule:SetJustifyH("LEFT")
	frame:Hide()
end

-- The popup grows with its text, so a long command never runs over the buttons.
local function Height(withRule)
	local height = TEXT_TOP + ui.text:GetStringHeight() + BUTTONS_ROOM
	if withRule then
		height = height + 8 + ui.rule:GetStringHeight()
	end
	return math.min(height, MAX_HEIGHT)
end

-- Reject stays at the left and the allow buttons at the right, with a wide gap
-- between, so a click in a hurry does not allow by mistake.
local function PlaceButtons(request)
	local left, right = 0, 0
	for i = #request.options, 1, -1 do
		local button = Button(i)
		button:ClearAllPoints()
		if not REJECTS[request.options[i].kind] then
			button:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -12 - right * (BUTTON_WIDTH + 6), 12)
			right = right + 1
		end
	end
	for i, option in ipairs(request.options) do
		if REJECTS[option.kind] then
			Button(i):SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", 12 + left * (BUTTON_WIDTH + 6), 12)
			left = left + 1
		end
	end
end

-- The sound calls the player to a request that waits. The timer turns the buttons on.
local function Arrive()
	armedAt = GetTime() + ARM_AFTER
	PlaySound(SOUNDKIT.READY_CHECK)
	C_Timer.After(ARM_AFTER, Popup.Refresh)
end

local function Show(request)
	if not frame then
		Build()
	end
	if not shown or shown.request ~= request.request then
		Arrive()
	end
	shown = request
	local waiting = ns.Transport.RequestCount()
	ui.count:SetText(string.format("1 of %d", waiting))
	ui.count:SetShown(waiting > 1)
	local chat = ns.Store.Chat(request.chat)
	ui.chat:SetText(ns.Relay.Plain(chat and chat.name or request.chat))
	ui.text:SetText(ns.Relay.Plain(request.text))
	local always = AlwaysOption(request)
	ui.rule:SetText(always and "Always allow: " .. ns.Relay.Plain(always.label) or "")
	ui.rule:SetShown(always ~= nil)
	for i, button in ipairs(ui.buttons) do
		button.option = nil
		button:SetShown(i <= #request.options)
	end
	for i, option in ipairs(request.options) do
		local button = Button(i)
		button.option = option.id
		button:SetText(LABELS[option.kind])
		button:SetEnabled(GetTime() >= armedAt)
		button:Show()
	end
	PlaceButtons(request)
	frame:SetHeight(Height(always ~= nil))
	frame:Show()
end

-- Shows the oldest open request, or hides the popup when none is open.
function Popup.Refresh()
	local request = ns.Transport.Request()
	if request then
		Show(request)
	elseif frame then
		shown = nil
		frame:Hide()
	end
end
