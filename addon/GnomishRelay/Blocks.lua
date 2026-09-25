-- Reply blocks from the bridge (SPEC.md 7.3.1). The bridge parses the Markdown and
-- escapes every text, so this file only splits lines and fields.

local _, ns = ...

local Blocks = {}
ns.Blocks = Blocks

local MARKER = "\27M1\n"
local HTML = { h = true, p = true, l = true, q = true }
local ENTITIES = { lt = "<", gt = ">", amp = "&" }

function Blocks.IsRendered(text)
	return type(text) == "string" and text:sub(1, #MARKER) == MARKER
end

-- "\1" never comes from the bridge, which drops control bytes. It holds "||" while
-- the codes go.
local function WithoutCodes(text)
	return (text:gsub("||", "\1"):gsub("|c" .. ("%x"):rep(8), ""):gsub("|r", ""))
end

-- A size limit cuts at any byte, so the last line can end in half a code, half an
-- entity, or half a character.
local function Uncut(text, html)
	text = WithoutCodes(text):gsub("|c%x*$", ""):gsub("|$", ""):gsub("[\192-\255][\128-\191]*$", "")
	if html then
		text = text:gsub("&[%a]*$", "")
	end
	return (text:gsub("\1", "||"))
end

local function Split(line)
	local parts = {}
	for part in (line .. "\31"):gmatch("([^\31]*)\31") do
		table.insert(parts, part)
	end
	return parts
end

local function Level(field, low, high)
	local n = tonumber(field) or low
	return math.max(low, math.min(high, n))
end

local function Block(parts)
	local kind = parts[1]
	if kind == "h" then
		return { kind = "heading", level = Level(parts[2], 1, 3), text = parts[3] or "" }
	elseif kind == "p" then
		return { kind = "paragraph", text = parts[2] or "" }
	elseif kind == "l" then
		return { kind = "item", level = Level(parts[2], 0, 4), number = parts[3] or "", text = parts[4] or "" }
	elseif kind == "q" then
		return { kind = "quote", text = parts[2] or "" }
	elseif kind == "c" then
		return { kind = "code", text = parts[2] or "" }
	elseif kind == "t" then
		local cells = {}
		for i = 3, #parts do
			table.insert(cells, parts[i])
		end
		return { kind = "row", header = parts[2] == "1", cells = cells }
	elseif kind == "r" then
		return { kind = "rule" }
	end
end

local function UncutLine(line)
	local html = HTML[line:sub(1, 1)] == true
	local parts = Split(line)
	for i = 2, #parts do
		parts[i] = Uncut(parts[i], html)
	end
	return parts
end

-- The blocks of a rendered reply. An unknown kind is left out, for a newer bridge.
function Blocks.Parse(text)
	local rest = text:sub(#MARKER + 1)
	local cut = rest:match("([^\n]*)$")
	local blocks = {}
	for line in rest:gmatch("([^\n]*)\n") do
		blocks[#blocks + 1] = Block(Split(line))
	end
	if cut ~= "" then
		blocks[#blocks + 1] = Block(UncutLine(cut))
	end
	return blocks
end

-- Words with no codes and no escapes: the caller escapes them for where they go.
local function Words(text, html)
	text = WithoutCodes(text):gsub("\1", "|")
	if html then
		text = text:gsub("&(%a+);", ENTITIES)
	end
	return text
end

local function PlainLine(block)
	if block.kind == "heading" or block.kind == "paragraph" then
		return Words(block.text, true)
	elseif block.kind == "item" then
		local mark = block.number ~= "" and block.number .. "." or "-"
		return string.rep("  ", block.level) .. mark .. " " .. Words(block.text, true)
	elseif block.kind == "quote" then
		return "> " .. Words(block.text, true)
	elseif block.kind == "code" then
		return "    " .. Words(block.text, false)
	elseif block.kind == "row" then
		local cells = {}
		for i, cell in ipairs(block.cells) do
			cells[i] = Words(cell, false)
		end
		return table.concat(cells, " | ")
	end
	return "----"
end

-- The reply as plain lines, for the whisper line and for a draw that fails.
function Blocks.Plain(text)
	local lines = {}
	for i, block in ipairs(Blocks.Parse(text)) do
		lines[i] = PlainLine(block)
	end
	return table.concat(lines, "\n")
end
