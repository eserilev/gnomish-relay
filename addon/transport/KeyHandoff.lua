-- Takes the strip key from the key addon of this app (SPEC.md 7.3.2). The desktop app
-- writes the key addon, so an addon app that replaces this folder keeps the key.

-- Each global name comes from ns.App, so only _G can reach the global.
--# selene: allow(global_usage)

local _, ns = ...

local KeyHandoff = {}
ns.KeyHandoff = KeyHandoff

local KEY_HEX_LENGTH = 64

-- Where the key came in, for /relay diag: "file load" or an event name. "missing" until then.
KeyHandoff.step = "missing"

-- Why the last try got no key: the reason of LoadAddOn, such as "MISSING".
KeyHandoff.problem = nil

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
	local _, reason = addons.LoadAddOn(ns.App.keyAddon)
	KeyHandoff.problem = reason
	local hex = rawget(_G, ns.App.keyGlobal)
	rawset(_G, ns.App.keyGlobal, nil)
	if not IsKeyHex(hex) then
		return nil
	end
	return Bytes(hex)
end

-- An older desktop app wrote the key addon for another WoW version (SPEC.md 7.9), so
-- WoW refuses it as out of date.
function KeyHandoff.OutOfDate()
	return KeyHandoff.problem == "INTERFACE_VERSION"
end

-- Nobody has tested LoadAddOn during the file load of another addon in the Forever
-- client. So the app tries again at its ADDON_LOADED and at PLAYER_LOGIN. The first key
-- wins, and each try clears the global in the same call that reads it.
function KeyHandoff.Try(step)
	if ns.key then
		return
	end
	ns.key = KeyHandoff.Take()
	if ns.key then
		KeyHandoff.step = step
	end
end

KeyHandoff.Try("file load")
