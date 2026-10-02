-- Mini chats (SPEC.md 13.1, "Mini chats"): a chat popped out of the window into its own
-- small window. A mini chat draws the saved history of its chat with the transcript of
-- the window, so both show one message list.

local _, ns = ...

local MiniChat = {}
ns.MiniChat = MiniChat

local MAX = 4
local FULL = "You can pop out up to 4 chats. Close one first."
local WIDTH, HEIGHT = 340, 270
local MIN_WIDTH, MIN_HEIGHT = 260, 160
local PAD = 6
local HEADER = 26
local INPUT_BOTTOM = 8
local INPUT_HEIGHT = 24
local MODE_HEIGHT = 16
-- The mode line sits on the input, and the desktop box and the log sit on the mode line.
local MODE_BOTTOM = INPUT_BOTTOM + INPUT_HEIGHT + 2
local LOG_BOTTOM = MODE_BOTTOM + MODE_HEIGHT + 2
local MAX_INPUT = 3200
local BODY_FONT = "Fonts\\ARIALN.TTF"
local GRIP = "Interface\\ChatFrame\\UI-ChatIM-SizeGrabber-"
local BACK = "Interface\\Buttons\\UI-SpellbookIcon-PrevPage-"
local ORANGE = "ff9f40"
local GREY = "9d9d9d"
-- The fonts of the game have no "⏵", so the mode line uses ">" as its mark.
local MODES = {
	ask = { mark = ">", color = "9a9a9a" },
	["auto-edit"] = { mark = ">", color = "c8b98a" },
	["full-auto"] = { mark = ">>", color = ns.LevelMenu.WARNING },
}
local BAR = { 0.1, 0.08, 0.05, 0.95 }
local UNREAD_BAR = { 0.35, 0.27, 0.06, 0.95 }

-- One mini for each place, 1 to MAX. A closed mini keeps its frames for the next chat,
-- because the game never frees a frame.
local minis = {}

-- The chat of an open mini, or nil when the chat is gone or back in the window.
local function ChatOf(mini)
	local chat = mini.chatId and ns.Store.Chat(mini.chatId)
	if chat and chat.poppedOut then
		return chat
	end
end

local function Find(chatId)
	for _, mini in ipairs(minis) do
		if mini.chatId == chatId then
			return mini
		end
	end
end

local function OpenCount()
	local count = 0
	for _, chat in ipairs(ns.Store.Chats()) do
		if chat.poppedOut then
			count = count + 1
		end
	end
	return count
end

function MiniChat.IsOpen(chatId)
	local chat = ns.Store.Chat(chatId)
	return chat ~= nil and chat.poppedOut == true
end

function MiniChat.Full()
	return OpenCount() >= MAX
end

function MiniChat.ShowFullTooltip(owner)
	if not MiniChat.Full() then
		return
	end
	GameTooltip:SetOwner(owner, "ANCHOR_TOP")
	GameTooltip:SetText(FULL, 1, 1, 1, 1, true)
	GameTooltip:Show()
end

local function HideTooltip()
	GameTooltip:Hide()
end

local function ShowTooltip(owner)
	if owner.tip and owner.tip ~= "" then
		GameTooltip:SetOwner(owner, "ANCHOR_TOP")
		GameTooltip:SetText(owner.tip, 1, 1, 1, 1, true)
		GameTooltip:Show()
	end
end

local function Tip(button, text)
	button.tip = text
	button:SetScript("OnEnter", ShowTooltip)
	button:SetScript("OnLeave", HideTooltip)
end

-- The player read the new reply: the gold bar and the "!" of the tile go.
local function MarkRead(mini)
	local chat = ChatOf(mini)
	if chat and chat.unread then
		chat.unread = nil
		ns.Window.Refresh()
	end
end

-- Places

local function LargestSize()
	return math.max(MIN_WIDTH, UIParent:GetWidth()), math.max(MIN_HEIGHT, UIParent:GetHeight())
end

-- Two columns at the right of the screen, as in the design.
local function DefaultPlace(index)
	local x = -24 - math.floor((index - 1) / 2) * (WIDTH + 24)
	local y = -96 - ((index - 1) % 2) * (HEIGHT + 26)
	return { point = "TOPRIGHT", relative = "TOPRIGHT", x = x, y = y }
end

-- A saved size larger than the screen opens at the size of the screen.
local function SavedSize(place)
	local largestWidth, largestHeight = LargestSize()
	local width = type(place.width) == "number" and place.width or WIDTH
	local height = type(place.height) == "number" and place.height or HEIGHT
	return math.min(largestWidth, math.max(MIN_WIDTH, width)), math.min(largestHeight, math.max(MIN_HEIGHT, height))
end

local function Place(mini, chat)
	local place = type(chat.mini) == "table" and type(chat.mini.point) == "string" and chat.mini
		or DefaultPlace(mini.index)
	local frame = mini.frame
	frame:ClearAllPoints()
	frame:SetPoint(place.point, UIParent, place.relative or place.point, place.x or 0, place.y or 0)
	frame:SetSize(SavedSize(type(chat.mini) == "table" and chat.mini or {}))
end

-- The saved variables keep the place and the size of each chat, so a mini opens where
-- the player left it, also after a /reload.
local function SavePlace(mini)
	local frame = mini.frame
	frame:StopMovingOrSizing()
	frame:SetUserPlaced(false)
	local chat = ChatOf(mini)
	local point, _, relative, x, y = frame:GetPoint()
	if chat and point then
		chat.mini = {
			point = point,
			relative = relative,
			x = x,
			y = y,
			width = frame:GetWidth(),
			height = frame:GetHeight(),
		}
	end
end

-- Drawing

local function LogSize(mini)
	local frame = mini.frame
	return frame:GetWidth() - 2 * PAD - 16, frame:GetHeight() - HEADER - 2 - mini.logBottom - 12
end

local function FitLog(mini, bottom)
	mini.logBottom = bottom
	mini.log:SetPoint("BOTTOMRIGHT", mini.frame, "BOTTOMRIGHT", -PAD, bottom)
	mini.view:Resize(LogSize(mini))
end

local function ModeText(chat)
	local mode = chat.level or chat.mode
	local look = MODES[mode] or MODES.ask
	return string.format("|cff%s%s %s|r", look.color, look.mark, ns.Relay.Plain(mode))
end

-- "Working..." says that a long run still goes. The hint shows only while the player types.
local function StateText(chat, typing)
	local parts = {}
	if ns.Transport.Working(chat.id) then
		table.insert(parts, "Working...")
	end
	if typing then
		table.insert(parts, "Shift+Tab to change")
	end
	return string.format("|cff%s%s|r", GREY, table.concat(parts, " · "))
end

local function DrawHeader(mini, chat)
	local bar = chat.unread and UNREAD_BAR or BAR
	mini.bar:SetColorTexture(bar[1], bar[2], bar[3], bar[4])
	mini.dot:SetShown(chat.unread == true)
	mini.title:SetText(ns.Relay.Plain(chat.name))
	mini.title:SetWidth(math.min(mini.title:GetUnboundedStringWidth(), mini.frame:GetWidth() / 2 - 40))
	mini.agent:SetText("· " .. ns.Relay.AgentName(chat.agent))
	local folder = ns.Relay.Plain(ns.Folders.Display(ns.Folders.Tree(), chat.cwd))
	mini.folder:SetText(folder)
	mini.folderTip.tip = folder
	local waits = ns.Transport.WaitsForAnswer(chat.id)
	mini.mark:SetText(waits and ("|cff" .. ORANGE .. "?|r") or "")
	local light = ns.Window.BridgeLight()
	mini.light:SetColorTexture(light[2][1], light[2][2], light[2][3], 1)
	mini.lightTip.tip = light[3]
end

local function Draw(mini, chat)
	DrawHeader(mini, chat)
	mini.view:Show(chat)
	mini.desktop:Refresh(chat)
	local room = mini.desktop:Place(PAD, LOG_BOTTOM)
	if LOG_BOTTOM + room ~= mini.logBottom then
		FitLog(mini, LOG_BOTTOM + room)
	end
	mini.mode:SetText(ModeText(chat))
	mini.state:SetText(StateText(chat, mini.input:HasFocus()))
	mini.input:SetFont(BODY_FONT, ns.Store.db.fontSize, "")
	mini.hint:SetShown((mini.input:GetText() or "") == "" and not mini.input:HasFocus())
end

-- Building

local function BuildBorder(mini)
	local frame = mini.frame
	local background = frame:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.03, 0.03, 0.03, 0.94)
	local border = CreateFrame("Frame", nil, frame, "DialogBorderDarkTemplate")
	border:SetAllPoints()
end

local function BuildButtons(mini, prefix)
	local frame = mini.frame
	local close = CreateFrame("Button", prefix .. "Close", frame, "UIPanelCloseButton")
	close:SetSize(24, 24)
	close:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -2, -2)
	close:SetScript("OnClick", function()
		MiniChat.Close(mini.chatId)
	end)
	Tip(close, "Close")
	local back = CreateFrame("Button", prefix .. "Back", frame)
	back:SetSize(20, 20)
	back:SetPoint("RIGHT", close, "LEFT", 0, 0)
	back:SetNormalTexture(BACK .. "Up")
	back:SetPushedTexture(BACK .. "Down")
	back:SetHighlightTexture("Interface\\Buttons\\UI-Common-MouseHilight", "ADD")
	back:SetScript("OnClick", function()
		MiniChat.Dock(mini.chatId)
	end)
	Tip(back, "Open in main window")
	mini.light = frame:CreateTexture(prefix .. "Light", "OVERLAY")
	mini.light:SetSize(8, 8)
	mini.light:SetPoint("RIGHT", back, "LEFT", -6, 0)
	mini.lightTip = CreateFrame("Button", nil, frame)
	mini.lightTip:SetAllPoints(mini.light)
	Tip(mini.lightTip, "")
end

local function BuildHeader(mini, prefix)
	local frame = mini.frame
	mini.bar = frame:CreateTexture(prefix .. "Bar", "BORDER")
	mini.bar:SetPoint("TOPLEFT", frame, "TOPLEFT", 4, -4)
	mini.bar:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -4, -4)
	mini.bar:SetHeight(HEADER - 4)
	mini.dot = frame:CreateTexture(prefix .. "Dot", "OVERLAY")
	mini.dot:SetSize(8, 8)
	mini.dot:SetPoint("TOPLEFT", frame, "TOPLEFT", 10, -11)
	mini.dot:SetColorTexture(1, 0.82, 0, 1)
	mini.title = frame:CreateFontString(prefix .. "Title", "OVERLAY", "GameFontNormal")
	mini.title:SetPoint("LEFT", mini.dot, "RIGHT", 6, 0)
	mini.title:SetWordWrap(false)
	mini.agent = frame:CreateFontString(prefix .. "Agent", "OVERLAY", "GameFontDisableSmall")
	mini.agent:SetPoint("LEFT", mini.title, "RIGHT", 4, 0)
	BuildButtons(mini, prefix)
	mini.mark = frame:CreateFontString(prefix .. "Mark", "OVERLAY", "GameFontNormalLarge")
	mini.mark:SetPoint("RIGHT", mini.light, "LEFT", -6, 0)
	mini.folder = frame:CreateFontString(prefix .. "Folder", "OVERLAY", "GameFontDisableSmall")
	mini.folder:SetPoint("LEFT", mini.agent, "RIGHT", 6, 0)
	mini.folder:SetPoint("RIGHT", mini.mark, "LEFT", -4, 0)
	mini.folder:SetJustifyH("LEFT")
	mini.folder:SetWordWrap(false)
	mini.folderTip = CreateFrame("Button", nil, frame)
	mini.folderTip:SetAllPoints(mini.folder)
	Tip(mini.folderTip, "")
end

local function BuildLog(mini, prefix)
	local frame = mini.frame
	mini.log = CreateFrame("Frame", prefix .. "Log", frame)
	mini.log:SetPoint("TOPLEFT", frame, "TOPLEFT", PAD, -HEADER - 2)
	local background = mini.log:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0, 0, 0, 0.6)
	mini.logBottom = LOG_BOTTOM
	mini.log:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -PAD, LOG_BOTTOM)
	local width, height = LogSize(mini)
	mini.view = ns.Transcript.New(mini.log, width, height, prefix)
	mini.desktop = ns.DesktopRequest.New(frame, prefix, true)
end

local function Selected(mini)
	return function()
		return ChatOf(mini)
	end
end

local function RefreshAll()
	ns.Window.Refresh()
end

local function BuildMode(mini, prefix)
	local frame = mini.frame
	mini.mode = frame:CreateFontString(prefix .. "Mode", "OVERLAY", "GameFontNormalSmall")
	mini.mode:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", PAD + 4, MODE_BOTTOM)
	mini.state = frame:CreateFontString(prefix .. "State", "OVERLAY", "GameFontDisableSmall")
	mini.state:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -PAD - 4, MODE_BOTTOM)
	ns.LevelMenu.Build(frame, mini.mode, Selected(mini), RefreshAll, prefix)
end

local function Send(mini, input)
	local chat = ChatOf(mini)
	local text = strtrim(input:GetText() or "")
	if text == "" or not chat then
		input:ClearFocus()
	elseif ns.Window.SendTo(chat, text) then
		-- The game gets its keys back, so a move key after a send moves the player.
		input:SetText("")
		input:ClearFocus()
	end
end

local function Recall(mini, input, key)
	local chat = ChatOf(mini)
	if not chat then
		return
	end
	local text
	if key == "UP" then
		text = ns.InputHistory.Older(chat, input:GetText() or "")
	elseif key == "DOWN" then
		text = ns.InputHistory.Newer(chat)
	end
	if text then
		input:SetText(text)
		input:SetCursorPosition(#text)
	end
end

local function RefreshInput(mini)
	local chat = ChatOf(mini)
	if chat then
		mini.state:SetText(StateText(chat, mini.input:HasFocus()))
	end
	mini.hint:SetShown((mini.input:GetText() or "") == "" and not mini.input:HasFocus())
end

local function BuildInput(mini, prefix)
	local frame = mini.frame
	local input = CreateFrame("EditBox", prefix .. "Input", frame, "InputBoxTemplate")
	input:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", PAD + 8, INPUT_BOTTOM)
	input:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -20, INPUT_BOTTOM)
	input:SetHeight(INPUT_HEIGHT)
	input:SetAutoFocus(false)
	input:SetMaxBytes(MAX_INPUT)
	input:SetFont(BODY_FONT, ns.Store.db.fontSize, "")
	input:SetScript("OnEnterPressed", function(self)
		Send(mini, self)
	end)
	input:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
	end)
	input:SetAltArrowKeyMode(false)
	input:SetScript("OnArrowPressed", function(self, key)
		Recall(mini, self, key)
	end)
	input:SetScript("OnEditFocusGained", function()
		MarkRead(mini)
		RefreshInput(mini)
	end)
	input:SetScript("OnEditFocusLost", function()
		RefreshInput(mini)
	end)
	input:SetScript("OnTextChanged", function()
		RefreshInput(mini)
	end)
	ns.LevelMenu.Bind(input, Selected(mini), RefreshAll)
	mini.hint = input:CreateFontString(prefix .. "Hint", "OVERLAY", "GameFontDisable")
	mini.hint:SetPoint("LEFT", input, "LEFT", 2, 0)
	mini.hint:SetText("Type a message, then press Enter.")
	mini.input = input
end

local function EndSizing(mini)
	SavePlace(mini)
	mini.view:Resize(LogSize(mini))
	local chat = ChatOf(mini)
	if chat then
		Draw(mini, chat)
	end
end

local function BuildGrip(mini, prefix)
	local frame = mini.frame
	frame:SetResizable(true)
	local largestWidth, largestHeight = LargestSize()
	frame:SetResizeBounds(MIN_WIDTH, MIN_HEIGHT, largestWidth, largestHeight)
	local grip = CreateFrame("Button", prefix .. "Grip", frame)
	grip:SetSize(16, 16)
	grip:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -2, 2)
	grip:SetNormalTexture(GRIP .. "Up")
	grip:SetHighlightTexture(GRIP .. "Highlight")
	grip:SetPushedTexture(GRIP .. "Down")
	grip:SetScript("OnMouseDown", function()
		frame:StartSizing("BOTTOMRIGHT")
	end)
	grip:SetScript("OnMouseUp", function()
		EndSizing(mini)
	end)
end

-- A plain frame on UIParent: nothing in it is protected, so it works in combat, and
-- Alt+Z hides it with the rest of the UI. Escape never closes it: players press Escape
-- all the time in a fight.
local function Build(index)
	local prefix = "GnomishRelayMini" .. index
	local mini = { index = index }
	local frame = CreateFrame("Frame", prefix, UIParent)
	frame:SetSize(WIDTH, HEIGHT)
	frame:SetFrameStrata("MEDIUM")
	frame:SetToplevel(true)
	frame:SetMovable(true)
	frame:EnableMouse(true)
	frame:SetClampedToScreen(true)
	frame:RegisterForDrag("LeftButton")
	frame:SetScript("OnDragStart", frame.StartMoving)
	frame:SetScript("OnDragStop", function()
		SavePlace(mini)
	end)
	frame:SetScript("OnMouseDown", function()
		MarkRead(mini)
	end)
	mini.frame = frame
	BuildBorder(mini)
	BuildHeader(mini, prefix)
	BuildLog(mini, prefix)
	BuildMode(mini, prefix)
	BuildInput(mini, prefix)
	BuildGrip(mini, prefix)
	frame:Hide()
	return mini
end

-- The free mini with the lowest place, or a new one.
local function FreeMini()
	for _, mini in ipairs(minis) do
		if not mini.chatId then
			return mini
		end
	end
	if #minis < MAX then
		table.insert(minis, Build(#minis + 1))
		return minis[#minis]
	end
end

local function Attach(chat)
	local mini = FreeMini()
	if not mini then
		-- Saved data from elsewhere can hold more than MAX: the rest stay in the window.
		chat.poppedOut = nil
		return
	end
	mini.chatId = chat.id
	mini.input:SetText("")
	Place(mini, chat)
	mini.frame:Show()
	mini.view:Resize(LogSize(mini))
end

local function Release(mini)
	mini.chatId = nil
	mini.input:ClearFocus()
	mini.frame:Hide()
end

-- Opens a mini for each popped-out chat, closes the mini of a chat that went back or
-- was deleted, and draws each open one.
function MiniChat.Refresh()
	-- With no key, nothing can send, and the first-run window shows instead.
	if not ns.key then
		return
	end
	for _, mini in ipairs(minis) do
		if mini.chatId and not ChatOf(mini) then
			Release(mini)
		end
	end
	for _, chat in ipairs(ns.Store.Chats()) do
		if chat.poppedOut and not Find(chat.id) then
			Attach(chat)
		end
	end
	for _, mini in ipairs(minis) do
		local chat = ChatOf(mini)
		if chat then
			Draw(mini, chat)
		end
	end
end

function MiniChat.PopOut(chatId)
	local chat = ns.Store.Chat(chatId)
	if not chat or chat.poppedOut then
		return
	end
	if MiniChat.Full() then
		UIErrorsFrame:AddMessage(FULL, 1, 0.1, 0.1)
		return
	end
	-- The search and the earlier messages belong to the chat of the window, which changes.
	if chat == ns.Window.SelectedChat() then
		ns.Search.Close()
		ns.InputHistory.Reset()
	end
	chat.poppedOut = true
	ns.Window.Refresh()
end

-- The chat goes back to its tile, and the window stays as it is.
function MiniChat.Close(chatId)
	local chat = chatId and ns.Store.Chat(chatId)
	if chat then
		chat.poppedOut = nil
	end
	ns.Window.Refresh()
end

-- The chat goes back to the window, and the window opens on it.
function MiniChat.Dock(chatId)
	MiniChat.Close(chatId)
	ns.Window.Open(chatId)
end

-- The player asked for this chat, so its mini comes to the front with the focus.
function MiniChat.Show(chatId)
	local mini = Find(chatId)
	if not mini then
		return
	end
	mini.frame:Raise()
	mini.input:SetFocus()
end

-- Resend after a /reload: the text goes into the box of the mini, and Enter sends it.
function MiniChat.Fill(chatId, text)
	local mini = Find(chatId)
	if mini then
		mini.input:SetText(text)
		mini.input:SetFocus()
	end
end

-- The player types in the mini of this chat, so a new reply there is already read.
function MiniChat.Reading(chatId)
	local mini = Find(chatId)
	return mini ~= nil and mini.input:HasFocus()
end

-- A new resolution or UI scale can leave a mini off screen, so each one goes back to
-- its place, and the clamp keeps it on screen.
local events = CreateFrame("Frame")
events:RegisterEvent("DISPLAY_SIZE_CHANGED")
events:RegisterEvent("UI_SCALE_CHANGED")
events:SetScript("OnEvent", function()
	for _, mini in ipairs(minis) do
		local chat = ChatOf(mini)
		if chat then
			Place(mini, chat)
		end
	end
end)
