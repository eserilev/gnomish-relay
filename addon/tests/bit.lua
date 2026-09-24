-- The `bit` library of WoW for plain Lua 5.1. With "signed", results come back the
-- way LuaJIT returns them, so the addon code gets tested against both.

local signed = ... == "signed"
local WORD = 2 ^ 32
local floor = math.floor

local function Out(r)
	if signed and r >= 2 ^ 31 then
		return r - WORD
	end
	return r
end

local function Bitwise(keep)
	return function(first, ...)
		local r = first % WORD
		for i = 1, select("#", ...) do
			local a, b = r, select(i, ...) % WORD
			r = 0
			local place = 1
			for _ = 1, 32 do
				local x, y = a % 2, b % 2
				if keep(x, y) then
					r = r + place
				end
				a, b, place = (a - x) / 2, (b - y) / 2, place * 2
			end
		end
		return Out(r)
	end
end

return {
	band = Bitwise(function(x, y)
		return x == 1 and y == 1
	end),
	bor = Bitwise(function(x, y)
		return x == 1 or y == 1
	end),
	bxor = Bitwise(function(x, y)
		return x ~= y
	end),
	bnot = function(x)
		return Out(WORD - 1 - x % WORD)
	end,
	lshift = function(x, n)
		return Out(x % WORD * 2 ^ n % WORD)
	end,
	rshift = function(x, n)
		return Out(floor(x % WORD / 2 ^ n))
	end,
}
