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
