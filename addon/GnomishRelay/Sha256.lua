-- SHA-256 and HMAC-SHA256 for the strip tag (SPEC.md 6.3).

local _, ns = ...

local band, bor, bxor, bnot = bit.band, bit.bor, bit.bxor, bit.bnot
local lshift, rshift = bit.lshift, bit.rshift
local byte, char = string.byte, string.char
local floor = math.floor

local WORD = 2 ^ 32

-- WoW's `bit` returns unsigned results and LuaJIT's returns signed ones, so every
-- result goes through this before it is stored.
local function u32(x)
	return x % WORD
end

local function ror(x, n)
	return u32(bor(rshift(x, n), lshift(x, 32 - n)))
end

-- stylua: ignore
local K = {
	0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
	0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
	0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
	0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
	0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
	0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
	0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
	0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
}

local w = {}

local function Compress(h, block, at)
	for i = 1, 16 do
		local a, b, c, d = byte(block, at + i * 4 - 4, at + i * 4 - 1)
		w[i] = ((a * 256 + b) * 256 + c) * 256 + d
	end
	for i = 17, 64 do
		local x, y = w[i - 15], w[i - 2]
		local s0 = bxor(ror(x, 7), ror(x, 18), rshift(x, 3))
		local s1 = bxor(ror(y, 17), ror(y, 19), rshift(y, 10))
		w[i] = u32(w[i - 16] + u32(s0) + w[i - 7] + u32(s1))
	end

	local a, b, c, d, e, f, g, hh = h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]
	for i = 1, 64 do
		local s1 = u32(bxor(ror(e, 6), ror(e, 11), ror(e, 25)))
		local ch = u32(bxor(band(e, f), band(bnot(e), g)))
		local t1 = hh + s1 + ch + K[i] + w[i]
		local s0 = u32(bxor(ror(a, 2), ror(a, 13), ror(a, 22)))
		local maj = u32(bxor(band(a, b), band(a, c), band(b, c)))
		hh, g, f, e = g, f, e, u32(d + t1)
		d, c, b, a = c, b, a, u32(t1 + s0 + maj)
	end

	h[1], h[2], h[3], h[4] = u32(h[1] + a), u32(h[2] + b), u32(h[3] + c), u32(h[4] + d)
	h[5], h[6], h[7], h[8] = u32(h[5] + e), u32(h[6] + f), u32(h[7] + g), u32(h[8] + hh)
end

local function BigEndian(n, width)
	local out = {}
	for i = width, 1, -1 do
		out[i] = n % 256
		n = floor(n / 256)
	end
	return char(unpack(out))
end

function ns.Sha256(msg)
	-- stylua: ignore
	local h = {
		0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
		0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
	}
	local zeros = (55 - #msg) % 64
	local padded = msg .. "\128" .. string.rep("\0", zeros) .. BigEndian(#msg * 8, 8)
	for at = 1, #padded, 64 do
		Compress(h, padded, at)
	end
	local out = {}
	for i = 1, 8 do
		out[i] = BigEndian(h[i], 4)
	end
	return table.concat(out)
end

local function XorEach(key, pad)
	local out = {}
	for i = 1, 64 do
		out[i] = bxor(byte(key, i), pad)
	end
	return char(unpack(out))
end

function ns.HmacSha256(key, msg)
	if #key > 64 then
		key = ns.Sha256(key)
	end
	key = key .. string.rep("\0", 64 - #key)
	return ns.Sha256(XorEach(key, 0x5c) .. ns.Sha256(XorEach(key, 0x36) .. msg))
end
