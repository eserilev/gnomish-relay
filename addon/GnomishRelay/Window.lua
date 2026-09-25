-- The chat window, in the layout of the Guild & Communities frame (SPEC.md 13.1).

local _, ns = ...

local Window = {}
ns.Window = Window

local WIDTH, HEIGHT = 900, 560
local SIDE = 200
local TILE_HEIGHT = 48
local STEP_ROWS = 14
local PICK_ROWS = 20
local PICK_ROW_HEIGHT = 19
local GREY = "9d9d9d"
local GREEN = "1eff00"
local MAX_INPUT = 3000
local EMBLEM = "Interface\\Icons\\INV_Misc_Wrench_01"
local STATUS_BAR = "Interface\\TargetingFrame\\UI-StatusBar"
local YOU = "69ccf0"
local CODE = "b8c8b8"

local frame
local tiles = {}
local ui = {}

-- The selected chat, or the first chat when none is selected.
local function Selected()
	local db = ns.Store.db
	return db.selected and ns.Store.Chat(db.selected) or db.chats[1]
end

local function MarkSelected(chatId)
	local chat = ns.Store.Chat(chatId)
	if chat then
		ns.Store.db.selected = chat.id
		chat.unread = nil
	end
end

local function Select(chatId)
	ui.picking = false
	MarkSelected(chatId)
	Window.Refresh()
end

local function Inset(parent, left, top, width, bottom)
	local inset = CreateFrame("Frame", nil, parent, "InsetFrameTemplate")
	inset:SetPoint("TOPLEFT", parent, "TOPLEFT", left, top)
	inset:SetPoint("BOTTOMLEFT", parent, "BOTTOMLEFT", left, bottom)
	inset:SetWidth(width)
	return inset
end

local function Label(parent, font, point, x, y)
	local text = parent:CreateFontString(nil, "OVERLAY", font)
	text:SetPoint(point, parent, point, x, y)
	text:SetJustifyH("LEFT")
	return text
end

local function Tile(index)
	local tile = tiles[index]
	if tile then
		return tile
	end
	tile = CreateFrame("Button", "GnomishRelayTile" .. index, ui.chats)
	tile:SetSize(SIDE - 12, TILE_HEIGHT)
	tile:SetPoint("TOPLEFT", ui.chats, "TOPLEFT", 6, -6 - (index - 1) * (TILE_HEIGHT + 4))
	tile.bg = tile:CreateTexture(nil, "BACKGROUND")
	tile.bg:SetAllPoints()
	tile:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	tile.name = Label(tile, "GameFontNormal", "TOPLEFT", 10, -9)
	tile.agent = Label(tile, "GameFontHighlightSmall", "BOTTOMLEFT", 10, 9)
	tile.mark = Label(tile, "GameFontNormalLarge", "RIGHT", -10, 0)
	tile:RegisterForClicks("LeftButtonUp", "RightButtonUp")
	tile:SetScript("OnClick", function(self, button)
		if button == "RightButton" then
			if self.chatId then
				Window.AskDelete(self.chatId)
			end
		elseif self.resume then
			Window.ShowSessions()
		elseif self.chatId then
			Select(self.chatId)
		else
			Select(ns.Store.NewChat().id)
		end
	end)
	tiles[index] = tile
	return tile
end

local function ShowResumeTile(index)
	local tile = Tile(index)
	tile.chatId = nil
	tile.resume = true
	tile.name:SetText("|cff1eff00Resume|r")
	tile.agent:SetText("")
	tile.mark:SetText("")
	if ui.picking then
		tile.bg:SetColorTexture(0.12, 0.35, 0.12, 0.9)
	else
		tile.bg:SetColorTexture(0.1, 0.15, 0.2, 0.9)
	end
	tile:Show()
end

local function ShowTile(index, chat, selected)
	local tile = Tile(index)
	tile.resume = nil
	tile.chatId = chat and chat.id
	if chat then
		tile.name:SetText(ns.Relay.Plain(chat.name))
		tile.agent:SetText(ns.Relay.AgentName(chat.agent))
		local mark = ""
		if chat.unread then
			mark = "!"
		elseif ns.Transport.Working(chat.id) then
			mark = "..."
		end
		tile.mark:SetText(mark)
	else
		tile.name:SetText("|cff1eff00New Chat|r")
		tile.agent:SetText("")
		tile.mark:SetText("|cff1eff00+|r")
	end
	if selected then
		tile.bg:SetColorTexture(0.12, 0.35, 0.12, 0.9)
	else
		tile.bg:SetColorTexture(0.1, 0.15, 0.2, 0.9)
	end
	tile:Show()
end

local function RefreshTiles(current)
	local chats = ns.Store.Chats()
	for i, chat in ipairs(chats) do
		ShowTile(i, chat, not ui.picking and current and chat.id == current.id)
	end
	ShowTile(#chats + 1, nil, false)
	ShowResumeTile(#chats + 2)
	for i = #chats + 3, #tiles do
		tiles[i]:Hide()
	end
end

local function AddText(name, color, text)
	local prefix = string.format("|cff%s[%s]|r: ", color, name)
	local inCode = false
	for line in (tostring(text) .. "\n"):gmatch("([^\n]*)\n") do
		if line:match("^```") then
			inCode = not inCode
		elseif inCode then
			ui.transcript:AddMessage(string.format("    |cff%s%s|r", CODE, ns.Relay.Plain(line)))
		else
			ui.transcript:AddMessage(prefix .. ns.Relay.Plain(line))
			prefix = ""
		end
	end
end

local function RefreshTranscript(chat)
	ui.transcript:Clear()
	if not chat then
		return
	end
	for _, entry in ipairs(chat.history) do
		if entry.attach then
			ui.transcript:AddMessage(string.format('|cff%sResumed "%s"|r', GREY, ns.Relay.Plain(chat.name)))
		elseif entry.role == "user" then
			AddText("You", YOU, entry.text)
		elseif entry.role == "error" then
			AddText(ns.Relay.AgentName(entry.agent or chat.agent), "ff2020", entry.text)
		else
			local agent = entry.agent or chat.agent
			AddText(ns.Relay.AgentName(agent), ns.Relay.AgentColor(agent), entry.text)
		end
	end
	ui.transcript:ScrollToBottom()
end

local function Elapsed(seconds)
	return string.format("%d:%02d", math.floor(seconds / 60), math.floor(seconds % 60))
end

local function RefreshActivity(chat)
	local working = chat and ns.Transport.Working(chat.id)
	ui.cast:SetShown(working ~= nil)
	ui.stop:SetShown(working ~= nil)
	local progress = working and working.progress or {}
	local first = math.max(1, #progress - STEP_ROWS + 1)
	for i, row in ipairs(ui.steps) do
		local step = progress[first + i - 1]
		row.detail = step
		row.text:SetText(step and ns.Relay.Plain(step) or "")
		row:SetShown(step ~= nil)
	end
end

local function RefreshStatus(chat)
	if chat then
		ui.agent:SetText(ns.Relay.AgentName(chat.agent) .. " · " .. chat.mode)
		ui.folder:SetText(chat.cwd ~= "" and ns.Relay.Plain(chat.cwd) or "")
	else
		ui.agent:SetText("")
		ui.folder:SetText("")
	end
	local problem = ns.Transport.Problem()
	if problem == "missing" then
		ui.bridge:SetText("|cffff2020Slots missing|r")
	elseif problem == "blocked" then
		ui.bridge:SetText("|cffff2020Screenshots blocked|r")
	elseif problem == "mismatch" then
		ui.bridge:SetText("|cffff2020Update the bridge|r")
	elseif ns.Transport.Online() then
		ui.bridge:SetText("|cff1eff00Bridge online|r")
	else
		ui.bridge:SetText("|cff9d9d9dBridge offline|r")
	end
	local outbox = #ns.Store.db.outbox > 0
	ui.banner:SetShown(ns.Transport.NeedsReload())
	ui.bannerText:SetText(outbox and "Reload to send" or "Reload soon")
end

local function Age(seconds)
	if seconds < 60 then
		return "now"
	elseif seconds < 3600 then
		return math.floor(seconds / 60) .. " min"
	elseif seconds < 86400 then
		return math.floor(seconds / 3600) .. " h"
	end
	return math.floor(seconds / 86400) .. " d"
end

-- The rows of the picker: a heading for each folder, then its sessions, newest first.
local function PickLines()
	local sessions = ns.Store.db.sessions
	local lines, groups, order = {}, {}, {}
	for _, row in ipairs(sessions and sessions.rows or {}) do
		if not groups[row.repo] then
			groups[row.repo] = {}
			table.insert(order, row.repo)
		end
		table.insert(groups[row.repo], row)
	end
	for _, repo in ipairs(order) do
		table.insert(lines, { heading = repo })
		for _, row in ipairs(groups[repo]) do
			table.insert(lines, { row = row })
		end
	end
	return lines
end

local function ShowPickRow(button, line, since)
	button.row = line and line.row
	if not line then
		button:Hide()
		return
	end
	if line.heading then
		button.text:SetText("|cffffd100" .. ns.Relay.Plain(line.heading) .. "|r")
		button.right:SetText("")
	else
		local row = line.row
		local age = row.active and ("|cff" .. GREEN .. "open|r") or Age(row.age + since)
		button.text:SetText("   " .. ns.Relay.Plain(row.title ~= "" and row.title or row.session))
		button.right:SetText(ns.Relay.AgentName(row.agent) .. "  " .. age)
	end
	button:Show()
end

local function RefreshPicker()
	local sessions = ns.Store.db.sessions
	local lines = PickLines()
	ui.pickOffset = math.max(0, math.min(ui.pickOffset or 0, #lines - PICK_ROWS))
	local since = sessions and time() - sessions.at or 0
	for i, button in ipairs(ui.pickRows) do
		ShowPickRow(button, lines[ui.pickOffset + i], since)
	end
	local note = ""
	if sessions and sessions.error then
		note = "|cffff2020" .. ns.Relay.Plain(sessions.error) .. "|r"
	elseif #lines == 0 and ns.Transport.Listing() then
		note = "Loading..."
	elseif #lines == 0 then
		note = "No sessions"
	end
	ui.pickNote:SetText(note)
end

function Window.Refresh()
	if not frame or not frame:IsShown() then
		return
	end
	local chat = Selected()
	RefreshTiles(chat)
	ui.log:SetShown(not ui.picking)
	ui.input:SetShown(not ui.picking)
	ui.picker:SetShown(ui.picking == true)
	if ui.picking then
		RefreshPicker()
	else
		RefreshTranscript(chat)
	end
	RefreshActivity(not ui.picking and chat or nil)
	RefreshStatus(not ui.picking and chat or nil)
end

function Window.ShowSessions()
	ui.picking = true
	ui.pickOffset = 0
	ns.Transport.ListSessions()
	Window.Refresh()
end

-- A session that already has a chat opens that chat.
function Window.Resume(row)
	if row.chat and ns.Store.Chat(row.chat) then
		Select(row.chat)
		return
	end
	local chat = ns.Store.ResumeChat(row)
	ns.Transport.Attach(chat)
	Select(chat.id)
end

-- Returns false, and shows an error, for a message that does not fit in one strip.
function Window.Send(text)
	ui.picking = false
	local chat = Selected() or ns.Store.NewChat()
	ns.Store.db.selected = chat.id
	if not ns.Transport.Send(chat, text) then
		UIErrorsFrame:AddMessage("Too long to send.", 1, 0.1, 0.1)
		return false
	end
	Window.Refresh()
	-- A click or Enter is a hardware event, the only time ReloadUI is allowed.
	if ns.Transport.NeedsReload() and not InCombatLockdown() then
		ReloadUI()
	end
	return true
end

local function BuildCenter()
	local left = SIDE + 14
	local width = WIDTH - 2 * SIDE - 28

	ui.agent = Label(frame, "GameFontNormal", "TOPLEFT", left, -64)
	ui.folder = Label(frame, "GameFontDisableSmall", "TOPLEFT", left + 170, -66)
	ui.bridge = Label(frame, "GameFontNormalSmall", "TOPRIGHT", -SIDE - 14, -66)

	local log = Inset(frame, left, -84, width, 72)
	ui.log = log
	ui.transcript = CreateFrame("ScrollingMessageFrame", "GnomishRelayTranscript", log)
	ui.transcript:SetPoint("TOPLEFT", log, "TOPLEFT", 8, -6)
	ui.transcript:SetPoint("BOTTOMRIGHT", log, "BOTTOMRIGHT", -8, 6)
	ui.transcript:SetFontObject(ChatFontNormal)
	ui.transcript:SetJustifyH("LEFT")
	ui.transcript:SetFading(false)
	ui.transcript:SetMaxLines(2000)
	ui.transcript:SetIndentedWordWrap(true)
	ui.transcript:EnableMouseWheel(true)
	ui.transcript:SetScript("OnMouseWheel", function(self, delta)
		if delta > 0 then
			self:ScrollUp()
		else
			self:ScrollDown()
		end
	end)

	ui.picker = Inset(frame, left, -84, width, 16)
	ui.pickNote = ui.picker:CreateFontString("GnomishRelayPickNote", "OVERLAY", "GameFontDisable")
	ui.pickNote:SetPoint("TOPLEFT", ui.picker, "TOPLEFT", 12, -12)
	ui.pickRows = {}
	for i = 1, PICK_ROWS do
		local row = CreateFrame("Button", "GnomishRelayPick" .. i, ui.picker)
		row:SetPoint("TOPLEFT", ui.picker, "TOPLEFT", 8, -8 - (i - 1) * PICK_ROW_HEIGHT)
		row:SetSize(width - 16, PICK_ROW_HEIGHT)
		row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
		row.text = Label(row, "GameFontHighlight", "LEFT", 4, 0)
		row.text:SetWidth(width - 150)
		row.text:SetWordWrap(false)
		row.right = Label(row, "GameFontHighlightSmall", "RIGHT", -4, 0)
		row.right:SetJustifyH("RIGHT")
		row:SetScript("OnClick", function(self)
			if self.row then
				Window.Resume(self.row)
			end
		end)
		row:Hide()
		ui.pickRows[i] = row
	end
	ui.picker:EnableMouseWheel(true)
	ui.picker:SetScript("OnMouseWheel", function(_, delta)
		ui.pickOffset = (ui.pickOffset or 0) - delta * 3
		RefreshPicker()
	end)
	ui.picker:Hide()

	ui.banner = CreateFrame("Frame", nil, frame)
	ui.banner:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left, 44)
	ui.banner:SetSize(width, 24)
	ui.bannerText = Label(ui.banner, "GameFontNormal", "LEFT", 6, 0)
	local reload = CreateFrame("Button", nil, ui.banner, "UIPanelButtonTemplate")
	reload:SetSize(90, 22)
	reload:SetPoint("RIGHT", ui.banner, "RIGHT", 0, 0)
	reload:SetText("Reload")
	reload:SetScript("OnClick", function()
		if not InCombatLockdown() then
			ReloadUI()
		end
	end)
	ui.banner:Hide()

	ui.input = CreateFrame("EditBox", "GnomishRelayInput", frame, "InputBoxTemplate")
	ui.input:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left + 6, 16)
	ui.input:SetSize(width - 6, 24)
	ui.input:SetAutoFocus(false)
	ui.input:SetMaxBytes(MAX_INPUT)
	ui.input:SetScript("OnEnterPressed", function(self)
		local text = strtrim(self:GetText() or "")
		if text == "" then
			self:ClearFocus()
		elseif Window.Send(text) then
			self:SetText("")
		end
	end)
	ui.input:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
	end)
end

local function ShowStepTooltip(row)
	if row.detail then
		GameTooltip:SetOwner(row, "ANCHOR_LEFT")
		GameTooltip:SetText(row.detail, 1, 1, 1, 1, true)
		GameTooltip:Show()
	end
end

local function BuildActivity()
	local panel = Inset(frame, WIDTH - SIDE - 6, -84, SIDE, 44)
	Label(frame, "GameFontNormal", "TOPRIGHT", -SIDE + 60, -64):SetText("Activity")

	ui.cast = CreateFrame("StatusBar", nil, panel)
	ui.cast:SetPoint("TOPLEFT", panel, "TOPLEFT", 8, -8)
	ui.cast:SetPoint("TOPRIGHT", panel, "TOPRIGHT", -8, -8)
	ui.cast:SetHeight(16)
	ui.cast:SetStatusBarTexture(STATUS_BAR)
	ui.cast:SetStatusBarColor(1, 0.7, 0)
	ui.cast:SetMinMaxValues(0, 1)
	ui.cast.text = Label(ui.cast, "GameFontHighlightSmall", "CENTER", 0, 0)
	ui.cast:SetScript("OnUpdate", function(self)
		local chat = Selected()
		local working = chat and ns.Transport.Working(chat.id)
		if working then
			local elapsed = GetTime() - working.since
			self:SetValue(elapsed % 10 / 10)
			self.text:SetText("Tinkering " .. Elapsed(elapsed))
		end
	end)
	ui.cast:Hide()

	ui.steps = {}
	for i = 1, STEP_ROWS do
		local row = CreateFrame("Button", nil, panel)
		row:SetPoint("TOPLEFT", panel, "TOPLEFT", 8, -30 - (i - 1) * 18)
		row:SetSize(SIDE - 16, 18)
		row.text = Label(row, "GameFontHighlightSmall", "LEFT", 0, 0)
		row.text:SetWidth(SIDE - 16)
		row.text:SetWordWrap(false)
		row:SetScript("OnEnter", ShowStepTooltip)
		row:SetScript("OnLeave", function()
			GameTooltip:Hide()
		end)
		row:Hide()
		ui.steps[i] = row
	end

	ui.stop = CreateFrame("Button", "GnomishRelayStop", frame, "UIPanelButtonTemplate")
	ui.stop:SetSize(90, 22)
	ui.stop:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -12, 14)
	ui.stop:SetText("Stop")
	ui.stop:SetScript("OnClick", function()
		local chat = Selected()
		if chat then
			ns.Transport.Stop(chat)
		end
	end)
	ui.stop:Hide()
end

local function BuildConfirm()
	ui.confirm = CreateFrame("Frame", "GnomishRelayConfirm", frame)
	ui.confirm:SetFrameStrata("DIALOG")
	ui.confirm:SetSize(320, 90)
	ui.confirm:SetPoint("CENTER", frame, "CENTER", 0, 40)
	ui.confirm:EnableMouse(true)
	local background = ui.confirm:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.9)
	ui.confirmText = Label(ui.confirm, "GameFontHighlight", "TOPLEFT", 12, -14)
	ui.confirmText:SetWidth(296)
	local delete = CreateFrame("Button", "GnomishRelayConfirmDelete", ui.confirm, "UIPanelButtonTemplate")
	delete:SetSize(100, 22)
	delete:SetPoint("BOTTOMLEFT", ui.confirm, "BOTTOMLEFT", 12, 12)
	delete:SetText("Delete")
	delete:SetScript("OnClick", function()
		local chat = ns.Store.Chat(ui.confirm.chatId)
		ui.confirm:Hide()
		if chat then
			ns.Transport.Delete(chat)
		end
	end)
	local cancel = CreateFrame("Button", "GnomishRelayConfirmCancel", ui.confirm, "UIPanelButtonTemplate")
	cancel:SetSize(100, 22)
	cancel:SetPoint("BOTTOMRIGHT", ui.confirm, "BOTTOMRIGHT", -12, 12)
	cancel:SetText("Cancel")
	cancel:SetScript("OnClick", function()
		ui.confirm:Hide()
	end)
	ui.confirm:Hide()
end

-- A chat that still works gets Stop and Delete in one click: the bridge stops the run.
function Window.AskDelete(chatId)
	local chat = ns.Store.Chat(chatId)
	if not chat then
		return
	end
	local name = ns.Relay.Plain(chat.name)
	if ns.Transport.Working(chat.id) then
		ui.confirmText:SetText(string.format('Stop and delete "%s"?', name))
	else
		ui.confirmText:SetText(string.format('Delete "%s"?', name))
	end
	ui.confirm.chatId = chat.id
	ui.confirm:Show()
end

local function Build()
	frame = CreateFrame("Frame", "GnomishRelayFrame", UIParent, "PortraitFrameTemplate")
	frame:SetSize(WIDTH, HEIGHT)
	frame:SetPoint("CENTER")
	frame:SetMovable(true)
	frame:EnableMouse(true)
	frame:SetClampedToScreen(true)
	frame:RegisterForDrag("LeftButton")
	frame:SetScript("OnDragStart", frame.StartMoving)
	frame:SetScript("OnDragStop", frame.StopMovingOrSizing)
	frame:SetScript("OnShow", Window.Refresh)
	table.insert(UISpecialFrames, "GnomishRelayFrame")

	if frame.SetTitle then
		frame:SetTitle("Gnomish Relay")
	end
	local portrait = frame.PortraitContainer and frame.PortraitContainer.portrait or frame.portrait
	if portrait then
		portrait:SetTexture(EMBLEM)
	end

	ui.chats = Inset(frame, 6, -60, SIDE, 30)
	BuildCenter()
	BuildActivity()
	BuildConfirm()
	frame:Hide()
end

function Window.Showing(chatId)
	local chat = Selected()
	return frame ~= nil and frame:IsShown() and chat ~= nil and chat.id == chatId
end

function Window.Open(chatId)
	if not frame then
		Build()
	end
	if chatId then
		ui.picking = false
		MarkSelected(chatId)
	end
	frame:Show()
	Window.Refresh()
end

function Window.Toggle()
	if frame and frame:IsShown() then
		frame:Hide()
	else
		Window.Open()
	end
end
