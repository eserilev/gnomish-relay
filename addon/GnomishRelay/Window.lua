-- The chat window, in the layout of the Guild & Communities frame (SPEC.md 13.1).

local _, ns = ...

local Window = {}
ns.Window = Window

local WIDTH, HEIGHT = 900, 560
local SIDE = 200
local TILE_HEIGHT = 48
local STEP_ROWS = 14
local MAX_INPUT = 3000
local EMBLEM = "Interface\\Icons\\INV_Misc_Wrench_01"
local STATUS_BAR = "Interface\\TargetingFrame\\UI-StatusBar"
local YOU = "69ccf0"
local CODE = "b8c8b8"

local frame
local tiles = {}
local ui = {}

local function Selected()
	local db = ns.Store.db
	local chat = db.selected and ns.Store.Chat(db.selected)
	if not chat and #db.chats > 0 then
		chat = db.chats[1]
		db.selected = chat.id
	end
	return chat
end

local function Select(chatId)
	local chat = ns.Store.Chat(chatId)
	if chat then
		ns.Store.db.selected = chat.id
		chat.unread = nil
	end
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
	tile = CreateFrame("Button", nil, ui.chats)
	tile:SetSize(SIDE - 12, TILE_HEIGHT)
	tile:SetPoint("TOPLEFT", ui.chats, "TOPLEFT", 6, -6 - (index - 1) * (TILE_HEIGHT + 4))
	tile.bg = tile:CreateTexture(nil, "BACKGROUND")
	tile.bg:SetAllPoints()
	tile:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	tile.name = Label(tile, "GameFontNormal", "TOPLEFT", 10, -9)
	tile.agent = Label(tile, "GameFontHighlightSmall", "BOTTOMLEFT", 10, 9)
	tile.mark = Label(tile, "GameFontNormalLarge", "RIGHT", -10, 0)
	tile:SetScript("OnClick", function(self)
		if self.chatId then
			Select(self.chatId)
		else
			Select(ns.Store.NewChat().id)
		end
	end)
	tiles[index] = tile
	return tile
end

local function ShowTile(index, chat, selected)
	local tile = Tile(index)
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
		ShowTile(i, chat, current and chat.id == current.id)
	end
	ShowTile(#chats + 1, nil, false)
	for i = #chats + 2, #tiles do
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
		if entry.role == "user" then
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

function Window.Refresh()
	if not frame or not frame:IsShown() then
		return
	end
	local chat = Selected()
	RefreshTiles(chat)
	RefreshTranscript(chat)
	RefreshActivity(chat)
	RefreshStatus(chat)
end

-- Returns false, and shows an error, for a message that does not fit in one strip.
function Window.Send(text)
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

	ui.stop = CreateFrame("Button", nil, frame, "UIPanelButtonTemplate")
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
		SetPortraitToTexture(portrait, EMBLEM)
	end

	ui.chats = Inset(frame, 6, -60, SIDE, 30)
	BuildCenter()
	BuildActivity()
	frame:Hide()
end

function Window.Showing(chatId)
	return frame ~= nil and frame:IsShown() and ns.Store.db.selected == chatId
end

function Window.Open(chatId)
	if not frame then
		Build()
	end
	if chatId then
		ns.Store.db.selected = chatId
		local chat = ns.Store.Chat(chatId)
		if chat then
			chat.unread = nil
		end
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
