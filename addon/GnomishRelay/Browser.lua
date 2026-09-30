-- The folder browser in the center of the window (SPEC.md 9.9 and 13.1).

local _, ns = ...

local Browser = {}
ns.Browser = Browser

local ROWS = 16
local ROW_HEIGHT = 19
local ROWS_TOP = 38
local CRUMBS = 7
local RECENTS = 5
local GOLD = "ffd100"
local GREEN = "1eff00"
local NEW = "9fe39f"
local GIT = "|cff8fb6e8git|r"
local SPINNER = "Interface\\Icons\\INV_Misc_Gear_01"

local ui = {}
-- `at` is the folder of the browser, as the game sends it. nil is the list of roots.
local state = { open = false, offset = 0, choice = 1 }

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

local function StartNaming()
	state.naming = true
	state.problem = nil
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
	state.offset = 0
	StopNaming()
	Browser.Refresh()
end

local function MatchLines(tree, lines)
	for i, node in ipairs(ns.Folders.Search(tree, Query(), ROWS)) do
		table.insert(lines, {
			text = ns.Relay.Plain(node.name),
			mark = node.repo and GIT or "",
			right = ns.Relay.Plain(ns.Folders.ParentDisplay(tree, node.folder)),
			chosen = i == state.choice,
			click = function()
				Choose(node.folder, node.name)
			end,
		})
	end
end

local function RecentLines(tree, chat, lines)
	for _, folder in ipairs(ns.Folders.Recents(tree, chat and chat.id, RECENTS)) do
		local name = ns.Folders.Label(tree, folder)
		table.insert(lines, {
			text = ns.Relay.Plain(name),
			right = ns.Relay.Plain(ns.Folders.ParentDisplay(tree, folder)),
			click = function()
				Choose(folder, name)
			end,
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
			click = function()
				Go(node.folder)
			end,
		})
	end
	if current then
		table.insert(lines, { new = true, text = "|cff" .. NEW .. "New folder|r", click = StartNaming })
	end
end

local function BrowseLines(tree, chat, lines)
	RecentLines(tree, chat, lines)
	table.insert(lines, { crumbs = true })
	ChildLines(tree, Current(tree), chat, lines)
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
		table.insert(crumbs, 1, { text = "Roots" })
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
			local text = ns.Relay.Plain(crumb.text)
			button.text:SetText((i > 1 and "\226\128\186 " or "") .. "|cff" .. GOLD .. text .. "|r")
			button:SetWidth(button.text:GetUnboundedStringWidth() + 8)
			button:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", x, y)
			button.folder = crumb.folder
			x = x + button:GetWidth()
		end
	end
end

local function ShowRow(row, line, y)
	row.line = line
	row:SetShown(line ~= nil and not line.crumbs)
	if not line or line.crumbs then
		return
	end
	row.text:SetText(line.text)
	row.mark:SetText(line.mark or "")
	row.right:SetText(line.right or "")
	row.choice:SetShown(line.chosen == true)
	if line.new and state.naming then
		row.text:SetText("")
		row.right:SetText(state.problem and ("|cffff2020" .. state.problem .. "|r") or "")
		ui.name:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 24, y)
	end
end

local function ShowRows(tree, lines)
	state.offset = math.max(0, math.min(state.offset, #lines - ROWS))
	local crumbsShown = false
	local namingShown = false
	for i, row in ipairs(ui.rows) do
		local line = lines[state.offset + i]
		local y = -ROWS_TOP - (i - 1) * ROW_HEIGHT
		ShowRow(row, line, y)
		if line and line.crumbs then
			ShowCrumbs(tree, y)
			crumbsShown = true
		end
		namingShown = namingShown or (line ~= nil and line.new == true and state.naming == true)
	end
	if not crumbsShown then
		for _, button in ipairs(ui.crumbs) do
			button:Hide()
		end
	end
	ui.name:SetShown(namingShown)
end

function Browser.Refresh()
	if not state.open then
		return
	end
	local tree = ns.Folders.Tree()
	local chat = Chat()
	local lines = {}
	local filtering = Query() ~= ""
	if filtering then
		MatchLines(tree, lines)
	else
		BrowseLines(tree, chat, lines)
	end
	ShowRows(tree, lines)
	ui.open:SetText(Fixed(chat) and "New Chat here" or "Open")
	ui.open:SetShown(not filtering and Current(tree) ~= nil)
	ui.spinner:SetShown(ns.Transport.ListingFolders())
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
	state.offset, state.choice = 0, 1
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

local function MoveChoice(key)
	if key == "UP" then
		state.choice = math.max(1, state.choice - 1)
	elseif key == "DOWN" then
		state.choice = state.choice + 1
	end
	local count = #ns.Folders.Search(ns.Folders.Tree(), Query(), ROWS)
	state.choice = math.max(1, math.min(state.choice, count))
	Browser.Refresh()
end

local function PickChoice()
	local tree = ns.Folders.Tree()
	local node = ns.Folders.Search(tree, Query(), ROWS)[state.choice]
	if node then
		Choose(node.folder, node.name)
	end
end

-- A typed text is only a filter, never a path.
local function BuildFilter(frame)
	ui.filter = CreateFrame("EditBox", "GnomishRelayBrowserFilter", frame, "InputBoxTemplate")
	ui.filter:SetPoint("TOPLEFT", frame, "TOPLEFT", 18, -8)
	ui.filter:SetHeight(22)
	ui.filter:SetAutoFocus(false)
	ui.filter:SetScript("OnTextChanged", function()
		state.choice, state.offset = 1, 0
		Browser.Refresh()
	end)
	ui.filter:SetScript("OnArrowPressed", function(_, key)
		MoveChoice(key)
	end)
	ui.filter:SetScript("OnEnterPressed", function(self)
		if Query() == "" then
			self:ClearFocus()
		else
			PickChoice()
		end
	end)
	-- Escape gives the keys back to the game.
	ui.filter:SetScript("OnEscapePressed", function(self)
		self:ClearFocus()
		ns.Window.CloseBrowser()
	end)
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

local function BuildRows(frame)
	ui.rows = {}
	for i = 1, ROWS do
		local row = CreateFrame("Button", "GnomishRelayBrowseRow" .. i, frame)
		row:SetPoint("TOPLEFT", frame, "TOPLEFT", 8, -ROWS_TOP - (i - 1) * ROW_HEIGHT)
		row:SetHeight(ROW_HEIGHT)
		row:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
		row.choice = row:CreateTexture(nil, "BACKGROUND")
		row.choice:SetAllPoints()
		row.choice:SetColorTexture(0.3, 0.25, 0.1, 0.6)
		row.text = Label(row, "GameFontHighlight", "LEFT", 4, 0)
		row.text:SetWordWrap(false)
		row.mark = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
		row.mark:SetPoint("LEFT", row.text, "RIGHT", 6, 0)
		row.right = Label(row, "GameFontDisableSmall", "RIGHT", -4, 0)
		row.right:SetJustifyH("RIGHT")
		row:SetScript("OnClick", function(self)
			if self.line and self.line.click then
				self.line.click()
			end
		end)
		row:Hide()
		ui.rows[i] = row
	end
end

local function BuildCrumbs(frame)
	ui.crumbs = {}
	for i = 1, CRUMBS do
		local crumb = CreateFrame("Button", "GnomishRelayCrumb" .. i, frame)
		crumb:SetHeight(ROW_HEIGHT)
		crumb.text = Label(crumb, "GameFontNormal", "LEFT", 0, 0)
		crumb:SetScript("OnClick", function(self)
			Go(self.folder)
		end)
		crumb:Hide()
		ui.crumbs[i] = crumb
	end
end

local function BuildSpinner(frame)
	ui.spinner = frame:CreateTexture("GnomishRelayBrowserSpinner", "OVERLAY")
	ui.spinner:SetSize(16, 16)
	ui.spinner:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -14, -11)
	ui.spinner:SetTexture(SPINNER)
	frame:SetScript("OnUpdate", function()
		if ui.spinner:IsShown() then
			ui.spinner:SetRotation(GetTime() * 3 % (2 * math.pi))
		end
	end)
end

local function BuildOpen(frame)
	ui.open = CreateFrame("Button", "GnomishRelayBrowserOpen", frame, "UIPanelButtonTemplate")
	ui.open:SetSize(120, 22)
	ui.open:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -10, 10)
	ui.open:SetScript("OnClick", function()
		local tree = ns.Folders.Tree()
		local current = Current(tree)
		if current then
			Choose(current.folder, current.name)
		end
	end)
end

-- The window calls this at each new size, so the filter and the rows use the room.
function Browser.Resize(width)
	ui.frame:SetWidth(width)
	ui.filter:SetWidth(width - 60)
	for _, row in ipairs(ui.rows) do
		row:SetWidth(width - 16)
	end
end

function Browser.Build(parent, left, width, bottom)
	local frame = CreateFrame("Frame", "GnomishRelayBrowser", parent, "InsetFrameTemplate")
	frame:SetPoint("TOPLEFT", parent, "TOPLEFT", left, -84)
	frame:SetPoint("BOTTOMLEFT", parent, "BOTTOMLEFT", left, bottom)
	ui.frame = frame
	BuildFilter(frame)
	BuildSpinner(frame)
	BuildRows(frame)
	BuildCrumbs(frame)
	BuildName(frame)
	BuildOpen(frame)
	frame:EnableMouseWheel(true)
	frame:SetScript("OnMouseWheel", function(_, delta)
		state.offset = state.offset - delta * 3
		Browser.Refresh()
	end)
	Browser.Resize(width)
	frame:Hide()
end
