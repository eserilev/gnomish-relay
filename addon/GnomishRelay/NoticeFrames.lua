-- The bell at the minimap, the list of notifications below it, and the toast above the
-- chat frame (SPEC.md 10.4). Notices.lua holds the list. Nothing here runs anything.

local _, ns = ...

local NoticeFrames = {}
ns.NoticeFrames = NoticeFrames

local ROWS = 20
local WIDTH = 320
local ROW_WIDTH = WIDTH - 24
local LINE = 14
local PREVIEW = 60
-- A repo can be 64 bytes. A cut keeps the state and the age on the one line of the head.
local REPO_BYTES = 24
local TOAST_TEXT = 150
local TOAST_SECONDS = 8
local DEFAULT_ANGLE = 200
local GOLD = "ffd100"
local GREY = "8d8778"
local STATES = {
	waiting = { color = "ff9f40", word = "Waiting" },
	finished = { color = "1eff00", word = "Finished" },
	failed = { color = "ff2020", word = "Failed" },
}
local BORDER = "Interface\\Minimap\\MiniMap-TrackingBorder"
local BACKGROUND = "Interface\\Minimap\\UI-Minimap-Background"
local HIGHLIGHT = "Interface\\Minimap\\UI-Minimap-ZoomButton-Highlight"

local ui = { rows = {}, open = {} }

local function Ago(seconds)
	return ns.Notices.Duration(math.floor(seconds)) .. " ago"
end

local function StateText(n)
	local s = STATES[n.kind]
	local word = s.word
	if n.kind ~= "waiting" and n.took > 0 then
		word = word .. " · " .. ns.Notices.Duration(n.took)
	end
	return "|cff" .. s.color .. word .. "|r"
end

local function Texture(parent, layer, path, size)
	local t = parent:CreateTexture(nil, layer)
	t:SetTexture(path)
	t:SetSize(size, size)
	return t
end

function NoticeFrames.PlaceBell()
	local angle = math.rad(ns.Store.db.bellAngle or DEFAULT_ANGLE)
	local radius = Minimap:GetWidth() / 2 + 10
	ui.bell:ClearAllPoints()
	ui.bell:SetPoint("CENTER", Minimap, "CENTER", math.cos(angle) * radius, math.sin(angle) * radius)
end

-- The saved variables keep the angle, so the bell stays where the player put it.
local function FollowCursor()
	local x, y = GetCursorPosition()
	local scale = Minimap:GetEffectiveScale()
	local cx, cy = Minimap:GetCenter()
	ns.Store.db.bellAngle = math.deg(math.atan2(y / scale - cy, x / scale - cx))
	NoticeFrames.PlaceBell()
end

local function BuildBell()
	local bell = CreateFrame("Button", "GnomishRelayBell", Minimap)
	bell:SetSize(31, 31)
	bell:SetFrameStrata("MEDIUM")
	bell:SetFrameLevel(8)
	Texture(bell, "BACKGROUND", BACKGROUND, 20):SetPoint("CENTER", bell, "CENTER", 0, 0)
	bell.icon = bell:CreateTexture(nil, "ARTWORK")
	ns.Atlases.SetBell(bell.icon)
	bell.icon:SetSize(18, 18)
	bell.icon:SetPoint("CENTER", bell, "CENTER", 0, 0)
	Texture(bell, "OVERLAY", BORDER, 53):SetPoint("TOPLEFT", bell, "TOPLEFT", 0, 0)
	bell.glow = Texture(bell, "OVERLAY", HIGHLIGHT, 31)
	bell.glow:SetPoint("CENTER", bell, "CENTER", 0, 0)
	bell.glow:SetBlendMode("ADD")
	bell:SetHighlightTexture(HIGHLIGHT, "ADD")
	bell:RegisterForDrag("LeftButton")
	bell:SetScript("OnDragStart", function(self)
		self:SetScript("OnUpdate", FollowCursor)
	end)
	bell:SetScript("OnDragStop", function(self)
		self:SetScript("OnUpdate", nil)
	end)
	bell:SetScript("OnClick", function()
		NoticeFrames.ToggleList()
	end)
	bell:Hide()
	ui.bell = bell
	NoticeFrames.PlaceBell()
end

local function Text(parent, font)
	local text = parent:CreateFontString(nil, "OVERLAY", font)
	text:SetJustifyH("LEFT")
	return text
end

local function Row(i)
	local row = CreateFrame("Button", "GnomishRelayNotice" .. i, ui.list)
	row:SetWidth(ROW_WIDTH)
	row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	row.head = Text(row, "GameFontHighlightSmall")
	row.head:SetPoint("TOPLEFT", row, "TOPLEFT", 0, 0)
	row.head:SetWidth(ROW_WIDTH)
	row.head:SetWordWrap(false)
	row.text = Text(row, "GameFontHighlightSmall")
	row.text:SetPoint("TOPLEFT", row, "TOPLEFT", 8, -LINE)
	row.text:SetWidth(ROW_WIDTH - 8)
	row:SetScript("OnClick", function(self)
		ui.open[self.id] = not ui.open[self.id] or nil
		NoticeFrames.Refresh()
	end)
	return row
end

-- A click on a row shows its full text, and a second click folds it.
local function ShowRow(row, n, y)
	local color = ns.Relay.AgentColor(n.source)
	row.id = n.id
	row.head:SetText(
		string.format(
			"|cff%s%s|r  |cff%s%s|r  %s  |cff%s%s|r",
			color,
			ns.Notices.Agent(n),
			GOLD,
			ns.Notices.Cut(n.repo, REPO_BYTES),
			StateText(n),
			GREY,
			Ago(ns.Notices.Age(n))
		)
	)
	local full = ui.open[n.id]
	row.text:SetWordWrap(full == true)
	row.text:SetText(full and n.text or ns.Notices.Cut(n.text, PREVIEW))
	local height = LINE + (full and row.text:GetStringHeight() or LINE) + 6
	row:SetHeight(height)
	row:SetPoint("TOPLEFT", ui.list, "TOPLEFT", 12, y)
	row:Show()
	return height
end

local function RefreshList()
	local list = ns.Notices.List()
	local y = -34
	for i = 1, ROWS do
		local n = list[i]
		if n then
			ui.rows[i] = ui.rows[i] or Row(i)
			y = y - ShowRow(ui.rows[i], n, y)
		elseif ui.rows[i] then
			ui.rows[i]:Hide()
		end
	end
	ui.list:SetHeight(-y + 12)
end

local function BuildList()
	local list = CreateFrame("Frame", "GnomishRelayNotices", UIParent, "TooltipBackdropTemplate")
	list:SetWidth(WIDTH)
	list:SetPoint("TOPRIGHT", Minimap, "BOTTOMRIGHT", 0, -24)
	list:SetFrameStrata("DIALOG")
	list:SetClampedToScreen(true)
	list:EnableMouse(true)
	local title = Text(list, "GameFontNormal")
	title:SetPoint("TOPLEFT", list, "TOPLEFT", 12, -12)
	title:SetText("Notifications")
	local close = CreateFrame("Button", "GnomishRelayNoticesClose", list, "UIPanelCloseButton")
	close:SetPoint("TOPRIGHT", list, "TOPRIGHT", -2, -2)
	local clear = CreateFrame("Button", "GnomishRelayNoticesClear", list, "UIPanelButtonTemplate")
	clear:SetSize(70, 20)
	-- In the title row, so a list longer than the screen never hides it.
	clear:SetPoint("RIGHT", close, "LEFT", -2, 0)
	clear:SetText("Clear")
	clear:SetScript("OnClick", function()
		ns.Notices.Clear()
	end)
	-- Escape closes a frame of this list, as it closes the frames of the game.
	table.insert(UISpecialFrames, "GnomishRelayNotices")
	list:Hide()
	ui.list = list
end

local function BuildToast()
	local toast = CreateFrame("Button", "GnomishRelayToast", UIParent, "TooltipBackdropTemplate")
	toast:SetSize(300, 58)
	toast:SetPoint("BOTTOMLEFT", DEFAULT_CHAT_FRAME, "TOPLEFT", 0, 40)
	toast:SetFrameStrata("DIALOG")
	toast:SetClampedToScreen(true)
	toast.title = Text(toast, "GameFontNormal")
	toast.title:SetPoint("TOPLEFT", toast, "TOPLEFT", 12, -10)
	toast.title:SetWidth(276)
	toast.title:SetWordWrap(false)
	toast.text = Text(toast, "GameFontHighlightSmall")
	toast.text:SetPoint("TOPLEFT", toast, "TOPLEFT", 12, -26)
	toast.text:SetWidth(276)
	toast.text:SetMaxLines(2)
	toast:SetScript("OnClick", function(self)
		self:Hide()
		NoticeFrames.OpenList()
	end)
	toast:Hide()
	ui.toast = toast
end

function NoticeFrames.ShowToast(n)
	local toast = ui.toast
	toast.title:SetText(string.format("%s is waiting · %s", ns.Notices.Agent(n), n.repo))
	toast.text:SetText(ns.Notices.Cut(n.text, TOAST_TEXT))
	toast:Show()
	local shownAt = GetTime()
	toast.shownAt = shownAt
	C_Timer.After(TOAST_SECONDS, function()
		if toast.shownAt == shownAt then
			toast:Hide()
		end
	end)
end

local function HasNotices()
	return ns.Notices.On() and #ns.Notices.List() > 0
end

function NoticeFrames.OpenList()
	if HasNotices() then
		ui.list:Show()
		RefreshList()
	end
end

function NoticeFrames.ToggleList()
	if ui.list:IsShown() then
		ui.list:Hide()
	else
		NoticeFrames.OpenList()
	end
end

-- The bell shows only while the list holds a notice, and glows while one waits.
function NoticeFrames.Refresh()
	if not ui.bell then
		return
	end
	local shown = HasNotices()
	ui.bell:SetShown(shown)
	ui.bell.glow:SetShown(shown and ns.Notices.HasWaiting())
	if not shown then
		ui.list:Hide()
	elseif ui.list:IsShown() then
		RefreshList()
	end
end

function NoticeFrames.Build()
	BuildBell()
	BuildList()
	BuildToast()
	ns.Notices.OnChange = NoticeFrames.Refresh
	ns.Notices.OnToast = NoticeFrames.ShowToast
end
