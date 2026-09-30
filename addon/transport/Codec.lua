-- Records, frames, and cells: the Lua side of `crates/protocol` (SPEC.md 7.1).

local _, ns = ...

local Codec = {}
ns.Codec = Codec

local byte, char, floor = string.byte, string.char, math.floor
local BigEndian = ns.BigEndian

Codec.RS = "\30"
Codec.US = "\31"
Codec.MAX_PAYLOAD = 3200
Codec.MAX_RECORDS = 16
Codec.CELLS_PER_ROW = 200
Codec.MAX_ROWS = 48

local MAGIC = "\110\82"
local VERSION = 1
local TAG_LEN = 8

function Codec.IsValidId(s)
	return type(s) == "string" and #s >= 1 and #s <= 32 and not s:find("[^a-z0-9_-]")
end

-- A separator inside a field would split the record.
local function Field(s)
	return (tostring(s or ""):gsub("[\30\31]", " "))
end

-- The text is the last field, so it can keep US.
local function Text(s)
	return (tostring(s or ""):gsub("\30", " "))
end

function Codec.Record(r)
	return table.concat({
		r.token,
		r.chat,
		tostring(r.id),
		Field(r.cwd),
		Field(r.flags),
		Field(r.name),
		Text(r.text),
	}, Codec.US)
end

function Codec.Payload(records)
	local parts = {}
	for i, r in ipairs(records) do
		parts[i] = Codec.Record(r)
	end
	return table.concat(parts, Codec.RS)
end

function Codec.Hex(s)
	return (s:gsub(".", function(c)
		return string.format("%02x", c:byte())
	end))
end

function Codec.FromHex(hex)
	return (hex:gsub("%x%x", function(pair)
		return char(tonumber(pair, 16))
	end))
end

function Codec.Fletcher16(s)
	local s1, s2 = 0, 0
	for i = 1, #s do
		s1 = (s1 + byte(s, i)) % 255
		s2 = (s2 + s1) % 255
	end
	return char(s1, s2)
end

-- Returns nil for a payload over MAX_PAYLOAD. The caller checks it first.
function Codec.Frame(time, frameId, payload, key)
	if #payload > Codec.MAX_PAYLOAD then
		return nil
	end
	local body = char(VERSION)
		.. BigEndian(time, 4)
		.. BigEndian(frameId % 65536, 2)
		.. BigEndian(#payload, 2)
		.. payload
	local signed = MAGIC .. body .. Codec.Fletcher16(body)
	return signed .. ns.HmacSha256(key, signed):sub(1, TAG_LEN)
end

-- Three bytes fill eight 3-bit cells, most significant bits first. The last group
-- is padded with zero bytes.
function Codec.Cells(bytes)
	local cells = {}
	for i = 1, #bytes, 3 do
		local a, b, c = byte(bytes, i, i + 2)
		local bits = (a * 256 + (b or 0)) * 256 + (c or 0)
		for shift = 21, 0, -3 do
			cells[#cells + 1] = floor(bits / 2 ^ shift) % 8
		end
	end
	return cells
end

-- Two calibration rows, then the data. The second row runs backwards, so a
-- decoder that reads one cell off fails at once.
function Codec.StripRows(frame)
	local width = Codec.CELLS_PER_ROW
	local rows = { {}, {} }
	for i = 0, width - 1 do
		rows[1][i + 1] = i % 8
		rows[2][i + 1] = 7 - i % 8
	end
	local cells = Codec.Cells(frame)
	for i = 0, #cells - 1 do
		local row = floor(i / width) + 3
		rows[row] = rows[row] or {}
		rows[row][i % width + 1] = cells[i + 1]
	end
	return rows
end

-- The line: cells of 1 or 2 physical pixels at the corner (SPEC.md 7.1.3). Mode 1 is
-- the smallest. `crates/bridge/src/line.rs` reads it.
Codec.LINE_CELLS_PER_ROW = 200
Codec.LINE_MODES = {
	{ size = 1, bits = 24 },
	{ size = 1, bits = 12 },
	{ size = 1, bits = 6 },
	{ size = 2, bits = 24 },
	{ size = 2, bits = 12 },
	{ size = 2, bits = 6 },
}

local LINE_MARKER = { 7, 0, 4, 2, 1, 6, 5, 3 }
-- Every level of every channel, in each of the three bit counts.
local LINE_CHECK = "\1\35\69\103\137\171\205\239\254\220\186\152"

local function FullColor(cell)
	return { floor(cell / 4) % 2 * 255, floor(cell / 2) % 2 * 255, cell % 2 * 255 }
end

-- The first third of the bits of a cell is red, then green, then blue.
local function CellColor(cell, bits)
	local levels = 2 ^ (bits / 3)
	local step = 255 / (levels - 1)
	return {
		floor(cell / levels ^ 2) % levels * step,
		floor(cell / levels) % levels * step,
		cell % levels * step,
	}
end

-- Zero bytes pad the last group of three.
local function AddDataColors(colors, bytes, bits)
	for i = 1, #bytes, 3 do
		local a, b, c = byte(bytes, i, i + 2)
		local group = (a * 256 + (b or 0)) * 256 + (c or 0)
		for shift = 24 - bits, 0, -bits do
			colors[#colors + 1] = CellColor(floor(group / 2 ^ shift) % 2 ^ bits, bits)
		end
	end
end

local function LineColors(frame, modeId)
	local bits = Codec.LINE_MODES[modeId].bits
	local colors = {}
	for i, cell in ipairs(LINE_MARKER) do
		colors[i] = FullColor(cell)
	end
	colors[#colors + 1] = FullColor(modeId)
	colors[#colors + 1] = FullColor(7 - modeId)
	AddDataColors(colors, LINE_CHECK, bits)
	AddDataColors(colors, frame, bits)
	return colors
end

-- Rows of 200 colors, each {r, g, b} from 0 to 255. Black fills the last row, so the
-- width never changes.
function Codec.LineRows(frame, modeId)
	local colors = LineColors(frame, modeId)
	local width = Codec.LINE_CELLS_PER_ROW
	local rows = {}
	for r = 1, math.ceil(#colors / width) do
		rows[r] = {}
		for c = 1, width do
			rows[r][c] = colors[(r - 1) * width + c] or { 0, 0, 0 }
		end
	end
	return rows
end

-- The line test (SPEC.md 7.1.4). `crates/bridge/src/line_test.rs` reads it.
local BEACON_MAGIC = "\76\84"

-- Tells the bridge that the picture holds the test lines, and for which screen.
function Codec.Beacon(width, height)
	local sized = BEACON_MAGIC .. BigEndian(width % 65536, 2) .. BigEndian(height % 65536, 2)
	return sized .. Codec.Fletcher16(sized)
end

-- Flat runs show a color shift, and the rest gives edges in every channel.
local function LineTest()
	local parts = {}
	for _, value in ipairs({ 0, 255, 85, 170 }) do
		parts[#parts + 1] = string.rep(char(value), 12)
	end
	for i = 0, 47 do
		parts[#parts + 1] = char((i * 37 + 11) % 256)
	end
	return table.concat(parts)
end

Codec.LINE_TEST = LineTest()
