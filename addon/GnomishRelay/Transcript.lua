-- The transcript of the window (SPEC.md 13.1): a scroll frame that stacks one
-- entry after the other. A rendered reply draws its blocks (SPEC.md 7.3.1).

local _, ns = ...

local Transcript = {}
ns.Transcript = Transcript

local MONO = "Interface\\AddOns\\GnomishRelay\\JetBrainsMono-Regular.ttf"
local MONO_FALLBACK = "Fonts\\ARIALN.TTF"
Transcript.MONO, Transcript.MONO_FALLBACK = MONO, MONO_FALLBACK
local BODY_FONT = "Fonts\\ARIALN.TTF"
local HEADING_FONT = "Fonts\\FRIZQT__.TTF"
-- Each heading is this much bigger than the body text. The font size is a setting.
local HEADINGS = { { "h1", 4 }, { "h2", 1 }, { "h3", -1 } }
local YOU = "69ccf0"
local GREY = "9d9d9d"
local CODE = "b8c8b8"
local LINK = "69ccf0"
local GAP = 8
local PAD = 6
local CELL_PAD = 6
local MAX_COLUMNS = 8
local WHEEL_STEP = 40
-- Room at the right of a sent message for its delivery state.
local STATUS_WIDTH = 80
-- Room at the right of the name line of a reply for its Pin link.
local PIN_WIDTH = 44
local GOLD = "ffd100"
-- Four no-break spaces: SimpleHTML drops normal spaces at the start of a line.
local INDENT = ("\194\160"):rep(4)

-- Each transcript is a view with its own scroll frame and widgets. `views` holds every
-- view, so a change of an entry draws again in each view that shows it.
local views = {}
local main
local View = {}
View.__index = View

local function NewPool(create)
	return { free = {}, used = {}, create = create }
end

local function Acquire(pool)
	local widget = table.remove(pool.free) or pool.create()
	table.insert(pool.used, widget)
	widget:ClearAllPoints()
	widget:Show()
	return widget
end

local function ReleaseAll(pool)
	for _, widget in ipairs(pool.used) do
		widget:Hide()
		table.insert(pool.free, widget)
	end
	pool.used = {}
end

-- The widgets that each pool has out, so a failed draw can give back its own.
local function Mark(v)
	local mark = {}
	for name, pool in pairs(v.pools) do
		mark[name] = #pool.used
	end
	return mark
end

local function ReleaseSince(v, mark)
	for name, pool in pairs(v.pools) do
		for i = #pool.used, mark[name] + 1, -1 do
			local widget = table.remove(pool.used, i)
			widget:Hide()
			table.insert(pool.free, widget)
		end
	end
end

local function Place(v, widget, x, y)
	widget:SetPoint("TOPLEFT", v.child, "TOPLEFT", x, -y)
end

local function FontSize()
	return ns.Store.db.fontSize
end

local function TextLine(v, text, x, y, w)
	local line = Acquire(v.pools.text)
	line:SetFont(BODY_FONT, FontSize(), "")
	line:SetWidth(w)
	line:SetText(text)
	Place(v, line, x, y)
	return y + line:GetStringHeight()
end

-- Plain text keeps the old look: code lines between ``` marks are grey and indented.
local function PlainText(text)
	local lines, inCode = {}, false
	for line in (tostring(text) .. "\n"):gmatch("([^\n]*)\n") do
		if line:match("^```") then
			inCode = not inCode
		elseif inCode then
			table.insert(lines, string.format("    |cff%s%s|r", CODE, ns.Relay.Plain(line)))
		else
			table.insert(lines, ns.Relay.Plain(line))
		end
	end
	return table.concat(lines, "\n")
end

local function Prefix(name, color)
	return string.format("|cff%s[%s]|r: ", color, name)
end

-- Blocks

local function Indent(level)
	return INDENT:rep(level + 1)
end

local function ItemHtml(block)
	local mark = block.number ~= "" and block.number .. "." or "\226\128\162"
	return "<p>" .. Indent(block.level) .. mark .. " " .. block.text .. "</p>"
end

local function BlockHtml(block)
	if block.kind == "heading" then
		local tag = HEADINGS[block.level][1]
		return string.format("<%s>%s</%s>", tag, block.text, tag)
	elseif block.kind == "item" then
		return ItemHtml(block)
	elseif block.kind == "quote" then
		return string.format("<p>%s|cff%s&gt;|r %s</p>", INDENT, GREY, block.text)
	end
	return "<p>" .. block.text .. "</p>"
end

-- A gap line goes between blocks, but not inside a list.
local function Html(run)
	local parts = { "<html><body>" }
	for i, block in ipairs(run) do
		if i > 1 and not (block.kind == "item" and run[i - 1].kind == "item") then
			table.insert(parts, "<br/>")
		end
		table.insert(parts, BlockHtml(block))
	end
	table.insert(parts, "</body></html>")
	return table.concat(parts)
end

local function NewHtml(v)
	local html = CreateFrame("SimpleHTML", nil, v.child)
	for _, heading in ipairs(HEADINGS) do
		html:SetTextColor(heading[1], 1, 0.82, 0)
	end
	html:SetTextColor("p", 0.92, 0.92, 0.92)
	return html
end

local function SetHtmlFonts(html)
	for _, heading in ipairs(HEADINGS) do
		html:SetFont(heading[1], HEADING_FONT, FontSize() + heading[2], "")
	end
	html:SetFont("p", BODY_FONT, FontSize(), "")
end

-- A guess from the text length, for a client that measures the content only later.
local function GuessHeight(run, w)
	local height = 0
	for _, block in ipairs(run) do
		local size = FontSize() + 2 + (block.kind == "heading" and HEADINGS[block.level][2] + 2 or 0)
		height = height + size * math.ceil((#block.text * 7 + 1) / w) + 14
	end
	return height
end

local function DrawHtml(v, run, x, y)
	local html = Acquire(v.pools.html)
	SetHtmlFonts(html)
	html:SetWidth(v.width - x)
	html:SetText(Html(run))
	local height = html:GetContentHeight()
	if height <= 0 then
		height = GuessHeight(run, v.width - x)
	end
	html:SetHeight(height)
	Place(v, html, x, y)
	return y + height
end

local function NewCodeBox(v)
	local box = CreateFrame("Frame", nil, v.child)
	local background = box:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.03, 0.03, 0.03, 0.95)
	box.text = box:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	box.text:SetPoint("TOPLEFT", box, "TOPLEFT", PAD, -PAD)
	box.text:SetJustifyH("LEFT")
	box.text:SetNonSpaceWrap(true)
	box.text:SetTextColor(0.85, 0.9, 0.85)
	return box
end

-- WoW finds a new file only at launch, so after an update the font can be missing.
-- Forever returns false for a missing font, and TBC Anniversary raises an error.
local function SetCodeFont(text)
	local ok, set = pcall(text.SetFont, text, MONO, FontSize() - 2, "")
	if not (ok and set) then
		text:SetFont(MONO_FALLBACK, FontSize() - 1, "")
	end
end

local function DrawCode(v, run, x, y)
	local lines = {}
	for i, block in ipairs(run) do
		lines[i] = block.text
	end
	local box = Acquire(v.pools.code)
	SetCodeFont(box.text)
	box.text:SetWidth(v.width - x - 2 * PAD)
	box.text:SetText(table.concat(lines, "\n"))
	local height = box.text:GetStringHeight() + 2 * PAD
	box:SetSize(v.width - x, height)
	Place(v, box, x, y)
	return y + height
end

local function DrawRule(v, x, y)
	local rule = Acquire(v.pools.rule)
	rule:SetSize(v.width - x, 1)
	Place(v, rule, x, y + 4)
	return y + 9
end

-- Tables

local function Cell(v, text, gold, w)
	local cell = Acquire(v.pools.cell)
	if gold then
		cell:SetTextColor(1, 0.82, 0)
	else
		cell:SetTextColor(1, 1, 1)
	end
	cell:SetFont(BODY_FONT, FontSize() - 2, "")
	cell:SetWidth(w or 0)
	cell:SetText(text)
	return cell
end

-- The width of each column, or nil when the table does not fit as a grid.
local function ColumnWidths(v, rows, room)
	local widths, total = {}, 0
	for _, row in ipairs(rows) do
		if #row.cells > MAX_COLUMNS then
			return nil
		end
		for i, text in ipairs(row.cells) do
			local cell = Cell(v, text, false)
			widths[i] = math.max(widths[i] or 0, cell:GetUnboundedStringWidth() + 2 * CELL_PAD)
		end
	end
	for _, w in ipairs(widths) do
		total = total + w
	end
	return total <= room and widths or nil
end

local function RowBackground(v, row, x, y, w, height)
	local background = Acquire(v.pools.band)
	if row.header then
		background:SetColorTexture(0.25, 0.19, 0.02, 0.9)
	else
		background:SetColorTexture(1, 1, 1, 0.05)
	end
	background:SetSize(w, height)
	Place(v, background, x, y)
end

local function DrawGridRow(v, row, widths, x, y)
	local left, height = x, 0
	for i, w in ipairs(widths) do
		local cell = Cell(v, row.cells[i] or "", row.header, w - 2 * CELL_PAD)
		Place(v, cell, left + CELL_PAD, y + 3)
		height = math.max(height, cell:GetStringHeight() + 6)
		left = left + w
	end
	RowBackground(v, row, x, y, left - x, height)
	return y + height + 1
end

local function DrawGrid(v, rows, widths, x, y)
	for _, row in ipairs(rows) do
		y = DrawGridRow(v, row, widths, x, y)
	end
	return y
end

local function CardBody(row, header)
	local lines = {}
	for i = 2, #row.cells do
		local name = header and header.cells[i]
		local label = name and name ~= "" and string.format("|cff%s%s:|r ", GREY, name) or ""
		table.insert(lines, label .. row.cells[i])
	end
	return table.concat(lines, "\n")
end

-- Too wide for a grid: each row is a card, its first cell in gold and the rest below.
local function DrawCards(v, rows, x, y)
	local header = rows[1].header and rows[1] or nil
	for _, row in ipairs(rows) do
		if not row.header then
			local title = Cell(v, row.cells[1] or "", true, v.width - x - CELL_PAD)
			Place(v, title, x + CELL_PAD, y + 3)
			local body = Cell(v, CardBody(row, header), false, v.width - x - 3 * CELL_PAD)
			Place(v, body, x + 3 * CELL_PAD, y + 3 + title:GetStringHeight())
			local height = title:GetStringHeight() + body:GetStringHeight() + 6
			RowBackground(v, row, x, y, v.width - x, height)
			y = y + height + 2
		end
	end
	return y
end

local function DrawTable(v, rows, x, y)
	local mark = Mark(v)
	local widths = ColumnWidths(v, rows, v.width - x)
	ReleaseSince(v, mark)
	if widths then
		return DrawGrid(v, rows, widths, x, y)
	end
	return DrawCards(v, rows, x, y)
end

-- Neighbor blocks of one family draw as one widget.
local FAMILY = {
	heading = "html",
	paragraph = "html",
	item = "html",
	quote = "html",
	code = "code",
	row = "table",
	rule = "rule",
}

local function DrawRun(v, family, run, x, y)
	if family == "html" then
		return DrawHtml(v, run, x, y)
	elseif family == "code" then
		return DrawCode(v, run, x, y)
	elseif family == "table" then
		return DrawTable(v, run, x, y)
	end
	return DrawRule(v, x, y)
end

local function DrawBlocks(v, blocks, x, y)
	local i = 1
	while i <= #blocks do
		local family = FAMILY[blocks[i].kind]
		local run = {}
		while blocks[i] and FAMILY[blocks[i].kind] == family do
			table.insert(run, blocks[i])
			i = i + 1
		end
		y = DrawRun(v, family, run, x, y) + PAD
	end
	return y
end

local function DrawLink(v, text, action, x, y)
	local button = Acquire(v.pools.link)
	button.action = action
	button.label:SetText(string.format("|cff%s%s|r", LINK, text))
	button:SetSize(button.label:GetUnboundedStringWidth() + 4, 16)
	Place(v, button, x, y)
	return button
end

local function DrawUsage(v, text, y)
	local usage = ns.Blocks.Usage(text)
	if not usage then
		return y
	end
	return TextLine(v, string.format("|cff%s%s|r", GREY, ns.Relay.Plain(usage)), PAD, y, v.width - PAD)
end

local function DrawRendered(v, entry, prefix, y)
	y = TextLine(v, prefix, 0, y, v.width - PIN_WIDTH)
	return DrawBlocks(v, ns.Blocks.Parse(entry.text), PAD, y + 2)
end

local function SetPinText(button, entry)
	local text = entry.pinned and "Unpin" or "Pin"
	button.label:SetText(string.format("|cff%s%s|r", entry.pinned and GOLD or LINK, text))
	button:SetSize(button.label:GetUnboundedStringWidth() + 4, 16)
end

local function DrawPin(v, entry, y)
	local button = DrawLink(v, "Pin", nil, v.width - PIN_WIDTH + 8, y)
	button.action = function()
		ns.Pins.Toggle(entry)
		SetPinText(button, entry)
	end
	SetPinText(button, entry)
end

-- If anything fails while it draws, the reply shows as plain text.
local function DrawReply(v, entry, prefix, y)
	DrawPin(v, entry, y)
	local text = entry.text
	if ns.Blocks.IsRendered(text) then
		local mark = Mark(v)
		local ok, bottom = pcall(DrawRendered, v, entry, prefix, y)
		if ok then
			return bottom
		end
		ReleaseSince(v, mark)
		text = ns.Blocks.Plain(text)
	end
	return TextLine(v, prefix .. PlainText(text), 0, y, v.width - PIN_WIDTH)
end

local function DeliveryText(entry)
	local where, shows, most = ns.Transport.Delivery(entry)
	if where == "reload" then
		return "Needs reload"
	elseif where == "delivered" then
		return "Delivered"
	elseif where == "sending" and shows >= 2 then
		return string.format("Retry %d of %d", shows, most)
	elseif where == "sending" then
		return "Sending..."
	end
end

-- A state that ends hides its line. The line keeps its room, so nothing moves.
local function UpdateDelivery(v)
	for i = #v.open, 1, -1 do
		local text = DeliveryText(v.open[i].entry)
		v.open[i].line:SetShown(text ~= nil)
		v.open[i].line:SetText(text and string.format("|cff%s%s|r", GREY, text) or "")
		if not text then
			table.remove(v.open, i)
		end
	end
end

local function DrawMessage(v, entry, y)
	local text = Prefix("You", YOU) .. PlainText(ns.Changes.Label(entry))
	if entry.answered then
		return TextLine(v, text, 0, y, v.width)
	end
	local line = Acquire(v.pools.status)
	line:SetWidth(STATUS_WIDTH)
	Place(v, line, v.width - STATUS_WIDTH, y)
	table.insert(v.open, { entry = entry, line = line, index = v.drawn.count })
	return TextLine(v, text, 0, y, v.width - STATUS_WIDTH)
end

local function DrawResend(v, message, y)
	local button = Acquire(v.pools.resend)
	button.message = message
	button.label:SetText(string.format("|cff%sResend|r", LINK))
	button:SetSize(button.label:GetUnboundedStringWidth() + 4, 16)
	Place(v, button, 0, y + 2)
	return y + 20
end

-- The words of a line of the relay. Only one with blocks of the bridge is rendered: the
-- bridge made it (SPEC.md 7.3.1). Its text can still hold agent words, and Plain takes
-- out their escapes.
local function RelayWords(text)
	if ns.Blocks.Git(text) then
		return ns.Relay.Plain(ns.Blocks.Plain(text))
	end
	return ns.Relay.Plain(text)
end

-- The answer to Checks is only blocks, and its blocks say it all.
local function RelayLine(v, text, y)
	local words = RelayWords(text)
	if words == "" and ns.Blocks.Git(text) then
		return y
	end
	return TextLine(v, string.format("|cff%s[Relay]: %s|r", GREY, words), 0, y, v.width)
end

-- An error comes from the relay, not from the agent, so it has its own grey line. An
-- error of a run can still have changes, and needs Revert most (SPEC.md 9.11).
local function DrawError(v, chat, entry, y)
	y = ns.Changes.Draw(v.canvas, chat, entry, RelayLine(v, entry.text, y), v.width)
	local message = entry.id and ns.Store.Message(chat, entry.id)
	if message and not message.attach and not message.git and message.text ~= "" then
		y = DrawResend(v, message, y)
	end
	return y
end

-- An error that looks rendered with no blocks of the bridge is text.
local function DrawEntry(v, chat, entry, y)
	if entry.attach then
		return TextLine(v, string.format('|cff%sResumed "%s"|r', GREY, ns.Relay.Plain(chat.name)), 0, y, v.width)
	elseif entry.role == "user" then
		return DrawMessage(v, entry, y)
	elseif entry.role == "error" then
		return DrawError(v, chat, entry, y)
	elseif entry.role == "note" then
		return ns.Changes.Draw(v.canvas, chat, entry, RelayLine(v, entry.text, y), v.width)
	end
	local agent = entry.agent or chat.agent
	y = DrawReply(v, entry, Prefix(ns.Relay.AgentName(agent), ns.Relay.AgentColor(agent)), y)
	y = ns.Changes.Draw(v.canvas, chat, entry, y, v.width)
	return DrawUsage(v, entry.text, y)
end

local function ScrollTo(v, offset)
	local most = math.max(0, v.contentHeight - v.viewHeight)
	v.scroll:SetVerticalScroll(math.max(0, math.min(most, offset)))
end

-- The drawn entries are still the start of the history. A history over its limit drops
-- its first entry, and then the whole chat draws again.
local function OnlyNewEntries(v, chat, history)
	if v.drawn.chatId ~= (chat and chat.id) or v.drawn.fontSize ~= FontSize() or v.drawn.width ~= v.width then
		return false
	end
	return v.drawn.count == 0 or (history[1] == v.drawn.first and history[v.drawn.count] == v.drawn.last)
end

local function Clear(v, chat)
	for _, pool in pairs(v.pools) do
		ReleaseAll(pool)
	end
	v.contentHeight = 0
	v.drawn = {
		count = 0,
		tops = {},
		marks = {},
		chat = chat,
		chatId = chat and chat.id,
		fontSize = FontSize(),
		width = v.width,
	}
	v.open = {}
end

-- Gives back the widgets of entry `from` and of every entry below it.
local function ForgetFrom(v, from)
	ReleaseSince(v, v.drawn.marks[from])
	for i = #v.open, 1, -1 do
		if v.open[i].index >= from then
			table.remove(v.open, i)
		end
	end
	v.drawn.count = from - 1
	v.contentHeight = v.drawn.tops[from]
end

local function IndexOf(v, history, entry)
	for i = 1, v.drawn.count do
		if history[i] == entry then
			return i
		end
	end
end

local function PlaceMark(v)
	local history = v.drawn.chat and v.drawn.chat.history or {}
	local i = v.marked and IndexOf(v, history, v.marked)
	v.mark:SetShown(i ~= nil)
	if not i then
		return
	end
	local bottom = i < v.drawn.count and v.drawn.tops[i + 1] or v.contentHeight
	v.mark:SetSize(v.width, bottom - GAP - v.drawn.tops[i] + 4)
	Place(v, v.mark, 0, v.drawn.tops[i] - 2)
end

-- Draws the entries after the drawn ones, and returns the new bottom.
local function DrawNew(v, chat, history)
	local y = v.contentHeight
	for i = v.drawn.count + 1, #history do
		v.drawn.tops[i], v.drawn.marks[i] = y, Mark(v)
		v.drawn.count = i
		y = DrawEntry(v, chat, history[i], y) + GAP
	end
	v.drawn.count, v.drawn.first, v.drawn.last = #history, history[1], history[#history]
	UpdateDelivery(v)
	v.contentHeight = y
	v.child:SetHeight(math.max(y, 1))
	PlaceMark(v)
	return y
end

-- Drawing costs time, so only new entries draw. The whole chat draws again only when
-- the chat, the font size, or the width changes.
function View:Show(chat)
	local history = chat and chat.history or {}
	if not OnlyNewEntries(self, chat, history) then
		Clear(self, chat)
	end
	if #history == self.drawn.count and self.drawn.count > 0 then
		UpdateDelivery(self)
		return
	end
	ScrollTo(self, DrawNew(self, chat, history))
end

-- Draws again from `entry` down, and keeps the scroll where it is.
local function RedrawFrom(v, entry)
	local chat = v.drawn.chat
	local history = chat and chat.history or {}
	local i = OnlyNewEntries(v, chat, history) and IndexOf(v, history, entry)
	if i then
		ForgetFrom(v, i)
	else
		Clear(v, chat)
	end
	local offset = v.scroll:GetVerticalScroll()
	DrawNew(v, chat, history)
	ScrollTo(v, offset)
end

-- Scrolls `entry` to the top and marks it with a band.
function View:JumpTo(entry)
	self.marked = entry
	PlaceMark(self)
	local i = self.drawn.chat and IndexOf(self, self.drawn.chat.history, entry)
	if i then
		ScrollTo(self, self.drawn.tops[i] - 2)
	end
end

function View:Unmark()
	self.marked = nil
	self.mark:Hide()
end

local function NewTexture(v, layer, r, g, b, a)
	return function()
		local texture = v.child:CreateTexture(nil, layer)
		if r then
			texture:SetColorTexture(r, g, b, a)
		end
		return texture
	end
end

local function NewFontString(v, template, font)
	return function()
		local text = v.child:CreateFontString(nil, "OVERLAY", template)
		text:SetJustifyH("LEFT")
		if font then
			text:SetFontObject(font)
		end
		return text
	end
end

-- Resend sends to the chat of the view, which is not always the chat of the window.
local function NewResend(v)
	v.resends = v.resends + 1
	local button = CreateFrame("Button", v.prefix .. "Resend" .. v.resends, v.child)
	button.label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	button.label:SetPoint("LEFT", button, "LEFT", 0, 0)
	button:SetScript("OnClick", function(self)
		ns.Window.Resend(v.drawn.chat, self.message)
	end)
	return button
end

local function NewLink(v)
	local button = CreateFrame("Button", nil, v.child)
	button.label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	button.label:SetPoint("LEFT", button, "LEFT", 0, 0)
	button:SetScript("OnClick", function(self)
		self.action()
	end)
	return button
end

local function RedrawIn(v, entry)
	local history = v.drawn.chat and v.drawn.chat.history or {}
	if not IndexOf(v, history, entry) then
		return
	end
	local grew = #history > v.drawn.count
	RedrawFrom(v, entry)
	if grew then
		ScrollTo(v, v.contentHeight)
	end
end

-- An old entry changed. An entry that is not on screen draws later as it is now. A new
-- entry below it still scrolls to the bottom.
function Transcript.Redraw(entry)
	for _, v in ipairs(views) do
		RedrawIn(v, entry)
	end
end

-- The next Show draws the whole chat again for the new width. A new height keeps the
-- line at the bottom of the view in place.
function View:Resize(w, h)
	local grown = self.viewHeight and h - self.viewHeight or 0
	self.width, self.viewHeight = w, h
	self.scroll:SetSize(w, h)
	self.child:SetWidth(w)
	ScrollTo(self, self.scroll:GetVerticalScroll() - grown)
end

local function BuildPools(v)
	local function Of(create)
		return NewPool(function()
			return create(v)
		end)
	end
	v.pools = {
		text = NewPool(NewFontString(v, "GameFontHighlight", ChatFontNormal)),
		cell = NewPool(NewFontString(v, "GameFontHighlightSmall")),
		status = NewPool(NewFontString(v, "GameFontDisableSmall")),
		resend = Of(NewResend),
		link = Of(NewLink),
		html = Of(NewHtml),
		code = Of(NewCodeBox),
		rule = NewPool(NewTexture(v, "ARTWORK", 0.6, 0.5, 0.2, 0.8)),
		band = NewPool(NewTexture(v, "BACKGROUND")),
	}
	-- A draw again from one entry gives back the change blocks below it too.
	for name, pool in pairs(v.canvas.pools) do
		v.pools["changes_" .. name] = pool
	end
end

-- A transcript in `parent`, with frame names that start with `prefix`. The owner gives
-- the size, so the layout needs no frame sizes.
function Transcript.New(parent, w, h, prefix)
	local v = setmetatable({ prefix = prefix, resends = 0, contentHeight = 0, open = {} }, View)
	v.drawn = { count = 0, tops = {}, marks = {} }
	v.scroll = CreateFrame("ScrollFrame", prefix .. "Scroll", parent)
	v.scroll:SetPoint("TOPLEFT", parent, "TOPLEFT", 8, -6)
	v.child = CreateFrame("Frame", prefix .. "Transcript", v.scroll)
	v.child:SetHeight(1)
	v:Resize(w, h)
	v.scroll:SetScrollChild(v.child)
	v.mark = v.child:CreateTexture(prefix .. "Mark", "BACKGROUND")
	v.mark:SetColorTexture(1, 0.82, 0, 0.12)
	v.mark:Hide()
	v.scroll:EnableMouseWheel(true)
	v.scroll:SetScript("OnMouseWheel", function(self, delta)
		ScrollTo(v, self:GetVerticalScroll() - delta * WHEEL_STEP)
	end)
	v.canvas = ns.Changes.Canvas(v.child)
	BuildPools(v)
	table.insert(views, v)
	return v
end

-- The transcript of the window. Search and Pins work on this one.
function Transcript.Build(parent, w, h)
	main = Transcript.New(parent, w, h, "GnomishRelay")
	ns.Changes.Build()
end

function Transcript.Show(chat)
	main:Show(chat)
end

function Transcript.Resize(w, h)
	main:Resize(w, h)
end

function Transcript.JumpTo(entry)
	main:JumpTo(entry)
end

function Transcript.Unmark()
	main:Unmark()
end
