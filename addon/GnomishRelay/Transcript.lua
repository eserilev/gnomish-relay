-- The transcript of the window (SPEC.md 13.1): a scroll frame that stacks one
-- entry after the other. A rendered reply draws its blocks (SPEC.md 7.3.1).

local _, ns = ...

local Transcript = {}
ns.Transcript = Transcript

local MONO = "Interface\\AddOns\\GnomishRelay\\JetBrainsMono-Regular.ttf"
local MONO_FALLBACK = "Fonts\\ARIALN.TTF"
local BODY_FONT = "Fonts\\ARIALN.TTF"
local HEADING_FONT = "Fonts\\FRIZQT__.TTF"
local HEADINGS = { { "h1", 18 }, { "h2", 15 }, { "h3", 13 } }
local YOU = "69ccf0"
local GREY = "9d9d9d"
local CODE = "b8c8b8"
local GAP = 8
local PAD = 6
local CELL_PAD = 6
local MAX_COLUMNS = 8
local WHEEL_STEP = 40
-- Four no-break spaces: SimpleHTML drops normal spaces at the start of a line.
local INDENT = ("\194\160"):rep(4)

local ui = {}
local width, viewHeight
local contentHeight = 0
local drawnKey

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
local function Mark()
	local mark = {}
	for name, pool in pairs(ui.pools) do
		mark[name] = #pool.used
	end
	return mark
end

local function ReleaseSince(mark)
	for name, pool in pairs(ui.pools) do
		for i = #pool.used, mark[name] + 1, -1 do
			local widget = table.remove(pool.used, i)
			widget:Hide()
			table.insert(pool.free, widget)
		end
	end
end

local function Place(widget, x, y)
	widget:SetPoint("TOPLEFT", ui.child, "TOPLEFT", x, -y)
end

local function TextLine(text, x, y, w)
	local line = Acquire(ui.pools.text)
	line:SetWidth(w)
	line:SetText(text)
	Place(line, x, y)
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

local function NewHtml()
	local html = CreateFrame("SimpleHTML", nil, ui.child)
	for _, heading in ipairs(HEADINGS) do
		html:SetFont(heading[1], HEADING_FONT, heading[2], "")
		html:SetTextColor(heading[1], 1, 0.82, 0)
	end
	html:SetFont("p", BODY_FONT, 14, "")
	html:SetTextColor("p", 0.92, 0.92, 0.92)
	return html
end

-- A guess from the text length, for a client that measures the content only later.
local function GuessHeight(run, w)
	local height = 0
	for _, block in ipairs(run) do
		local size = block.kind == "heading" and HEADINGS[block.level][2] + 4 or 16
		height = height + size * math.ceil((#block.text * 7 + 1) / w) + 14
	end
	return height
end

local function DrawHtml(run, x, y)
	local html = Acquire(ui.pools.html)
	html:SetWidth(width - x)
	html:SetText(Html(run))
	local height = html:GetContentHeight()
	if height <= 0 then
		height = GuessHeight(run, width - x)
	end
	html:SetHeight(height)
	Place(html, x, y)
	return y + height
end

local function NewCodeBox()
	local box = CreateFrame("Frame", nil, ui.child)
	local background = box:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.03, 0.03, 0.03, 0.95)
	box.text = box:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	box.text:SetPoint("TOPLEFT", box, "TOPLEFT", PAD, -PAD)
	box.text:SetJustifyH("LEFT")
	box.text:SetNonSpaceWrap(true)
	-- WoW finds a new file only at launch, so after an update the font can be missing.
	if not box.text:SetFont(MONO, 12, "") then
		box.text:SetFont(MONO_FALLBACK, 13, "")
	end
	box.text:SetTextColor(0.85, 0.9, 0.85)
	return box
end

local function DrawCode(run, x, y)
	local lines = {}
	for i, block in ipairs(run) do
		lines[i] = block.text
	end
	local box = Acquire(ui.pools.code)
	box.text:SetWidth(width - x - 2 * PAD)
	box.text:SetText(table.concat(lines, "\n"))
	local height = box.text:GetStringHeight() + 2 * PAD
	box:SetSize(width - x, height)
	Place(box, x, y)
	return y + height
end

local function DrawRule(x, y)
	local rule = Acquire(ui.pools.rule)
	rule:SetSize(width - x, 1)
	Place(rule, x, y + 4)
	return y + 9
end

-- Tables

local function Cell(text, gold, w)
	local cell = Acquire(ui.pools.cell)
	if gold then
		cell:SetTextColor(1, 0.82, 0)
	else
		cell:SetTextColor(1, 1, 1)
	end
	cell:SetWidth(w or 0)
	cell:SetText(text)
	return cell
end

-- The width of each column, or nil when the table does not fit as a grid.
local function ColumnWidths(rows, room)
	local widths, total = {}, 0
	for _, row in ipairs(rows) do
		if #row.cells > MAX_COLUMNS then
			return nil
		end
		for i, text in ipairs(row.cells) do
			local cell = Cell(text, false)
			widths[i] = math.max(widths[i] or 0, cell:GetUnboundedStringWidth() + 2 * CELL_PAD)
		end
	end
	for _, w in ipairs(widths) do
		total = total + w
	end
	return total <= room and widths or nil
end

local function RowBackground(row, x, y, w, height)
	local background = Acquire(ui.pools.band)
	if row.header then
		background:SetColorTexture(0.25, 0.19, 0.02, 0.9)
	else
		background:SetColorTexture(1, 1, 1, 0.05)
	end
	background:SetSize(w, height)
	Place(background, x, y)
end

local function DrawGridRow(row, widths, x, y)
	local left, height = x, 0
	for i, w in ipairs(widths) do
		local cell = Cell(row.cells[i] or "", row.header, w - 2 * CELL_PAD)
		Place(cell, left + CELL_PAD, y + 3)
		height = math.max(height, cell:GetStringHeight() + 6)
		left = left + w
	end
	RowBackground(row, x, y, left - x, height)
	return y + height + 1
end

local function DrawGrid(rows, widths, x, y)
	for _, row in ipairs(rows) do
		y = DrawGridRow(row, widths, x, y)
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
local function DrawCards(rows, x, y)
	local header = rows[1].header and rows[1] or nil
	for _, row in ipairs(rows) do
		if not row.header then
			local title = Cell(row.cells[1] or "", true, width - x - CELL_PAD)
			Place(title, x + CELL_PAD, y + 3)
			local body = Cell(CardBody(row, header), false, width - x - 3 * CELL_PAD)
			Place(body, x + 3 * CELL_PAD, y + 3 + title:GetStringHeight())
			local height = title:GetStringHeight() + body:GetStringHeight() + 6
			RowBackground(row, x, y, width - x, height)
			y = y + height + 2
		end
	end
	return y
end

local function DrawTable(rows, x, y)
	local mark = Mark()
	local widths = ColumnWidths(rows, width - x)
	ReleaseSince(mark)
	if widths then
		return DrawGrid(rows, widths, x, y)
	end
	return DrawCards(rows, x, y)
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

local function DrawRun(family, run, x, y)
	if family == "html" then
		return DrawHtml(run, x, y)
	elseif family == "code" then
		return DrawCode(run, x, y)
	elseif family == "table" then
		return DrawTable(run, x, y)
	end
	return DrawRule(x, y)
end

local function DrawBlocks(blocks, x, y)
	local i = 1
	while i <= #blocks do
		local family = FAMILY[blocks[i].kind]
		local run = {}
		while blocks[i] and FAMILY[blocks[i].kind] == family do
			table.insert(run, blocks[i])
			i = i + 1
		end
		y = DrawRun(family, run, x, y) + PAD
	end
	return y
end

local function DrawRendered(prefix, text, y)
	y = TextLine(prefix, 0, y, width)
	return DrawBlocks(ns.Blocks.Parse(text), PAD, y + 2)
end

-- If anything fails while it draws, the reply shows as plain text.
local function DrawReply(prefix, text, y)
	if ns.Blocks.IsRendered(text) then
		local mark = Mark()
		local ok, bottom = pcall(DrawRendered, prefix, text, y)
		if ok then
			return bottom
		end
		ReleaseSince(mark)
		text = ns.Blocks.Plain(text)
	end
	return TextLine(prefix .. PlainText(text), 0, y, width)
end

-- Only the bridge renders, and only a done reply: an error that looks rendered is text.
local function DrawEntry(chat, entry, y)
	if entry.attach then
		return TextLine(string.format('|cff%sResumed "%s"|r', GREY, ns.Relay.Plain(chat.name)), 0, y, width)
	elseif entry.role == "user" then
		return TextLine(Prefix("You", YOU) .. PlainText(entry.text), 0, y, width)
	end
	local agent = entry.agent or chat.agent
	local name = ns.Relay.AgentName(agent)
	if entry.role == "error" then
		return TextLine(Prefix(name, "ff2020") .. PlainText(entry.text), 0, y, width)
	end
	return DrawReply(Prefix(name, ns.Relay.AgentColor(agent)), entry.text, y)
end

local function ScrollTo(offset)
	local most = math.max(0, contentHeight - viewHeight)
	ui.scroll:SetVerticalScroll(math.max(0, math.min(most, offset)))
end

-- Drawing costs time, so a chat draws again only when its history changes.
local function KeyOf(chat)
	if not chat then
		return "none"
	end
	local history = chat.history
	return chat.id .. ":" .. #history .. ":" .. tostring(history[#history])
end

function Transcript.Show(chat)
	local key = KeyOf(chat)
	if key == drawnKey then
		return
	end
	drawnKey = key
	for _, pool in pairs(ui.pools) do
		ReleaseAll(pool)
	end
	local y = 0
	for _, entry in ipairs(chat and chat.history or {}) do
		y = DrawEntry(chat, entry, y) + GAP
	end
	contentHeight = y
	ui.child:SetHeight(math.max(y, 1))
	ScrollTo(y)
end

local function NewTexture(layer, r, g, b, a)
	return function()
		local texture = ui.child:CreateTexture(nil, layer)
		if r then
			texture:SetColorTexture(r, g, b, a)
		end
		return texture
	end
end

local function NewFontString(template, font)
	return function()
		local text = ui.child:CreateFontString(nil, "OVERLAY", template)
		text:SetJustifyH("LEFT")
		if font then
			text:SetFontObject(font)
		end
		return text
	end
end

-- `parent` is the inset of the log. The size is fixed, so the layout needs no frame sizes.
function Transcript.Build(parent, w, h)
	width, viewHeight = w, h
	ui.scroll = CreateFrame("ScrollFrame", "GnomishRelayScroll", parent)
	ui.scroll:SetPoint("TOPLEFT", parent, "TOPLEFT", 8, -6)
	ui.scroll:SetSize(w, h)
	ui.child = CreateFrame("Frame", "GnomishRelayTranscript", ui.scroll)
	ui.child:SetSize(w, 1)
	ui.scroll:SetScrollChild(ui.child)
	ui.scroll:EnableMouseWheel(true)
	ui.scroll:SetScript("OnMouseWheel", function(self, delta)
		ScrollTo(self:GetVerticalScroll() - delta * WHEEL_STEP)
	end)
	ui.pools = {
		text = NewPool(NewFontString("GameFontHighlight", ChatFontNormal)),
		cell = NewPool(NewFontString("GameFontHighlightSmall")),
		html = NewPool(NewHtml),
		code = NewPool(NewCodeBox),
		rule = NewPool(NewTexture("ARTWORK", 0.6, 0.5, 0.2, 0.8)),
		band = NewPool(NewTexture("BACKGROUND")),
	}
end
