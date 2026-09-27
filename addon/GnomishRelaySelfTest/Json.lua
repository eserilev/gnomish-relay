-- A small JSON writer for the results. `gnomish-relay selftest collect` reads them.

local _, ns = ...

local Json = {}
ns.Json = Json

Json.List = {}
Json.Null = {}

function Json.NewList()
	return setmetatable({}, Json.List)
end

-- A list of return values. A nil inside it keeps its place as null.
function Json.Pack(...)
	local out = Json.NewList()
	for i = 1, select("#", ...) do
		local value = select(i, ...)
		if value == nil then
			value = Json.Null
		end
		out[i] = value
	end
	return out
end

-- Every byte outside printable ASCII becomes \u00XX, so the text never needs UTF-8 rules.
local function String(s)
	return '"' .. s:gsub('[%c"\\\128-\255]', function(c)
		return string.format("\\u%04x", c:byte())
	end) .. '"'
end

local function Number(n)
	if n ~= n or n == math.huge or n == -math.huge then
		return "null"
	end
	if n == math.floor(n) and math.abs(n) < 2 ^ 53 then
		return string.format("%.0f", n)
	end
	return string.format("%.17g", n)
end

-- An empty table is an object, unless it has the List metatable.
local function IsList(t)
	if getmetatable(t) == Json.List then
		return true
	end
	local count = 0
	for _ in pairs(t) do
		count = count + 1
	end
	return count > 0 and count == #t
end

local Value

local function List(t)
	local parts = {}
	for i = 1, #t do
		parts[i] = Value(t[i])
	end
	return "[" .. table.concat(parts, ",") .. "]"
end

local function Object(t)
	local keys = {}
	for k in pairs(t) do
		table.insert(keys, tostring(k))
	end
	table.sort(keys)
	local parts = {}
	for i, k in ipairs(keys) do
		parts[i] = String(k) .. ":" .. Value(t[k])
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

-- A function, a thread, or a userdata shows as its type name, so nothing is lost silently.
Value = function(v)
	local kind = type(v)
	if kind == "nil" or v == Json.Null then
		return "null"
	elseif kind == "boolean" then
		return tostring(v)
	elseif kind == "number" then
		return Number(v)
	elseif kind == "string" then
		return String(v)
	elseif kind == "table" then
		return IsList(v) and List(v) or Object(v)
	end
	return String("<" .. kind .. ">")
end

Json.Encode = Value
