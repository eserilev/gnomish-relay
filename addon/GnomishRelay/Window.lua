-- The chat window, in the layout of the Guild & Communities frame (SPEC.md 13.1).

local _, ns = ...

local Window = {}
ns.Window = Window

local WIDTH, HEIGHT = 900, 560
local SIDE = 200
local TILE_HEIGHT = 48
local TAB_WIDTH = 74
local STEP_ROWS = 14
-- The cast bar text changes at most this often, in seconds.
local CAST_UPDATE = 0.2
local PICK_ROWS = 20
local PICK_ROW_HEIGHT = 19
local GREEN = "1eff00"
-- The text of a message is only part of a strip, so Room() is the real limit.
local MAX_INPUT = 3200
-- The input counts the bytes left only near the limit.
local COUNT_FROM = 400
local EMBLEM = "Interface\\Icons\\INV_Misc_Wrench_01"
local FOLDER_ICON = "Interface\\Icons\\INV_Misc_Bag_10"
local ARROW = "Interface\\ChatFrame\\UI-ChatIcon-ScrollDown-Up"
local NEW = "9fe39f"
local STATUS_BAR = "Interface\\TargetingFrame\\UI-StatusBar"
local BODY_FONT = "Fonts\\ARIALN.TTF"
local FONT_MIN, FONT_MAX = 12, 20
-- Pings has no content yet, so it has no tab (SPEC.md 13.1).
local TABS =
	{ { id = "chats", name = "Chats" }, { id = "settings", name = "Settings" }, { id = "diag", name = "Diag" } }

local frame
local tiles = {}
local ui = { tab = "chats" }

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
	ui.tab = "chats"
	ui.picking = false
	ns.Browser.Close()
	MarkSelected(chatId)
	Window.Refresh()
end

local function Inset(parent, left, top, width, bottom, name)
	local inset = CreateFrame("Frame", name, parent, "InsetFrameTemplate")
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
			Window.NewChat()
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

-- The column is 90 less than the window: 60 above it and 30 below.
local function TileRows()
	return math.floor((frame:GetHeight() - 90 - 12) / (TILE_HEIGHT + 4))
end

-- The column shows the tiles from `ui.tileOffset` on: the chats, New Chat, and Resume.
local function RefreshTiles(current)
	local chats = ns.Store.Chats()
	local chatsTab = ui.tab == "chats" and not ui.picking
	local rows = TileRows()
	ui.tileOffset = math.max(0, math.min(ui.tileOffset or 0, #chats + 2 - rows))
	for slot = 1, math.max(rows, #tiles) do
		local i = ui.tileOffset + slot
		local chat = chats[i]
		if slot > rows or i > #chats + 2 then
			Tile(slot):Hide()
		elseif chat then
			ShowTile(slot, chat, chatsTab and current and chat.id == current.id)
		elseif i == #chats + 1 then
			ShowTile(slot, nil, false)
		else
			ShowResumeTile(slot)
		end
	end
end

local function Elapsed(seconds)
	return string.format("%d:%02d", math.floor(seconds / 60), math.floor(seconds % 60))
end

-- Replies come only at a poll, so the player sees when the next one is.
local function UpdateCast(chat)
	local working = chat and ns.Transport.Working(chat.id)
	ui.nextCheck:SetShown(working ~= nil)
	if not working then
		return
	end
	local elapsed = GetTime() - working.since
	ui.cast:SetValue(elapsed % 10 / 10)
	ui.cast.text:SetText("Tinkering " .. Elapsed(elapsed))
	ui.nextCheck:SetText(string.format("Next check in %d s", math.ceil(ns.Transport.NextPollIn())))
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
	UpdateCast(chat)
end

-- Only a click on Reload reloads. A reload from Enter took the game away with no warning.
local function BannerText(waiting)
	if waiting == 1 then
		return "Press Reload to send 1 message."
	elseif waiting > 1 then
		return string.format("Press Reload to send %d messages.", waiting)
	end
	return "Reload soon"
end

local function RefreshStatus(chat)
	if chat then
		ui.agent:SetText(ns.Relay.AgentName(chat.agent) .. " · " .. (chat.level or chat.mode))
		local folder = ns.Relay.Plain(ns.Folders.Display(ns.Folders.Tree(), chat.cwd))
		if chat.newFolder then
			folder = folder .. " |cff" .. NEW .. "new|r"
		end
		ui.folder:SetText(folder)
		ui.folderButton:SetWidth(ui.folder:GetUnboundedStringWidth() + 40)
	else
		ui.agent:SetText("")
	end
	ui.folderButton:SetShown(chat ~= nil)
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
	ui.banner:SetShown(ns.Transport.NeedsReload())
	ui.bannerText:SetText(BannerText(#ns.Store.db.outbox))
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
	elseif #lines == 0 and ns.Transport.ListingSessions() then
		note = "Loading..."
	elseif #lines == 0 then
		note = "No sessions"
	end
	ui.pickNote:SetText(note)
end

local function RefreshTabs()
	for _, tab in ipairs(ui.tabs) do
		local open = tab.id == ui.tab
		tab.bg:SetColorTexture(open and 0.11 or 0.04, open and 0.09 or 0.04, open and 0.06 or 0.05, 0.95)
		tab.label:SetTextColor(open and 1 or 0.6, open and 0.82 or 0.58, open and 0 or 0.51)
	end
end

-- Settings and Diag take the place of the center and the Activity panel. The chat
-- tiles stay, and a click on one goes back to Chats.
local function RefreshPages()
	local chats = ui.tab == "chats"
	for _, part in ipairs(ui.chatParts) do
		part:SetShown(chats)
	end
	ui.settings:SetShown(ui.tab == "settings")
	ui.diag:SetShown(ui.tab == "diag")
	ns.SettingsTab.Refresh()
	ns.DiagTab.Refresh()
end

local function RefreshChats(chat)
	local browsing = not ui.picking and ns.Browser.IsOpen()
	ui.log:SetShown(not ui.picking and not browsing)
	ui.input:SetShown(not ui.picking)
	ui.picker:SetShown(ui.picking == true)
	ns.Browser.Show(browsing)
	if ui.picking then
		RefreshPicker()
	elseif not browsing then
		ns.Transcript.Show(chat)
	end
	RefreshActivity(not ui.picking and chat or nil)
	RefreshStatus(not ui.picking and chat or nil)
end

function Window.Refresh()
	if not frame or not frame:IsShown() then
		return
	end
	local chat = Selected()
	RefreshTiles(chat)
	RefreshTabs()
	RefreshPages()
	if ui.tab == "chats" then
		RefreshChats(chat)
	else
		for _, part in ipairs({ ui.log, ui.input, ui.picker, ui.banner, ui.stop }) do
			part:Hide()
		end
		ns.Browser.Show(false)
	end
end

function Window.ShowTab(id)
	ui.tab = id
	ui.picking = false
	ns.Browser.Close()
	if id ~= "chats" then
		ns.BridgeSettings.AskIfOld()
	end
	Window.Refresh()
end

-- The font size of all chat text. The window keeps its size, and long lines wrap.
function Window.SetFontSize(size)
	size = math.max(FONT_MIN, math.min(FONT_MAX, math.floor(size + 0.5)))
	ns.Store.db.fontSize = size
	if ui.input then
		ui.input:SetFont(BODY_FONT, size, "")
	end
	Window.Refresh()
end

function Window.ResetPosition()
	ns.Store.db.windowPoint = nil
	if frame then
		frame:ClearAllPoints()
		frame:SetPoint("CENTER", UIParent, "CENTER", 0, 0)
	end
end

-- The chat starts in the default folder. The folder button changes it (SPEC.md 9.9).
function Window.NewChat()
	local chat = ns.Store.NewChat()
	-- The new tile is at the end of the column. RefreshTiles clamps the offset.
	ui.tileOffset = math.huge
	Select(chat.id)
end

Window.SelectedChat = Selected

function Window.ToggleBrowser()
	if ns.Browser.IsOpen() then
		ns.Browser.Close()
	else
		ns.Browser.Open(Selected())
	end
	Window.Refresh()
end

function Window.CloseBrowser()
	ns.Browser.Close()
	Window.Refresh()
end

-- The first message fixes the folder of a chat, so a later choice makes a new chat.
function Window.ChooseFolder(folder, name, isNew)
	local chat = Selected()
	if not chat or #chat.history > 0 then
		chat = ns.Store.NewChat(chat and chat.agent)
	end
	ns.Store.SetFolder(chat, folder, name, isNew)
	Select(chat.id)
end

function Window.ShowSessions()
	ns.Browser.Close()
	ui.tab = "chats"
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
	ns.Browser.Close()
	local chat = Selected() or ns.Store.NewChat()
	ns.Store.db.selected = chat.id
	if not ns.Transport.Send(chat, text) then
		UIErrorsFrame:AddMessage("Too long to send.", 1, 0.1, 0.1)
		return false
	end
	Window.Refresh()
	return true
end

function Window.PutBack(text)
	if ui.input then
		ui.input:SetText(text)
		ui.input:SetFocus()
	end
end

-- Rows of a picker: a click calls `choose` with the row that the button shows.
local function PickRows(parent, name, width, choose)
	local rows = {}
	for i = 1, PICK_ROWS do
		local row = CreateFrame("Button", name .. i, parent)
		row:SetPoint("TOPLEFT", parent, "TOPLEFT", 8, -8 - (i - 1) * PICK_ROW_HEIGHT)
		row:SetSize(width - 16, PICK_ROW_HEIGHT)
		row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
		row.text = Label(row, "GameFontHighlight", "LEFT", 4, 0)
		row.text:SetWidth(width - 150)
		row.text:SetWordWrap(false)
		row.right = Label(row, "GameFontHighlightSmall", "RIGHT", -4, 0)
		row.right:SetJustifyH("RIGHT")
		row:SetScript("OnClick", function(self)
			if self.row then
				choose(self.row)
			end
		end)
		row:Hide()
		rows[i] = row
	end
	return rows
end

-- The folder of the chat, as a button like the agent dropdown. Gold on hover.
local function BuildFolderButton(x)
	local button = CreateFrame("Button", "GnomishRelayFolderButton", frame)
	button:SetPoint("TOPLEFT", frame, "TOPLEFT", x, -61)
	button:SetHeight(18)
	local icon = button:CreateTexture(nil, "ARTWORK")
	icon:SetSize(14, 14)
	icon:SetPoint("LEFT", button, "LEFT", 0, 0)
	icon:SetTexture(FOLDER_ICON)
	ui.folder = Label(button, "GameFontDisableSmall", "LEFT", 18, 0)
	local arrow = button:CreateTexture(nil, "ARTWORK")
	arrow:SetSize(16, 16)
	arrow:SetPoint("LEFT", ui.folder, "RIGHT", 2, 0)
	arrow:SetTexture(ARROW)
	button:SetScript("OnEnter", function()
		ui.folder:SetTextColor(1, 0.82, 0)
	end)
	button:SetScript("OnLeave", function()
		ui.folder:SetTextColor(0.5, 0.5, 0.5)
	end)
	button:SetScript("OnClick", Window.ToggleBrowser)
	button.text = ui.folder
	ui.folderButton = button
end

local function RefreshInputHelp()
	local text = ui.input:GetText() or ""
	ui.hint:SetShown(text == "" and not ui.input:HasFocus())
	local chat = Selected()
	local left = chat and ns.Transport.Room(chat) - #text
	ui.count:SetShown(left ~= nil and left < COUNT_FROM)
	if left and left < 0 then
		ui.count:SetText(string.format("|cffff2020%d bytes too many|r", -left))
	elseif left then
		ui.count:SetText(string.format("%d bytes left", left))
	end
end

local function BuildInputHelp()
	ui.hint = ui.input:CreateFontString("GnomishRelayInputHint", "OVERLAY", "GameFontDisable")
	ui.hint:SetPoint("LEFT", ui.input, "LEFT", 2, 0)
	ui.hint:SetText("Type a task. Enter sends.")
	ui.count = ui.input:CreateFontString("GnomishRelayInputCount", "OVERLAY", "GameFontDisableSmall")
	ui.count:SetPoint("BOTTOMRIGHT", ui.input, "TOPRIGHT", 0, 2)
	ui.count:Hide()
	ui.input:SetScript("OnTextChanged", RefreshInputHelp)
	ui.input:SetScript("OnEditFocusGained", RefreshInputHelp)
	ui.input:SetScript("OnEditFocusLost", RefreshInputHelp)
end

local function BuildCenter()
	local left = SIDE + 14
	local width = WIDTH - 2 * SIDE - 28

	ui.agent = Label(frame, "GameFontNormal", "TOPLEFT", left, -64)
	BuildFolderButton(left + 170)
	ui.bridge = Label(frame, "GameFontNormalSmall", "TOPRIGHT", -SIDE - 14, -66)

	local log = Inset(frame, left, -84, width, 72)
	ui.log = log
	-- The inset is HEIGHT - 84 - 72 high, less 6 at the top and the bottom.
	ns.Transcript.Build(log, width - 16, HEIGHT - 84 - 72 - 12)

	ui.picker = Inset(frame, left, -84, width, 16)
	ui.pickNote = ui.picker:CreateFontString("GnomishRelayPickNote", "OVERLAY", "GameFontDisable")
	ui.pickNote:SetPoint("TOPLEFT", ui.picker, "TOPLEFT", 12, -12)
	ui.pickRows = PickRows(ui.picker, "GnomishRelayPick", width, Window.Resume)
	ui.picker:EnableMouseWheel(true)
	ui.picker:SetScript("OnMouseWheel", function(_, delta)
		ui.pickOffset = (ui.pickOffset or 0) - delta * 3
		RefreshPicker()
	end)
	ui.picker:Hide()

	ns.Browser.Build(frame, left, width, 72)

	ui.banner = CreateFrame("Frame", "GnomishRelayBanner", frame)
	ui.banner:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left, 44)
	ui.banner:SetSize(width, 24)
	ui.bannerText = ui.banner:CreateFontString("GnomishRelayBannerText", "OVERLAY", "GameFontNormal")
	ui.bannerText:SetPoint("LEFT", ui.banner, "LEFT", 6, 0)
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
	ui.input:SetFont(BODY_FONT, ns.Store.db.fontSize, "")
	ui.input:SetScript("OnEnterPressed", function(self)
		local text = strtrim(self:GetText() or "")
		if text == "" then
			self:ClearFocus()
		elseif Window.Send(text) then
			-- The game gets its keys back, so a move key after a send moves the player.
			self:SetText("")
			self:ClearFocus()
		end
	end)
	ui.input:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
	end)
	BuildInputHelp()
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
	local title = Label(frame, "GameFontNormal", "TOPRIGHT", -SIDE + 60, -64)
	title:SetText("Activity")
	ui.activity = { panel, title }

	ui.cast = CreateFrame("StatusBar", "GnomishRelayCast", panel)
	ui.cast:SetPoint("TOPLEFT", panel, "TOPLEFT", 8, -8)
	ui.cast:SetPoint("TOPRIGHT", panel, "TOPRIGHT", -8, -8)
	ui.cast:SetHeight(16)
	ui.cast:SetStatusBarTexture(STATUS_BAR)
	ui.cast:SetStatusBarColor(1, 0.7, 0)
	ui.cast:SetMinMaxValues(0, 1)
	ui.cast.text = Label(ui.cast, "GameFontHighlightSmall", "CENTER", 0, 0)
	ui.nextCheck = Label(panel, "GameFontDisableSmall", "BOTTOMLEFT", 8, 8)
	ui.cast:SetScript("OnUpdate", function(self, elapsed)
		self.wait = (self.wait or 0) - elapsed
		if self.wait <= 0 then
			self.wait = CAST_UPDATE
			UpdateCast(Selected())
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

-- The saved variables keep the place, so the window opens where the player left it.
local function SavePosition()
	frame:StopMovingOrSizing()
	frame:SetUserPlaced(false)
	local point, _, relative, x, y = frame:GetPoint()
	if point then
		ns.Store.db.windowPoint = { point = point, relative = relative, x = x, y = y }
	end
end

local function PlaceFrame()
	local saved = ns.Store.db.windowPoint
	frame:ClearAllPoints()
	if type(saved) == "table" and type(saved.point) == "string" then
		frame:SetPoint(saved.point, UIParent, saved.relative or saved.point, saved.x or 0, saved.y or 0)
	else
		frame:SetPoint("CENTER", UIParent, "CENTER", 0, 0)
	end
end

local function BuildTabs()
	ui.tabs = {}
	for i, tab in ipairs(TABS) do
		local button = CreateFrame("Button", "GnomishRelayTab" .. i, frame)
		button:SetSize(TAB_WIDTH, 28)
		button:SetPoint("TOPLEFT", frame, "TOPRIGHT", 0, -70 - (i - 1) * 32)
		button.bg = button:CreateTexture(nil, "BACKGROUND")
		button.bg:SetAllPoints()
		button.label = button:CreateFontString(nil, "OVERLAY", "GameFontNormal")
		button.label:SetPoint("CENTER", button, "CENTER", 0, 0)
		button.label:SetText(tab.name)
		button.id = tab.id
		button:SetScript("OnClick", function(self)
			Window.ShowTab(self.id)
		end)
		ui.tabs[i] = button
	end
end

local function BuildPages()
	local left = SIDE + 14
	local width = WIDTH - SIDE - 20
	ui.settings = Inset(frame, left, -60, width, 16)
	ui.diag = Inset(frame, left, -60, width, 16)
	ns.SettingsTab.Build(ui.settings)
	ns.DiagTab.Build(ui.diag)
	ui.settings:Hide()
	ui.diag:Hide()
end

local function Build()
	frame = CreateFrame("Frame", "GnomishRelayFrame", UIParent, "PortraitFrameTemplate")
	frame:SetSize(WIDTH, HEIGHT)
	PlaceFrame()
	frame:SetMovable(true)
	frame:EnableMouse(true)
	frame:SetClampedToScreen(true)
	-- The side tabs hang out of the right edge, and the clamp keeps them on screen too.
	frame:SetClampRectInsets(0, TAB_WIDTH, 0, 0)
	frame:RegisterForDrag("LeftButton")
	frame:SetScript("OnDragStart", frame.StartMoving)
	frame:SetScript("OnDragStop", SavePosition)
	frame:SetScript("OnShow", Window.Refresh)
	table.insert(UISpecialFrames, "GnomishRelayFrame")

	if frame.SetTitle then
		frame:SetTitle("Gnomish Relay")
	end
	local portrait = frame.PortraitContainer and frame.PortraitContainer.portrait or frame.portrait
	if portrait then
		portrait:SetTexture(EMBLEM)
	end

	ui.chats = Inset(frame, 6, -60, SIDE, 30, "GnomishRelayChats")
	ui.chats:EnableMouseWheel(true)
	ui.chats:SetScript("OnMouseWheel", function(_, delta)
		ui.tileOffset = (ui.tileOffset or 0) - delta
		Window.Refresh()
	end)
	BuildCenter()
	BuildActivity()
	BuildConfirm()
	BuildTabs()
	BuildPages()
	ui.chatParts = { ui.agent, ui.folderButton, ui.bridge, ui.activity[1], ui.activity[2] }
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
		ui.tab = "chats"
		ui.picking = false
		ns.Browser.Close()
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
