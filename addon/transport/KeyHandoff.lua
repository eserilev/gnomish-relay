-- Takes the strip key from the key addon of this app (SPEC.md 7.3.2). The desktop app
-- writes the key addon, so an addon app that replaces this folder keeps the key.

-- Each global name comes from ns.App, so only _G can reach the global.
--# selene: allow(global_usage)

local _, ns = ...

local KeyHandoff = {}
ns.KeyHandoff = KeyHandoff

local KEY_HEX_LENGTH = 64

local function IsKeyHex(value)
	return type(value) == "string" and #value == KEY_HEX_LENGTH and value:match("^%x+$") ~= nil
end

local function Bytes(hex)
	return (hex:gsub("%x%x", function(pair)
		return string.char(tonumber(pair, 16))
	end))
end

-- The key addon loads only on demand, so it runs once in a UI session, and only here.
-- The global lives from its file to the line after LoadAddOn. rawget and rawset skip a
-- metatable that another addon put on _G.
function KeyHandoff.Take()
	local addons = C_AddOns or {}
	if not addons.LoadAddOn then
		return nil
	end
	addons.EnableAddOn(ns.App.keyAddon)
	addons.LoadAddOn(ns.App.keyAddon)
	local hex = rawget(_G, ns.App.keyGlobal)
	rawset(_G, ns.App.keyGlobal, nil)
	if not IsKeyHex(hex) then
		return nil
	end
	return Bytes(hex)
end

ns.key = KeyHandoff.Take()
