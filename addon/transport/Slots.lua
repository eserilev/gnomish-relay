-- The slot poll: loads one slot addon and takes the values that its files set
-- (SPEC.md 7.3). The names come from ns.App, so two apps never share a slot or a global.

-- Each global name comes from ns.App, so only _G can reach the global.
--# selene: allow(global_usage)

local _, ns = ...

local Slots = {}
ns.Slots = Slots

Slots.COUNT = 1000

function Slots.Name(n)
	return string.format(ns.App.slotPrefix, n)
end

-- The first slot that this UI session has not loaded.
function Slots.FirstFree()
	local n = 1
	while n <= Slots.COUNT and C_AddOns.IsAddOnLoaded(Slots.Name(n)) do
		n = n + 1
	end
	return n
end

local function Clear()
	_G[ns.App.slotData] = nil
	_G[ns.App.restore] = nil
	_G[ns.App.live] = nil
end

-- Returns whether the slot loaded, then its body, restore bundle, and live file. A
-- value that is left from an earlier load never counts, so each global starts empty.
function Slots.Load(n)
	local name = Slots.Name(n)
	C_AddOns.EnableAddOn(name)
	Clear()
	local loaded = C_AddOns.LoadAddOn(name)
	local data, restore, live = _G[ns.App.slotData], _G[ns.App.restore], _G[ns.App.live]
	Clear()
	return loaded, data, restore, live
end
