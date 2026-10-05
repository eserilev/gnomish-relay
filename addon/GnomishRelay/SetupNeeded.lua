-- The first-run window of an addon with no key: an install from an addon site with no
-- desktop app yet, a new key addon that WoW finds only after a restart (SPEC.md 7.3.2),
-- or a key addon that an older desktop app wrote for another WoW version (7.9). It looks
-- like the setup window of Timeways.

local _, ns = ...

local SetupNeeded = {}
ns.SetupNeeded = SetupNeeded

local WIDTH, HEIGHT = 540, 360
local SHEET_INSET = 18
local TEXT_WIDTH = WIDTH - 2 * SHEET_INSET - 32
-- No `|`: WoW reads it as an escape, and a doubled `||` can reach the clipboard. These
-- forms run the same script as the README lines, with no pipe.
local INSTALL_WINDOWS = "iex (irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1)"
local INSTALL_UNIX =
	'sh -c "$(curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh)"'

-- The chat line, the heading, and the text of the window for each missing step.
local STEPS = {
	install = {
		line = "Gnomish Relay needs its desktop app. Get it at github.com/eserilev/gnomish-relay, then restart WoW.",
		heading = "Gnomish Relay needs its desktop app",
		body = "The desktop app runs your coding agents. Paste this line in a terminal to install it.",
	},
	restart = {
		line = "Gnomish Relay: restart WoW to finish setup. If this shows again, run gnomish-relay setup on your desktop.",
		heading = "Restart WoW to finish setup",
		body = "If this shows again after the restart, run gnomish-relay setup on your desktop.",
	},
	update = {
		line = "Gnomish Relay: the desktop app is out of date. On your desktop, run gnomish-relay update, then restart WoW.",
		heading = "Update the desktop app",
		body = "The desktop app is older than this version of WoW. On your desktop, run gnomish-relay update, then restart WoW.",
	},
}
SetupNeeded.NO_APP = STEPS.install.line
SetupNeeded.RESTART = STEPS.restart.line

local frame
local ui = {}

-- A key in an earlier UI session means the desktop app is there.
function SetupNeeded.HadKey()
	local db = ns.Store.db
	return db ~= nil and db.hadKey == true
end

-- The key addon is wrong before a restart can help: an old desktop app wrote it.
local function Step()
	if ns.KeyHandoff.OutOfDate() then
		return STEPS.update
	end
	if SetupNeeded.HadKey() then
		return STEPS.restart
	end
	return STEPS.install
end

function SetupNeeded.Line()
	return Step().line
end

-- Linux players run the Windows client under Wine, so a Windows client gets both lines.
local function Commands()
	if IsMacClient() then
		return { { "In Terminal:", INSTALL_UNIX } }
	end
	return {
		{ "Windows, in PowerShell:", INSTALL_WINDOWS },
		{ "Linux, in a terminal:", INSTALL_UNIX },
	}
end

local function Text(font, anchor, x, y)
	local text = ui.sheet:CreateFontString(nil, "OVERLAY", font)
	text:SetPoint("TOPLEFT", anchor, "BOTTOMLEFT", x, y)
	text:SetWidth(TEXT_WIDTH)
	text:SetJustifyH("LEFT")
	text:SetWordWrap(true)
	return text
end

-- The box keeps its line: typing puts the text back, and a click selects all of it for Ctrl+C.
local function CommandBox(name, command, anchor)
	local box = CreateFrame("EditBox", name, ui.sheet, "InputBoxTemplate")
	box:SetPoint("TOPLEFT", anchor, "BOTTOMLEFT", 6, -4)
	box:SetSize(TEXT_WIDTH - 6, 22)
	box:SetFontObject(ChatFontNormal)
	box:SetAutoFocus(false)
	box:SetText(command)
	box:SetCursorPosition(0)
	box:SetScript("OnTextChanged", function(self, typed)
		if typed then
			self:SetText(command)
			self:HighlightText()
		end
	end)
	box:SetScript("OnEditFocusGained", function(self)
		self:HighlightText()
	end)
	box:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
	end)
	return box
end

-- The labels, the boxes, and the hints show only for a missing desktop app. A box sits
-- 6 pixels right of its label, so the text below a box moves 6 pixels back.
local function BuildCommands(anchor)
	ui.commands = {}
	local x = 0
	for i, command in ipairs(Commands()) do
		local label = Text("QuestFont", anchor, x, -12)
		label:SetText(command[1])
		anchor = CommandBox("GnomishRelaySetupCommand" .. i, command[2], label)
		x = -6
		table.insert(ui.commands, label)
		table.insert(ui.commands, anchor)
	end
	local copy = Text("QuestFontNormalSmall", anchor, x, -8)
	copy:SetText("Click a line and press Ctrl+C to copy it (Cmd+C on a Mac).")
	copy:SetAlpha(0.7)
	local run = Text("QuestFont", copy, 0, -10)
	run:SetText("Run it on your computer. Then restart WoW.")
	table.insert(ui.commands, copy)
	table.insert(ui.commands, run)
end

local function BuildSheet()
	ui.sheet = CreateFrame("Frame", nil, frame)
	ui.sheet:SetPoint("TOPLEFT", frame, "TOPLEFT", SHEET_INSET, -40)
	ui.sheet:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -SHEET_INSET, 48)
	local parchment = ui.sheet:CreateTexture(nil, "BACKGROUND")
	parchment:SetAllPoints()
	ns.Atlases.SetParchment(parchment)
	ui.heading = ui.sheet:CreateFontString("GnomishRelaySetupHeading", "OVERLAY", "QuestTitleFont")
	ui.heading:SetPoint("TOPLEFT", ui.sheet, "TOPLEFT", 16, -16)
	ui.heading:SetWidth(TEXT_WIDTH)
	ui.heading:SetJustifyH("LEFT")
	ui.body = Text("QuestFont", ui.heading, 0, -10)
	BuildCommands(ui.body)
end

local function Build()
	frame = CreateFrame("Frame", "GnomishRelaySetupNeeded", UIParent, "BackdropTemplate")
	frame:SetFrameStrata("DIALOG")
	frame:SetSize(WIDTH, HEIGHT)
	frame:SetPoint("CENTER")
	frame:EnableMouse(true)
	frame:SetBackdrop({
		bgFile = "Interface\\FrameGeneral\\UI-Background-Rock",
		edgeFile = "Interface\\DialogFrame\\UI-DialogBox-Border",
		tile = true,
		tileSize = 256,
		edgeSize = 32,
		insets = { left = 11, right = 12, top = 12, bottom = 11 },
	})
	table.insert(UISpecialFrames, "GnomishRelaySetupNeeded")

	local title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormal")
	title:SetPoint("TOP", frame, "TOP", 0, -14)
	title:SetText("Gnomish Relay Setup")
	local x = CreateFrame("Button", nil, frame, "UIPanelCloseButton")
	x:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -4, -4)
	BuildSheet()

	local close = CreateFrame("Button", "GnomishRelaySetupClose", frame, "UIPanelButtonTemplate")
	close:SetSize(90, 22)
	close:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -18, 16)
	close:SetText("Close")
	close:SetScript("OnClick", function()
		frame:Hide()
	end)
	frame:Hide()
end

local function Refresh()
	local step = Step()
	ui.heading:SetText(step.heading)
	ui.body:SetText(step.body)
	for _, part in ipairs(ui.commands) do
		part:SetShown(step == STEPS.install)
	end
end

function SetupNeeded.Show()
	if not frame then
		Build()
	end
	Refresh()
	frame:Show()
end

function SetupNeeded.Toggle()
	if frame and frame:IsShown() then
		frame:Hide()
	else
		SetupNeeded.Show()
	end
end
