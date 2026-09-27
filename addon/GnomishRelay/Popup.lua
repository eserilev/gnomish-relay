-- The honest permission popup (SPEC.md 6.4). The text comes from the bridge, written
-- by `popup_text` (S15): the raw command first, and the words of the agent below it.

local _, ns = ...

local Popup = {}
ns.Popup = Popup

local WIDTH = 460
local BUTTON_WIDTH = 104
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
	local background = frame:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.9)
	ui.chat = frame:CreateFontString(nil, "OVERLAY", "GameFontNormal")
	ui.chat:SetPoint("TOPLEFT", frame, "TOPLEFT", 12, -10)
	ui.text = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	ui.text:SetPoint("TOPLEFT", frame, "TOPLEFT", 12, -32)
	ui.text:SetWidth(WIDTH - 24)
	ui.text:SetJustifyH("LEFT")
	ui.text:SetWordWrap(true)
	ui.rule = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	ui.rule:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", 12, 42)
	ui.rule:SetWidth(WIDTH - 24)
	ui.rule:SetJustifyH("LEFT")
	frame:Hide()
end

local function Show(request)
	if not frame then
		Build()
	end
	shown = request
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
		button:Show()
	end
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
