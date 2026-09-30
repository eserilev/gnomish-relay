-- The Settings tab of the window (SPEC.md 13.1): the settings of the addon, which apply
-- at once, and the level ceiling of the bridge. The game never writes the config.

local _, ns = ...

local SettingsTab = {}
ns.SettingsTab = SettingsTab

local LABEL_WIDTH = 150
local ROW = 30
local GREY = "8d8778"
local ORANGE = "ff9f40"
local GREEN = "1eff00"
local LEVELS = { "ask", "auto-edit" }
local COLORS = { "f0a860", "ff80ff", "69ccf0", "9fe39f", "ffd100" }
local FONT_MIN, FONT_MAX = 12, 20
-- A list older than this shows its age in orange.
local STALE_AFTER = 600
local APPEARANCE_TOP = -40 - 2 * ROW - 14
local NOTIFY_TOP = APPEARANCE_TOP - 28 - 4 * ROW - 10
-- The Notifications group takes a heading and two rows. The rules group then shows
-- fewer rows, so the page still fits the least window.
local NOTIFY_HEIGHT = 28 + 2 * ROW + 6
local RULE_ROWS, RULE_ROWS_WITH_NOTIFY = 6, 3

local ui = { dropdowns = {} }

local function Label(parent, font, x, y)
	local text = parent:CreateFontString(nil, "OVERLAY", font)
	text:SetPoint("TOPLEFT", parent, "TOPLEFT", x, y)
	text:SetJustifyH("LEFT")
	return text
end

local function Heading(text, y)
	Label(ui.page, "GameFontNormalLarge", 16, y):SetText(text)
end

local function RowLabel(text, y, parent)
	Label(parent or ui.page, "GameFontHighlight", 20, y - 6):SetText(text)
end

-- A button that shows the value, and a list of choices below it while open.
local function Dropdown(name, y, width, onPick, x, parent)
	parent = parent or ui.page
	local button = CreateFrame("Button", name, parent, "UIPanelButtonTemplate")
	button:SetSize(width, 22)
	button:SetPoint("TOPLEFT", parent, "TOPLEFT", x or 20 + LABEL_WIDTH, y)
	local list = CreateFrame("Frame", name .. "List", ui.page)
	list:SetFrameStrata("DIALOG")
	list:SetPoint("TOPLEFT", button, "BOTTOMLEFT", 0, 0)
	list:SetWidth(width)
	local background = list:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.95)
	list:Hide()
	button.list, button.choices, button.baseName = list, {}, name
	button:SetScript("OnClick", function()
		list:SetShown(not list:IsShown())
	end)
	button.Pick = function(value)
		list:Hide()
		onPick(value)
	end
	table.insert(ui.dropdowns, button)
	return button
end

-- An open list closes with its page and at a click outside it, as a menu of the game does.
local function CloseLists(keepUnderMouse)
	for _, dropdown in ipairs(ui.dropdowns) do
		local underMouse = dropdown.list:IsMouseOver() or dropdown:IsMouseOver()
		if not (keepUnderMouse and underMouse) then
			dropdown.list:Hide()
		end
	end
end

local function WatchClicks()
	ui.page:HookScript("OnHide", function()
		CloseLists(false)
	end)
	local clicks = CreateFrame("Frame")
	clicks:RegisterEvent("GLOBAL_MOUSE_DOWN")
	clicks:SetScript("OnEvent", function()
		CloseLists(true)
	end)
end

local function Choice(dropdown, i)
	local choice = dropdown.choices[i]
	if choice then
		return choice
	end
	choice = CreateFrame("Button", dropdown.baseName .. "Choice" .. i, dropdown.list)
	choice:SetSize(dropdown.list:GetWidth(), 20)
	choice:SetPoint("TOPLEFT", dropdown.list, "TOPLEFT", 0, -(i - 1) * 20)
	choice:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	choice.text = Label(choice, "GameFontHighlight", 8, -3)
	choice:SetScript("OnClick", function(self)
		dropdown.Pick(self.value)
	end)
	dropdown.choices[i] = choice
	return choice
end

-- `options` holds `{ value, text }` pairs.
local function SetChoices(dropdown, options)
	for i, option in ipairs(options) do
		local choice = Choice(dropdown, i)
		choice.value = option.value
		choice.text:SetText(option.text)
		choice:Show()
	end
	for i = #options + 1, #dropdown.choices do
		dropdown.choices[i]:Hide()
	end
	dropdown.list:SetHeight(math.max(1, #options) * 20)
end

local function AgentOptions()
	local last = ns.BridgeSettings.Last()
	local options = {}
	for _, agent in ipairs(last and last.agents or {}) do
		table.insert(options, { value = agent.name, text = ns.Relay.AgentName(agent.name) })
	end
	return options
end

local function NewAgent()
	return ns.BridgeSettings.NewChatAgent("claude")
end

local function Ceiling(agent)
	local known = ns.BridgeSettings.FindAgent(agent)
	return known and known.level or "?"
end

local function Ago(seconds)
	local minutes = math.floor(seconds / 60)
	if minutes < 60 then
		return minutes .. "m ago"
	elseif minutes < 48 * 60 then
		return math.floor(minutes / 60) .. "h ago"
	end
	return math.floor(minutes / 1440) .. "d ago"
end

-- "Online · 2m ago", orange when the list is old, grey while the bridge is offline.
function SettingsTab.Status()
	local age = ns.BridgeSettings.Age()
	if not age then
		return "|cff" .. GREY .. "Not loaded yet|r"
	elseif not ns.Transport.Online() then
		return string.format("|cff%sOffline · %s|r", GREY, Ago(age))
	end
	local color = age >= STALE_AFTER and ORANGE or GREEN
	return string.format("|cff%sOnline · %s|r", color, Ago(age))
end

local function PreviewLine()
	local db = ns.Store.db
	local agent = NewAgent()
	local line = string.format("[%s] whispers: [Chat 1] Done.", ns.Relay.AgentName(agent))
	if not db.whisperOn then
		return "|cff" .. GREY .. line .. "|r"
	end
	return "|cff" .. db.whisperColor .. line .. "|r"
end

local function RefreshSwatches()
	for _, swatch in ipairs(ui.swatches) do
		local picked = swatch.color == ns.Store.db.whisperColor
		swatch.border:SetShown(picked)
		swatch:SetAlpha(ns.Store.db.whisperOn and 1 or 0.35)
	end
end

local function FinishedText()
	for _, choice in ipairs(ns.Notices.FINISHED) do
		if choice.value == ns.Store.db.notifyFinished then
			return choice.text
		end
	end
	return "?"
end

-- Off stops the lines, the sounds, the toasts, the bell, and the faster polls, and
-- greys the other rows (SPEC.md 13.1).
local function RefreshNotify()
	local db = ns.Store.db
	local shown = ns.BridgeSettings.HooksOn()
	ui.notify:SetShown(shown)
	ns.RulesGroup.Place(
		shown and NOTIFY_TOP - NOTIFY_HEIGHT or NOTIFY_TOP,
		shown and RULE_ROWS_WITH_NOTIFY or RULE_ROWS
	)
	ui.notifyOn:SetChecked(db.notifyOn)
	ui.finished:SetText(FinishedText())
	ui.notifyChat:SetChecked(db.notifyChat)
	ui.notifySound:SetChecked(db.notifySound)
	ui.notifyToast:SetChecked(db.notifyToast)
	for _, part in ipairs(ui.notifyParts) do
		part:SetAlpha(db.notifyOn and 1 or 0.35)
	end
end

function SettingsTab.Refresh()
	if not ui.page or not ui.page:IsShown() then
		return
	end
	local db = ns.Store.db
	local agent = NewAgent()
	SetChoices(ui.agent, AgentOptions())
	ui.agent:SetText(ns.Relay.AgentName(agent))
	ui.level:SetText(ns.Store.NewLevel())
	ui.ceiling:SetText(string.format("|cff%sUp to %s (set on your desktop)|r", GREY, Ceiling(agent)))
	ui.font:SetValue(db.fontSize)
	ui.fontValue:SetText(db.fontSize)
	ui.whisperOn:SetChecked(db.whisperOn)
	ui.sound:SetChecked(db.whisperSound)
	RefreshSwatches()
	ui.preview:SetText(PreviewLine())
	ui.status.text:SetText(SettingsTab.Status())
	RefreshNotify()
end

local function BuildNewChats()
	Heading("New chats", -12)
	RowLabel("Agent", -40)
	ui.agent = Dropdown("GnomishRelaySettingsAgent", -40, 140, function(value)
		ns.Store.db.newAgent = value
		SettingsTab.Refresh()
	end)
	RowLabel("Permissions", -40 - ROW)
	ui.level = Dropdown("GnomishRelaySettingsLevel", -40 - ROW, 140, function(value)
		ns.Store.db.newLevel = value
		SettingsTab.Refresh()
	end)
	local levels = {}
	for _, level in ipairs(LEVELS) do
		table.insert(levels, { value = level, text = level })
	end
	SetChoices(ui.level, levels)
	ui.ceiling = Label(ui.page, "GameFontHighlightSmall", 20 + LABEL_WIDTH + 150, -44 - ROW)
end

local function BuildFontSize(y)
	RowLabel("Font size", y)
	ui.font = CreateFrame("Slider", "GnomishRelaySettingsFont", ui.page, "UISliderTemplate")
	ui.font:SetPoint("TOPLEFT", ui.page, "TOPLEFT", 20 + LABEL_WIDTH, y - 4)
	ui.font:SetSize(200, 16)
	ui.font:SetMinMaxValues(FONT_MIN, FONT_MAX)
	ui.font:SetValueStep(1)
	ui.font:SetObeyStepOnDrag(true)
	ui.font:SetScript("OnValueChanged", function(_, value)
		if math.floor(value + 0.5) ~= ns.Store.db.fontSize then
			ns.Window.SetFontSize(value)
		end
	end)
	ui.fontValue = Label(ui.page, "GameFontHighlight", 20 + LABEL_WIDTH + 212, y - 6)
end

local function Checkbox(name, x, y, onClick, parent)
	parent = parent or ui.page
	local box = CreateFrame("CheckButton", name, parent, "UICheckButtonTemplate")
	box:SetSize(24, 24)
	box:SetPoint("TOPLEFT", parent, "TOPLEFT", x, y)
	box:SetScript("OnClick", function(self)
		onClick(self:GetChecked() and true or false)
		SettingsTab.Refresh()
	end)
	return box
end

local function Swatch(i, color, y)
	local swatch = CreateFrame("Button", "GnomishRelaySettingsColor" .. i, ui.page)
	swatch:SetSize(20, 20)
	swatch:SetPoint("TOPLEFT", ui.page, "TOPLEFT", 20 + LABEL_WIDTH + 34 + (i - 1) * 26, y - 2)
	swatch.border = swatch:CreateTexture(nil, "BACKGROUND")
	swatch.border:SetPoint("TOPLEFT", swatch, "TOPLEFT", -2, 2)
	swatch.border:SetPoint("BOTTOMRIGHT", swatch, "BOTTOMRIGHT", 2, -2)
	swatch.border:SetColorTexture(1, 1, 1, 1)
	local fill = swatch:CreateTexture(nil, "ARTWORK")
	fill:SetAllPoints()
	local r, g, b = tonumber(color:sub(1, 2), 16), tonumber(color:sub(3, 4), 16), tonumber(color:sub(5, 6), 16)
	fill:SetColorTexture(r / 255, g / 255, b / 255, 1)
	swatch.color = color
	swatch:SetScript("OnClick", function(self)
		ns.Store.db.whisperColor = self.color
		SettingsTab.Refresh()
	end)
	return swatch
end

local function BuildReplyLine(y)
	RowLabel("Reply whisper", y)
	ui.whisperOn = Checkbox("GnomishRelaySettingsReply", 20 + LABEL_WIDTH, y + 2, function(on)
		ns.Store.db.whisperOn = on
	end)
	ui.swatches = {}
	for i, color in ipairs(COLORS) do
		ui.swatches[i] = Swatch(i, color, y)
	end
	local soundX = 20 + LABEL_WIDTH + 34 + #COLORS * 26 + 14
	ui.sound = Checkbox("GnomishRelaySettingsSound", soundX, y + 2, function(on)
		ns.Store.db.whisperSound = on
	end)
	Label(ui.page, "GameFontHighlight", soundX + 26, y - 6):SetText("Sound")
	ui.preview = Label(ui.page, "GameFontHighlight", 20 + LABEL_WIDTH, y - ROW + 2)
end

local function BuildAppearance()
	local top = APPEARANCE_TOP
	Heading("Appearance", top)
	BuildFontSize(top - 28)
	BuildReplyLine(top - 28 - ROW)
	local y = top - 28 - 3 * ROW
	RowLabel("Window position", y)
	local reset = CreateFrame("Button", "GnomishRelaySettingsReset", ui.page, "UIPanelButtonTemplate")
	reset:SetSize(90, 22)
	reset:SetPoint("TOPLEFT", ui.page, "TOPLEFT", 20 + LABEL_WIDTH, y)
	reset:SetText("Reset")
	reset:SetScript("OnClick", ns.Window.ResetPosition)
	-- The page has no free row, so the quick actions share the row of the window position.
	local x = 20 + LABEL_WIDTH + 130
	Label(ui.page, "GameFontHighlight", x, y - 6):SetText("Quick actions")
	local edit = CreateFrame("Button", "GnomishRelaySettingsQuick", ui.page, "UIPanelButtonTemplate")
	edit:SetSize(90, 22)
	edit:SetPoint("TOPLEFT", ui.page, "TOPLEFT", x + 110, y)
	edit:SetText("Edit")
	edit:SetScript("OnClick", ns.QuickEditor.Open)
end

local function Alert(name, key, text, x, y)
	local box = Checkbox(name, x, y + 2, function(on)
		ns.Store.db[key] = on
	end, ui.notify)
	Label(ui.notify, "GameFontHighlight", x + 26, y - 6):SetText(text)
	return box
end

local function BuildNotify()
	ui.notify = CreateFrame("Frame", "GnomishRelaySettingsNotify", ui.page)
	ui.notify:SetPoint("TOPLEFT", ui.page, "TOPLEFT", 0, NOTIFY_TOP)
	ui.notify:SetSize(ui.page:GetWidth(), NOTIFY_HEIGHT)
	Label(ui.notify, "GameFontNormalLarge", 16, 0):SetText("Notifications")
	RowLabel("Notifications", -28, ui.notify)
	ui.notifyOn = Checkbox("GnomishRelaySettingsNotifyOn", 20 + LABEL_WIDTH, -26, function(on)
		ns.Store.db.notifyOn = on
		ns.NoticeFrames.Refresh()
	end, ui.notify)
	local finishedLabel = Label(ui.notify, "GameFontHighlight", 20 + LABEL_WIDTH + 44, -34)
	finishedLabel:SetText("Finished tasks")
	ui.finished = Dropdown("GnomishRelaySettingsFinished", -28, 110, function(value)
		ns.Store.db.notifyFinished = value
		ns.Notices.Refilter()
		SettingsTab.Refresh()
	end, 20 + LABEL_WIDTH + 140, ui.notify)
	local choices = {}
	for _, choice in ipairs(ns.Notices.FINISHED) do
		table.insert(choices, { value = choice.value, text = choice.text })
	end
	SetChoices(ui.finished, choices)
	RowLabel("Alerts", -28 - ROW, ui.notify)
	local x = 20 + LABEL_WIDTH
	ui.notifyChat = Alert("GnomishRelaySettingsNotifyChat", "notifyChat", "Chat line", x, -28 - ROW)
	ui.notifySound = Alert("GnomishRelaySettingsNotifySound", "notifySound", "Sound", x + 100, -28 - ROW)
	ui.notifyToast = Alert("GnomishRelaySettingsNotifyToast", "notifyToast", "Banner", x + 180, -28 - ROW)
	ui.notifyParts = { finishedLabel, ui.finished, ui.notifyChat, ui.notifySound, ui.notifyToast }
	ui.notify:Hide()
end

local function BuildStatus()
	ui.status = CreateFrame("Button", "GnomishRelaySettingsStatus", ui.page)
	ui.status:SetSize(200, 20)
	ui.status:SetPoint("BOTTOMRIGHT", ui.page, "BOTTOMRIGHT", -12, 10)
	ui.status.text = ui.status:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	ui.status.text:SetPoint("RIGHT", ui.status, "RIGHT", 0, 0)
	ui.status.text:SetJustifyH("RIGHT")
	ui.status:SetScript("OnClick", function()
		ns.BridgeSettings.Ask()
		SettingsTab.Refresh()
	end)
end

-- `page` is the inset that takes the place of the center and the Activity panel.
function SettingsTab.Build(page)
	ui.page = page
	BuildNewChats()
	BuildAppearance()
	BuildNotify()
	ns.RulesGroup.Build(page, NOTIFY_TOP)
	BuildStatus()
	WatchClicks()
end
