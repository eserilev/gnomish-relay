-- The chat window, in the layout of the Guild & Communities frame (SPEC.md 13.1).

local _, ns = ...

local Window = {}
ns.Window = Window

-- The least size. The grip makes the window bigger, up to the size of the screen.
local WIDTH, HEIGHT = 900, 560
local GRIP = "Interface\\ChatFrame\\UI-ChatIM-SizeGrabber-"
local SIDE = 200
local TILE_HEIGHT = 48
local TAB_WIDTH = 74
local STEP_ROWS = 14
-- The cast bar text changes at most this often, in seconds.
local CAST_UPDATE = 0.2
local PICK_ROWS = 20
local PICK_ROW_HEIGHT = 19
local GREEN = "1eff00"
local ORANGE = "ff9f40"
local GREY = "9d9d9d"
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
-- The room at the right end of the header of a chat, for Pinned and Search.
local HEADER_RIGHT = 140
-- The bottom of the transcript: above the input, or above the row over the input.
local LOG_BOTTOM, LOG_BOTTOM_ROW = 44, 72
-- Pings has no content yet, so it has no tab (SPEC.md 13.1).
local TABS =
	{ { id = "chats", name = "Chats" }, { id = "settings", name = "Settings" }, { id = "diag", name = "Diag" } }

local frame
local tiles = {}
local ui = { tab = "chats" }

local function CenterWidth()
	return frame:GetWidth() - 2 * SIDE - 28
end

-- The selected chat, or the first chat when none is selected.
local function Selected()
	local db = ns.Store.db
	return db.selected and ns.Store.Chat(db.selected) or db.chats[1]
end

local function MarkSelected(chatId)
	local chat = ns.Store.Chat(chatId)
	if chat then
		-- A search finds text in one chat, so another chat closes it.
		if chat ~= Selected() then
			ns.Search.Close()
		end
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

-- The inset keeps `right` from the right edge of the window, so it grows with the window.
local function Stretch(inset, right, top)
	inset:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -right, top)
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
		if ns.Transport.WaitsForAnswer(chat.id) then
			mark = "|cff" .. ORANGE .. "?|r"
		elseif chat.unread then
			mark = "!"
		elseif ns.Transport.Working(chat.id) then
			mark = "..."
		end
		tile.mark:SetText(mark)
	else
		tile.name:SetText("|cff1eff00New chat|r")
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
	-- A run that waits for the player or for other chats makes no progress, so the bar
	-- stands still.
	local queued = ns.Transport.Queued(chat.id)
	if queued then
		ui.cast:SetStatusBarColor(0.3, 0.3, 0.3)
		ui.cast:SetValue(1)
		ui.cast.text:SetText("|cff" .. GREY .. ns.Relay.Plain(queued) .. "|r")
	elseif ns.Transport.WaitsForAnswer(chat.id) then
		ui.cast:SetStatusBarColor(0.3, 0.3, 0.3)
		ui.cast:SetValue(1)
		ui.cast.text:SetText("|cff" .. ORANGE .. "Waiting for your approval|r")
	else
		local elapsed = GetTime() - working.since
		ui.cast:SetStatusBarColor(1, 0.7, 0)
		ui.cast:SetValue(elapsed % 10 / 10)
		ui.cast.text:SetText("Tinkering " .. Elapsed(elapsed))
	end
	ui.nextCheck:SetText(string.format("Checking again in %ds", math.ceil(ns.Transport.NextPollIn())))
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
		return "1 message is waiting. Reload to send it."
	elseif waiting > 1 then
		return string.format("%d messages are waiting. Reload to send them.", waiting)
	end
	return "Reload soon to keep chatting."
end

local function RefreshStatus(chat)
	if chat then
		ui.agent:SetText(ns.Relay.AgentName(chat.agent) .. " · " .. ns.LevelMenu.Text(chat.level or chat.mode))
		local folder = ns.Relay.Plain(ns.Folders.Display(ns.Folders.Tree(), chat.cwd))
		if chat.newFolder then
			folder = folder .. " |cff" .. NEW .. "new|r"
		end
		ui.folder:SetText(folder)
		-- A long folder cuts its name, so the header keeps room for Pinned and Search.
		local room = CenterWidth() - 170 - HEADER_RIGHT - 40
		ui.folder:SetWidth(math.min(ui.folder:GetUnboundedStringWidth(), room))
		ui.folderButton:SetWidth(ui.folder:GetWidth() + 40)
	else
		ui.agent:SetText("")
	end
	ui.folderButton:SetShown(chat ~= nil)
	-- The player asked for the search, so its bar comes before the banner.
	ui.banner:SetShown(ns.Transport.NeedsReload() and not ns.Search.IsOpen())
	ui.bannerText:SetText(BannerText(#ns.Store.db.outbox))
end

-- The color of the text, the color of the dot, and the text of each state of the bridge.
local LIGHTS = {
	checking = { "9d9d9d", { 0.6, 0.6, 0.6 }, "Connecting..." },
	online = { "1eff00", { 0.1, 1, 0 }, "Connected" },
	slow = { "ffb000", { 1, 0.7, 0 }, "Slow connection" },
	offline = { "ff2020", { 1, 0.1, 0.1 }, "Desktop app offline" },
	missing = { "ff2020", { 1, 0.1, 0.1 }, "Addon files missing" },
	blocked = { "ff2020", { 1, 0.1, 0.1 }, "Screenshots blocked" },
	mismatch = { "ff2020", { 1, 0.1, 0.1 }, "Update needed" },
}

-- The light is in the title bar, so every tab shows it.
local function RefreshBridge()
	local light = LIGHTS[ns.Transport.Problem() or ns.Transport.Bridge()]
	ui.bridge:SetText(string.format("|cff%s%s|r", light[1], light[3]))
	ui.bridgeDot:SetColorTexture(light[2][1], light[2][2], light[2][3], 1)
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
		note = "No sessions to resume."
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
	-- The editor of the quick actions closes with the Settings page.
	if ui.tab ~= "settings" then
		ns.QuickEditor.Close()
	end
	local editing = ns.QuickEditor.IsOpen()
	ui.settings:SetShown(ui.tab == "settings" and not editing)
	ui.quickEdit:SetShown(editing)
	ns.QuickEditor.Refresh()
	ui.diag:SetShown(ui.tab == "diag")
	ns.SettingsTab.Refresh()
	ns.DiagTab.Refresh()
end

-- The transcript is the center inset less 8 at each side and 6 at the top and bottom.
local function TranscriptSize()
	return CenterWidth() - 16, frame:GetHeight() - 84 - ui.logBottom - 12
end

local function FitTranscript(bottom)
	if bottom == ui.logBottom then
		return
	end
	ui.logBottom = bottom
	ui.log:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", SIDE + 14, bottom)
	ns.Transcript.Resize(TranscriptSize())
end

-- The search bar, the banner, and the byte counter share the row above the input. While
-- none of them shows, the transcript takes the row.
local function RefreshInputRow()
	local chats = ui.tab == "chats" and not ui.picking
	ns.Search.Show(chats)
	local rowUsed = ns.Search.IsOpen() or ui.banner:IsShown() or ui.count:IsShown()
	FitTranscript(rowUsed and LOG_BOTTOM_ROW or LOG_BOTTOM)
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
		ns.Suggestions.Refresh(chat)
	end
	RefreshActivity(not ui.picking and chat or nil)
	RefreshStatus(not ui.picking and chat or nil)
	RefreshInputRow()
	ui.pinned:SetShown(not ui.picking)
	ui.searchButton:SetShown(not ui.picking)
	ns.Pins.Refresh()
	ns.GitBar.Refresh(not ui.picking and not browsing and chat or nil)
end

function Window.Refresh()
	if not frame or not frame:IsShown() then
		return
	end
	local chat = Selected()
	RefreshBridge()
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
		RefreshInputRow()
		ns.GitBar.Refresh(nil)
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

-- The default folder often holds all the projects, so a new chat asks for its folder
-- first (SPEC.md 9.9). Escape keeps the default folder.
function Window.NewChat()
	local chat = ns.Store.NewChat()
	-- The new tile is at the end of the column. RefreshTiles clamps the offset.
	ui.tileOffset = math.huge
	MarkSelected(chat.id)
	ui.tab = "chats"
	ui.picking = false
	ns.Browser.Open(chat)
	Window.Refresh()
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
		UIErrorsFrame:AddMessage("Too long to send. Try a shorter message.", 1, 0.1, 0.1)
		return false
	end
	Window.Refresh()
	return true
end

-- After a /reload only the saved variables hold the text, and those are never signed
-- again (SPEC.md 6.6.1). So the text goes into the input, and Enter sends it.
function Window.Resend(message)
	local text = ns.Messages.PrivateText(message.id)
	if text then
		Window.Send(text)
		return
	end
	if ui.input then
		ui.input:SetText(message.text)
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
	ui.folder:SetWordWrap(false)
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
		ui.count:SetText(string.format("|cffff2020%d over the limit|r", -left))
	elseif left then
		ui.count:SetText(string.format("%d left", left))
	end
	RefreshInputRow()
end

local function BuildInputHelp()
	ui.hint = ui.input:CreateFontString("GnomishRelayInputHint", "OVERLAY", "GameFontDisable")
	ui.hint:SetPoint("LEFT", ui.input, "LEFT", 2, 0)
	ui.hint:SetText("Type a message, then press Enter.")
	ui.count = ui.input:CreateFontString("GnomishRelayInputCount", "OVERLAY", "GameFontDisableSmall")
	ui.count:SetPoint("BOTTOMRIGHT", ui.input, "TOPRIGHT", 0, 2)
	ui.count:Hide()
	ui.input:SetScript("OnTextChanged", RefreshInputHelp)
	ui.input:SetScript("OnEditFocusGained", RefreshInputHelp)
	ui.input:SetScript("OnEditFocusLost", RefreshInputHelp)
end

-- The Settings and Diag pages cover the window right of the chat column.
local function PageSize()
	return frame:GetWidth() - SIDE - 20, frame:GetHeight() - 60 - 16
end

local function BuildCenter()
	local left = SIDE + 14
	local width = WIDTH - 2 * SIDE - 28

	ui.agent = Label(frame, "GameFontNormal", "TOPLEFT", left, -64)
	ui.levelButton = ns.LevelMenu.Build(frame, ui.agent, Selected, function()
		Window.Refresh()
	end)
	BuildFolderButton(left + 170)
	ui.pinned = ns.Pins.Build(frame, left, -61)
	ui.searchButton = ns.Search.Build(frame, left + 6, 46, ui.pinned)

	ui.logBottom = LOG_BOTTOM
	local log = Inset(frame, left, -84, width, LOG_BOTTOM)
	Stretch(log, left, -84)
	ui.log = log
	ns.Transcript.Build(log, TranscriptSize())
	ns.Suggestions.Build(log, CenterWidth() - 16)
	ns.GitBar.Build(frame, left, -34)

	ui.picker = Inset(frame, left, -84, width, 16)
	Stretch(ui.picker, left, -84)
	ui.pickNote = ui.picker:CreateFontString("GnomishRelayPickNote", "OVERLAY", "GameFontDisable")
	ui.pickNote:SetPoint("TOPLEFT", ui.picker, "TOPLEFT", 12, -12)
	ui.pickRows = PickRows(ui.picker, "GnomishRelayPick", width, Window.Resume)
	ui.picker:EnableMouseWheel(true)
	ui.picker:SetScript("OnMouseWheel", function(_, delta)
		ui.pickOffset = (ui.pickOffset or 0) - delta * 3
		RefreshPicker()
	end)
	ui.picker:Hide()

	ns.Browser.Build(frame, left, CenterWidth(), 72)

	ui.banner = CreateFrame("Frame", "GnomishRelayBanner", frame)
	ui.banner:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left, 44)
	ui.banner:SetSize(width, 24)
	ui.bannerText = ui.banner:CreateFontString("GnomishRelayBannerText", "OVERLAY", "GameFontNormal")
	ui.bannerText:SetPoint("LEFT", ui.banner, "LEFT", 6, 0)
	local reload = CreateFrame("Button", "GnomishRelayReload", ui.banner, "UIPanelButtonTemplate")
	reload:SetSize(90, 22)
	reload:SetPoint("RIGHT", ui.banner, "RIGHT", 0, 0)
	reload:SetText("Reload")
	reload:SetScript("OnClick", function()
		if InCombatLockdown() then
			UIErrorsFrame:AddMessage("Reload works after combat.", 1, 0.1, 0.1)
			return
		end
		ReloadUI()
	end)
	ui.banner:Hide()

	ui.input = CreateFrame("EditBox", "GnomishRelayInput", frame, "InputBoxTemplate")
	ui.input:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left + 6, 16)
	ui.input:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -left, 16)
	ui.input:SetHeight(24)
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
	ns.LevelMenu.Bind(ui.input)
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
	local panel = CreateFrame("Frame", nil, frame, "InsetFrameTemplate")
	panel:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -6, -84)
	panel:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -6, 44)
	panel:SetWidth(SIDE)
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
	-- A long waiting line gets cut with "...", so it never runs over the transcript.
	ui.cast.text = ui.cast:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	ui.cast.text:SetPoint("LEFT", ui.cast, "LEFT", 4, 0)
	ui.cast.text:SetPoint("RIGHT", ui.cast, "RIGHT", -4, 0)
	ui.cast.text:SetWordWrap(false)
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

-- The dialog of the game has a border, and Escape closes only the dialog.
StaticPopupDialogs.GNOMISHRELAY_DELETE = {
	text = "%s",
	button1 = "Delete",
	button2 = "Cancel",
	OnAccept = function(_, chatId)
		local chat = ns.Store.Chat(chatId)
		if chat then
			ns.Changes.Forget(chat)
			ns.Transport.Delete(chat)
		end
	end,
	timeout = 0,
	whileDead = true,
	hideOnEscape = true,
	preferredIndex = 3,
}

-- A chat that still works gets Stop and Delete in one click: the bridge stops the run.
function Window.AskDelete(chatId)
	local chat = ns.Store.Chat(chatId)
	if not chat then
		return
	end
	local name = ns.Relay.Plain(chat.name)
	local question = string.format('Delete "%s"?', name)
	if ns.Transport.Working(chat.id) then
		question = string.format('Stop and delete "%s"?', name)
	end
	StaticPopup_Show("GNOMISHRELAY_DELETE", question, nil, chat.id)
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
	ui.quickEdit = Inset(frame, left, -60, width, 16, "GnomishRelayQuickEdit")
	Stretch(ui.settings, 6, -60)
	Stretch(ui.diag, 6, -60)
	Stretch(ui.quickEdit, 6, -60)
	ns.SettingsTab.Build(ui.settings)
	ns.DiagTab.Build(ui.diag)
	ns.QuickEditor.Build(ui.quickEdit)
	ns.DiagTab.Resize(PageSize())
	ui.settings:Hide()
	ui.diag:Hide()
	ui.quickEdit:Hide()
end

local function BuildBridgeLight()
	ui.bridge = frame:CreateFontString("GnomishRelayBridgeText", "OVERLAY", "GameFontNormalSmall")
	ui.bridge:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -30, -6)
	ui.bridgeDot = frame:CreateTexture("GnomishRelayBridgeDot", "OVERLAY")
	ui.bridgeDot:SetSize(8, 8)
	ui.bridgeDot:SetPoint("RIGHT", ui.bridge, "LEFT", -4, 0)
end

local function LargestSize()
	return math.max(WIDTH, UIParent:GetWidth() - TAB_WIDTH), math.max(HEIGHT, UIParent:GetHeight())
end

-- A larger UI scale or a lower resolution since the save leaves the grip off screen, so
-- the size never passes the screen.
local function SavedSize()
	local saved = ns.Store.db.windowSize
	if type(saved) ~= "table" or type(saved.width) ~= "number" or type(saved.height) ~= "number" then
		return WIDTH, HEIGHT
	end
	local largestWidth, largestHeight = LargestSize()
	local width = math.min(largestWidth, math.max(WIDTH, saved.width))
	local height = math.min(largestHeight, math.max(HEIGHT, saved.height))
	return width, height
end

-- The transcript lays out its entries for one width, so a new size draws it again.
local function Resized()
	ns.Transcript.Resize(TranscriptSize())
	ns.Browser.Resize(CenterWidth())
	ns.Suggestions.Resize(CenterWidth() - 16)
	ns.DiagTab.Resize(PageSize())
	Window.Refresh()
end

local function EndSizing()
	SavePosition()
	ns.Store.db.windowSize = { width = frame:GetWidth(), height = frame:GetHeight() }
	Resized()
end

function Window.ResetPosition()
	ns.Store.db.windowPoint = nil
	ns.Store.db.windowSize = nil
	if frame then
		frame:ClearAllPoints()
		frame:SetPoint("CENTER", UIParent, "CENTER", 0, 0)
		frame:SetSize(WIDTH, HEIGHT)
		Resized()
	end
end

local function BuildGrip()
	frame:SetResizable(true)
	local largestWidth, largestHeight = LargestSize()
	frame:SetResizeBounds(WIDTH, HEIGHT, largestWidth, largestHeight)
	local grip = CreateFrame("Button", "GnomishRelayResizeGrip", frame)
	grip:SetSize(16, 16)
	grip:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -2, 2)
	grip:SetNormalTexture(GRIP .. "Up")
	grip:SetHighlightTexture(GRIP .. "Highlight")
	grip:SetPushedTexture(GRIP .. "Down")
	grip:SetScript("OnMouseDown", function()
		frame:StartSizing("BOTTOMRIGHT")
	end)
	grip:SetScript("OnMouseUp", EndSizing)
end

local function Build()
	frame = CreateFrame("Frame", "GnomishRelayFrame", UIParent, "PortraitFrameTemplate")
	frame:SetSize(SavedSize())
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
	-- The Commit dialog sits on UIParent, so it outlives the window unless we close it.
	frame:HookScript("OnHide", function()
		ns.Changes.CloseCommit()
	end)

	if frame.SetTitle then
		frame:SetTitle("Gnomish Relay")
	end
	local portrait = frame.PortraitContainer and frame.PortraitContainer.portrait or frame.portrait
	if portrait then
		portrait:SetTexture(EMBLEM)
	end

	BuildBridgeLight()
	BuildGrip()
	ui.chats = Inset(frame, 6, -60, SIDE, 30, "GnomishRelayChats")
	ui.chats:EnableMouseWheel(true)
	ui.chats:SetScript("OnMouseWheel", function(_, delta)
		ui.tileOffset = (ui.tileOffset or 0) - delta
		Window.Refresh()
	end)
	BuildCenter()
	BuildActivity()
	BuildTabs()
	BuildPages()
	ui.chatParts =
		{ ui.agent, ui.levelButton, ui.folderButton, ui.pinned, ui.searchButton, ui.activity[1], ui.activity[2] }
	frame:Hide()
end

function Window.Showing(chatId)
	local chat = Selected()
	return frame ~= nil and frame:IsShown() and chat ~= nil and chat.id == chatId
end

function Window.Open(chatId)
	if not ns.key then
		ns.SetupNeeded.Show()
		return
	end
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

-- The key binding "Search chat": the window on its chat, with the search open.
function Window.OpenSearch()
	Window.Open()
	-- With no key, Open shows the first-run window and builds nothing to search.
	if not frame or not frame:IsShown() then
		return
	end
	ui.tab = "chats"
	ui.picking = false
	ns.Browser.Close()
	ns.Search.Open()
end

function Window.Toggle()
	if not ns.key then
		ns.SetupNeeded.Toggle()
	elseif frame and frame:IsShown() then
		frame:Hide()
	else
		Window.Open()
	end
end
