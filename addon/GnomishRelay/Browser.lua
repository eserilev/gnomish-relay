-- The folder browser in the center of the window (SPEC.md 9.9 and 13.1).

local _, ns = ...

local Browser = {}
ns.Browser = Browser

local ROWS = 16
local ROW_HEIGHT = 18
local ROWS_TOP = 58
local CRUMBS = 7
local RECENTS = 5
local GOLD = "ffd100"
local GREEN = "1eff00"
local RED = "ff2020"
local WHITE = "ffffff"
local GREY = "c0c0c0"
local GIT = "|cff8fb6e8git|r"
local SPINNER = "Interface\\Icons\\INV_Misc_Gear_01"
local NEXT_PAGE = "Interface\\Buttons\\UI-SpellbookIcon-NextPage-"
local HIGHLIGHT = "Interface\\QuestFrame\\UI-QuestTitleHighlight"
local BACK_WIDTH = 76

local TITLE = "Pick a project folder"
local HINT = "Search folders"
local LOADING = "Loading your folders..."
local OFFLINE = "The desktop app isn't running, so your folders can't load. On your desktop, run gnomish-relay restart."
local NO_FOLDERS = "No folders found. Click Cancel to chat in your default folder."
local NO_MATCH = "No folder matches."
local NO_SUBFOLDERS = "No subfolders."
local LOADING_BELOW = "Loading folders..."
local BACK = "\226\128\185 Back"
local INTO_TIP = "Open folder"
local CHAT_HERE = "Chat here"
local NEW_CHAT_HERE = "New chat here"

local ui = {}
-- `at` is the folder of the browser, as the game sends it. nil is the list of roots.
-- `choice` is the index of the highlighted line, 0 for none.
-- `asked` holds the folders whose subfolders this open of the browser asked for.
local state = { open = false, offset = 0, choice = 0, asked = {} }

local function Label(parent, font, point, x, y)
	local text = parent:CreateFontString(nil, "OVERLAY", font)
	text:SetPoint(point, parent, point, x, y)
	text:SetJustifyH("LEFT")
	return text
end

local function Chat()
	return ns.Window.SelectedChat()
end

-- A chat with a message keeps its folder: a choice then makes a new chat (SPEC.md 9.9).
local function Fixed(chat)
	return chat ~= nil and #chat.history > 0
end

local function Current(tree)
	local node = ns.Folders.Find(tree, state.at)
	if node then
		return node
	end
	if tree and #tree.roots == 1 then
		return tree.roots[1]
	end
end

local function Choose(folder, name, isNew)
	ns.Window.ChooseFolder(folder, name, isNew)
end

local function Query()
	return strtrim(ui.filter:GetText() or "")
end

local function Searching()
	return Query() ~= ""
end

-- No answer comes while the desktop app is off, so the browser stops waiting for one.
local function Offline()
	return ns.Transport.Bridge() == "offline"
end

local function StartNaming()
	state.naming = true
	state.problem = nil
	-- The edit box is the last line, so the list scrolls to the end.
	state.offset = math.huge
	ui.name:SetText("")
	Browser.Refresh()
	ui.name:SetFocus()
end

local function StopNaming()
	state.naming = false
	state.problem = nil
	ui.name:ClearFocus()
end

local function Go(folder)
	state.at = folder
	state.offset, state.choice = 0, 0
	StopNaming()
	Browser.Refresh()
end

-- A row of a search or of the recent folders leaves the search, as in a file explorer.
local function GoInto(folder)
	if Searching() then
		ui.filter:SetText("")
	end
	Go(folder)
	ui.filter:SetFocus()
end

-- The folder above, and whether there is one. nil is the list of roots.
local function Above(tree)
	local current = Current(tree)
	if not current then
		return nil, false
	end
	if current.parent then
		return current.parent.folder, true
	end
	return nil, #tree.roots > 1
end

local function CanGoBack(tree)
	if Searching() then
		return true
	end
	local _, exists = Above(tree)
	return exists
end

-- With a search, Back leaves the search first.
local function Back()
	if Searching() then
		ui.filter:SetText("")
		return
	end
	local folder, exists = Above(ns.Folders.Tree())
	if exists then
		Go(folder)
	end
end

-- The bridge did not walk the subfolders of this folder, so the browser asks once.
local function AskBelow(node)
	if not node or not node.unwalked or state.asked[node.folder] or Offline() then
		return
	end
	state.asked[node.folder] = true
	ns.Transport.ListSubfolders(node.folder)
end

-- A line that names a folder has `folder` and `name`, so a click can highlight it.
local function MatchLines(tree, lines)
	for _, node in ipairs(ns.Folders.Search(tree, Query(), ROWS)) do
		table.insert(lines, {
			text = ns.Relay.Plain(node.name),
			mark = node.repo and GIT or "",
			right = ns.Relay.Plain(ns.Folders.ParentDisplay(tree, node.folder)),
			folder = node.folder,
			name = node.name,
		})
	end
end

local function RecentLines(tree, chat, lines)
	for _, folder in ipairs(ns.Folders.Recents(tree, chat and chat.id, RECENTS)) do
		local name = ns.Folders.Label(tree, folder)
		table.insert(lines, {
			text = ns.Relay.Plain(name),
			right = ns.Relay.Plain(ns.Folders.ParentDisplay(tree, folder)),
			folder = folder,
			name = name,
		})
	end
end

local function ChildLines(tree, current, chat, lines)
	local children = current and current.children or (tree and tree.roots or {})
	for _, node in ipairs(children) do
		local text = ns.Relay.Plain(node.name)
		if chat and node.folder == chat.cwd then
			text = "|cff" .. GREEN .. text .. "|r"
		end
		table.insert(lines, {
			text = text,
			mark = node.repo and GIT or "",
			folder = node.folder,
			name = node.name,
		})
	end
	if current and state.naming then
		table.insert(lines, { new = true })
	end
end

local function BrowseLines(tree, chat, lines)
	RecentLines(tree, chat, lines)
	if tree and #tree.roots > 0 then
		table.insert(lines, { crumbs = true })
	end
	ChildLines(tree, Current(tree), chat, lines)
end

local function Lines(tree, chat)
	local lines = {}
	if Searching() then
		MatchLines(tree, lines)
	else
		BrowseLines(tree, chat, lines)
	end
	return lines
end

-- The highlighted folder, else the folder of the breadcrumb. A search has no breadcrumb.
local function Target(tree, lines)
	local line = lines[state.choice]
	if line and line.folder then
		return line.folder, line.name
	end
	local current = Current(tree)
	if current and not Searching() then
		return current.folder, current.name
	end
end

-- Right in the search box goes into the highlighted folder.
local function IntoChoice()
	local line = Lines(ns.Folders.Tree(), Chat())[state.choice]
	if line and line.folder then
		GoInto(line.folder)
	end
end

local function OpenTarget()
	local tree = ns.Folders.Tree()
	local folder, name = Target(tree, Lines(tree, Chat()))
	if not folder then
		return false
	end
	Choose(folder, name)
	return true
end

local function PaintCrumb(button)
	local color = button.hover and GOLD or (button.last and WHITE or GREY)
	local text = "|cff" .. color .. ns.Relay.Plain(button.label) .. "|r"
	button.text:SetText((button.first and "" or "\226\128\186 ") .. text)
end

-- The parts of the breadcrumb, from the root to the folder of the browser.
local function Crumbs(tree)
	local crumbs = {}
	local node = Current(tree)
	while node do
		table.insert(crumbs, 1, { text = node.name, folder = node.folder })
		node = node.parent
	end
	if tree and #tree.roots > 1 then
		table.insert(crumbs, 1, { text = "All folders" })
	end
	return crumbs
end

local function ShowCrumbs(tree, y)
	local crumbs = Crumbs(tree)
	local x = 12
	for i, button in ipairs(ui.crumbs) do
		local crumb = crumbs[i]
		button:SetShown(crumb ~= nil)
		if crumb then
			button.label, button.first, button.last = crumb.text, i == 1, crumbs[i + 1] == nil
			PaintCrumb(button)
			button:SetWidth(button.text:GetUnboundedStringWidth() + 8)
			button:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", x, y)
			button.folder = crumb.folder
			x = x + button:GetWidth()
		end
	end
end

local function ShowRow(row, line, index, y)
	row.line = line
	row.index = index
	row:SetShown(line ~= nil and not line.crumbs)
	if not line or line.crumbs then
		return
	end
	row.text:SetText(line.text or "")
	row.mark:SetText(line.mark or "")
	row.right:SetText(line.right or "")
	row.choice:SetShown(index == state.choice and line.folder ~= nil)
	row.go:SetShown(line.folder ~= nil)
	if line.new then
		row.right:SetText(state.problem and ("|cff" .. RED .. state.problem .. "|r") or "")
		ui.name:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 24, y)
	end
end

local function RowY(i)
	return -ROWS_TOP - (i - 1) * ROW_HEIGHT
end

local function ShowRows(tree, lines)
	state.offset = math.max(0, math.min(state.offset, #lines - ROWS))
	local crumbsShown = false
	local namingShown = false
	for i, row in ipairs(ui.rows) do
		local index = state.offset + i
		local line = lines[index]
		ShowRow(row, line, index, RowY(i))
		if line and line.crumbs then
			ShowCrumbs(tree, RowY(i))
			crumbsShown = true
		end
		namingShown = namingShown or (line ~= nil and line.new == true)
	end
	if not crumbsShown then
		for _, button in ipairs(ui.crumbs) do
			button:Hide()
		end
	end
	ui.name:SetShown(namingShown)
end

local function NoTreeNote()
	local saved = ns.Store.db.folders
	if Offline() then
		return OFFLINE
	end
	if saved and saved.error and not ns.Transport.ListingFolders() then
		return "|cff" .. RED .. ns.Relay.Plain(saved.error) .. "|r"
	end
	return LOADING
end

-- An empty folder says so, and a folder whose subfolders are on the way says that.
local function EmptyFolderNote(tree)
	local current = Current(tree)
	if not current or #current.children > 0 or state.naming then
		return ""
	end
	if ns.Transport.ListingSubfolders(current.folder) and not Offline() then
		return LOADING_BELOW
	end
	return NO_SUBFOLDERS
end

-- Why the list is empty, so the player never sees a silent empty box.
local function Note(tree, lines)
	if not tree then
		return NoTreeNote()
	end
	if Searching() then
		return #lines == 0 and NO_MATCH or ""
	end
	if #tree.list == 0 then
		return NO_FOLDERS
	end
	return EmptyFolderNote(tree)
end

local function ShowNote(text, lines)
	ui.note:SetText(text)
	ui.note:SetShown(text ~= "")
	local row = math.min(#lines - state.offset + 1, ROWS)
	ui.note:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 14, RowY(row) - 3)
end

local function ShowButtons(tree, lines)
	ui.open:SetText(Fixed(Chat()) and NEW_CHAT_HERE or CHAT_HERE)
	ui.open:SetShown(Target(tree, lines) ~= nil)
	ui.newFolder:SetShown(not Searching() and Current(tree) ~= nil)
	ui.back:SetEnabled(CanGoBack(tree))
end

function Browser.Refresh()
	if not state.open then
		return
	end
	local tree = ns.Folders.Tree()
	if not Searching() then
		AskBelow(Current(tree))
	end
	local lines = Lines(tree, Chat())
	ShowRows(tree, lines)
	ShowNote(Note(tree, lines), lines)
	ShowButtons(tree, lines)
	ui.hint:SetShown(ui.filter:GetText() == "")
	local below = state.at and ns.Transport.ListingSubfolders(state.at)
	ui.spinner:SetShown((ns.Transport.ListingFolders() or below) and not Offline())
end

function Browser.IsOpen()
	return state.open
end

-- Shows the last tree at once, and asks the bridge for a new one.
function Browser.Open(chat)
	local tree = ns.Folders.Tree()
	local folder = chat and chat.cwd or ""
	state.open = true
	state.at = ns.Folders.Find(tree, folder) and folder or ""
	state.offset, state.choice = 0, 0
	state.asked = {}
	StopNaming()
	ns.Transport.ListFolders()
	ui.filter:SetText("")
	ui.filter:SetFocus()
end

function Browser.Close()
	if not state.open then
		return
	end
	state.open = false
	StopNaming()
	ui.filter:ClearFocus()
end

function Browser.Show(shown)
	ui.frame:SetShown(shown)
	if shown then
		Browser.Refresh()
	end
end

local function Step(key)
	if key == "UP" then
		return -1
	elseif key == "DOWN" then
		return 1
	end
	return 0
end

-- The highlight skips the breadcrumb and the edit box, and stays in view.
local function MoveChoice(key)
	local step = Step(key)
	local lines = Lines(ns.Folders.Tree(), Chat())
	local i = state.choice + step
	while lines[i] and not lines[i].folder do
		i = i + step
	end
	if step == 0 or not lines[i] then
		return
	end
	state.choice = i
	state.offset = math.max(math.min(state.offset, i - 1), i - ROWS)
	Browser.Refresh()
end

local function Cancel()
	ui.filter:ClearFocus()
	ns.Window.CloseBrowser()
end

-- A typed text is only a filter, never a path.
local function BuildFilter(frame)
	ui.filter = CreateFrame("EditBox", "GnomishRelayBrowserFilter", frame, "InputBoxTemplate")
	ui.filter:SetPoint("TOPLEFT", frame, "TOPLEFT", 18 + BACK_WIDTH, -30)
	ui.filter:SetHeight(22)
	ui.filter:SetAutoFocus(false)
	ui.filter:SetScript("OnTextChanged", function()
		state.choice = Searching() and 1 or 0
		state.offset = 0
		Browser.Refresh()
	end)
	-- Left, Right, and Backspace move through the folders only while the search is
	-- empty. With text, they edit it.
	ui.filter:SetScript("OnArrowPressed", function(_, key)
		if key == "LEFT" and not Searching() then
			Back()
		elseif key == "RIGHT" and not Searching() then
			IntoChoice()
		else
			MoveChoice(key)
		end
	end)
	ui.filter:SetScript("OnKeyDown", function(_, key)
		if key == "BACKSPACE" and not Searching() then
			Back()
		end
	end)
	ui.filter:SetScript("OnEnterPressed", function(self)
		if not OpenTarget() then
			self:ClearFocus()
		end
	end)
	-- Escape gives the keys back to the game.
	ui.filter:SetScript("OnEscapePressed", Cancel)
	ui.hint = ui.filter:CreateFontString("GnomishRelayBrowserHint", "OVERLAY", "GameFontDisable")
	ui.hint:SetPoint("LEFT", ui.filter, "LEFT", 2, 0)
	ui.hint:SetText(HINT)
end

local function BuildName(frame)
	ui.name = CreateFrame("EditBox", "GnomishRelayBrowserName", frame, "InputBoxTemplate")
	ui.name:SetSize(220, 20)
	ui.name:SetAutoFocus(false)
	ui.name:SetMaxBytes(255)
	ui.name:SetScript("OnTextChanged", function()
		state.problem = nil
	end)
	ui.name:SetScript("OnEnterPressed", function(self)
		local current = Current(ns.Folders.Tree())
		local name = strtrim(self:GetText() or "")
		if not current then
			return
		end
		state.problem = ns.Folders.CheckName(name, ns.Folders.ChildNames(current))
		if state.problem then
			Browser.Refresh()
			return
		end
		Choose(ns.Folders.Join(current, name), name, true)
	end)
	ui.name:SetScript("OnEscapePressed", function()
		StopNaming()
		Browser.Refresh()
	end)
	ui.name:Hide()
end

local function ShowTip(button, text)
	GameTooltip:SetOwner(button, "ANCHOR_RIGHT")
	GameTooltip:SetText(text)
	GameTooltip:Show()
end

local function HideTip()
	GameTooltip:Hide()
end

-- The arrow of the spell book, so it reads as "go on" in the style of the game.
local function BuildGo(row, i)
	row.go = CreateFrame("Button", "GnomishRelayBrowseGo" .. i, row)
	row.go:SetSize(ROW_HEIGHT + 2, ROW_HEIGHT + 2)
	row.go:SetPoint("RIGHT", row, "RIGHT", -2, 0)
	row.go:SetNormalTexture(NEXT_PAGE .. "Up")
	row.go:SetPushedTexture(NEXT_PAGE .. "Down")
	row.go:SetHighlightTexture("Interface\\Buttons\\UI-Common-MouseHilight", "ADD")
	row.go:SetScript("OnClick", function()
		GoInto(row.line.folder)
	end)
	row.go:SetScript("OnEnter", function(self)
		ShowTip(self, INTO_TIP)
	end)
	row.go:SetScript("OnLeave", HideTip)
end

local function BuildRow(frame, i)
	local row = CreateFrame("Button", "GnomishRelayBrowseRow" .. i, frame)
	row:SetPoint("TOPLEFT", frame, "TOPLEFT", 8, RowY(i))
	row:SetHeight(ROW_HEIGHT)
	row:SetHighlightTexture(HIGHLIGHT, "ADD")
	row.choice = row:CreateTexture(nil, "BACKGROUND")
	row.choice:SetAllPoints()
	row.choice:SetColorTexture(0.3, 0.25, 0.1, 0.6)
	row.text = Label(row, "GameFontHighlight", "LEFT", 4, 0)
	row.text:SetWordWrap(false)
	row.mark = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	row.mark:SetPoint("LEFT", row.text, "RIGHT", 6, 0)
	row.right = Label(row, "GameFontDisableSmall", "RIGHT", -(ROW_HEIGHT + 8), 0)
	row.right:SetJustifyH("RIGHT")
	BuildGo(row, i)
	-- WoW runs OnClick for each click of a double-click, so one click only highlights.
	row:SetScript("OnClick", function(self)
		if self.line.folder then
			state.choice = self.index
			Browser.Refresh()
			ui.filter:SetFocus()
		end
	end)
	row:SetScript("OnDoubleClick", function(self)
		if self.line.folder then
			GoInto(self.line.folder)
		end
	end)
	row:Hide()
	return row
end

local function BuildRows(frame)
	ui.rows = {}
	for i = 1, ROWS do
		ui.rows[i] = BuildRow(frame, i)
	end
end

local function BuildCrumbs(frame)
	ui.crumbs = {}
	for i = 1, CRUMBS do
		local crumb = CreateFrame("Button", "GnomishRelayCrumb" .. i, frame)
		crumb:SetHeight(ROW_HEIGHT)
		crumb:SetHighlightTexture(HIGHLIGHT, "ADD")
		crumb.text = Label(crumb, "GameFontNormal", "LEFT", 0, 0)
		crumb:SetScript("OnClick", function(self)
			Go(self.folder)
			ui.filter:SetFocus()
		end)
		crumb:SetScript("OnEnter", function(self)
			self.hover = true
			PaintCrumb(self)
		end)
		crumb:SetScript("OnLeave", function(self)
			self.hover = false
			PaintCrumb(self)
		end)
		crumb:Hide()
		ui.crumbs[i] = crumb
	end
end

local function BuildSpinner(frame)
	ui.spinner = frame:CreateTexture("GnomishRelayBrowserSpinner", "OVERLAY")
	ui.spinner:SetSize(16, 16)
	ui.spinner:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -14, -33)
	ui.spinner:SetTexture(SPINNER)
	frame:SetScript("OnUpdate", function()
		if ui.spinner:IsShown() then
			ui.spinner:SetRotation(GetTime() * 3 % (2 * math.pi))
		end
	end)
end

local function Button(frame, name, text, width, onClick)
	local button = CreateFrame("Button", name, frame, "UIPanelButtonTemplate")
	button:SetSize(width, 22)
	button:SetText(text)
	button:SetScript("OnClick", onClick)
	return button
end

local function BuildButtons(frame)
	ui.cancel = Button(frame, "GnomishRelayBrowserCancel", "Cancel", 100, Cancel)
	ui.cancel:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -10, 10)
	ui.open = Button(frame, "GnomishRelayBrowserOpen", CHAT_HERE, 120, OpenTarget)
	ui.open:SetPoint("BOTTOMRIGHT", ui.cancel, "BOTTOMLEFT", -6, 0)
	ui.newFolder = Button(frame, "GnomishRelayBrowserNewFolder", "New folder", 110, StartNaming)
	ui.newFolder:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", 10, 10)
	-- The way out is always at the top left, as in a file explorer.
	ui.back = Button(frame, "GnomishRelayBrowserBack", BACK, BACK_WIDTH - 6, Back)
	ui.back:SetPoint("TOPLEFT", frame, "TOPLEFT", 10, -30)
end

local function BuildTexts(frame)
	ui.title = frame:CreateFontString("GnomishRelayBrowserTitle", "OVERLAY", "GameFontNormalLarge")
	ui.title:SetPoint("TOPLEFT", frame, "TOPLEFT", 14, -10)
	ui.title:SetText(TITLE)
	ui.note = frame:CreateFontString("GnomishRelayBrowserNote", "OVERLAY", "GameFontDisable")
	ui.note:SetJustifyH("LEFT")
	ui.note:Hide()
end

-- The window calls this at each new size, so the filter and the rows use the room.
function Browser.Resize(width)
	ui.frame:SetWidth(width)
	ui.filter:SetWidth(width - 60 - BACK_WIDTH)
	ui.note:SetWidth(width - 28)
	for _, row in ipairs(ui.rows) do
		row:SetWidth(width - 16)
	end
end

function Browser.Build(parent, left, width, bottom)
	local frame = CreateFrame("Frame", "GnomishRelayBrowser", parent, "InsetFrameTemplate")
	frame:SetPoint("TOPLEFT", parent, "TOPLEFT", left, -84)
	frame:SetPoint("BOTTOMLEFT", parent, "BOTTOMLEFT", left, bottom)
	ui.frame = frame
	BuildTexts(frame)
	BuildFilter(frame)
	BuildSpinner(frame)
	BuildRows(frame)
	BuildCrumbs(frame)
	BuildName(frame)
	BuildButtons(frame)
	frame:EnableMouseWheel(true)
	frame:SetScript("OnMouseWheel", function(_, delta)
		state.offset = state.offset - delta * 3
		Browser.Refresh()
	end)
	Browser.Resize(width)
	frame:Hide()
end
