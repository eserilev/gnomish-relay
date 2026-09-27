-- LoadAddOn as the slots use it (SPEC.md 7.3): a present, a missing, a disabled, and an
-- out-of-date load-on-demand addon. scripts/selftest-link.sh links the helper addons.

-- The helper addons count their loads in a global.
--# selene: allow(global_usage)

local _, ns = ...

local AddOns = {}
ns.AddOns = AddOns

local SLOT = "GnomishRelaySelfTest_Slot"
local OFF = "GnomishRelaySelfTest_Off"
local OLD = "GnomishRelaySelfTest_Old"
local MISSING = "GnomishRelaySelfTest_Missing"

-- With no character, the scope of Enable and Disable is a guess.
local function Character()
	return (UnitName("player"))
end

-- A crashed run can leave the helper disabled, and WoW keeps that in AddOns.txt.
function AddOns.EnableHelpers()
	C_AddOns.EnableAddOn(OFF, Character())
end

local function Loads(name)
	local loads = rawget(_G, "GnomishRelaySelfTestLoads")
	return type(loads) == "table" and loads[name] or 0
end

local addonLoadedDuringCall

local watcher = CreateFrame("Frame")
watcher:RegisterEvent("ADDON_LOADED")
watcher:SetScript("OnEvent", function(_, _, name)
	if addonLoadedDuringCall and name == addonLoadedDuringCall.name then
		addonLoadedDuringCall.fired = true
	end
end)

local function Load(name)
	addonLoadedDuringCall = { name = name, fired = false }
	local returns = ns.Json.Pack(pcall(C_AddOns.LoadAddOn, name))
	local fired = addonLoadedDuringCall.fired
	addonLoadedDuringCall = nil
	return {
		returns = returns,
		addon_loaded_in_call = fired,
		is_loaded_after = C_AddOns.IsAddOnLoaded(name),
		file_runs = Loads(name),
	}
end

local function Present()
	local before = C_AddOns.IsAddOnLoaded(SLOT)
	local first = Load(SLOT)
	return { is_loaded_before = before, first = first, again = Load(SLOT) }
end

local function Disabled()
	C_AddOns.DisableAddOn(OFF, Character())
	local disabled = Load(OFF)
	C_AddOns.EnableAddOn(OFF, Character())
	return { disabled = disabled, enabled_then_loaded = Load(OFF) }
end

function AddOns.Measure()
	return {
		present = Present(),
		missing = Load(MISSING),
		disabled = Disabled(),
		out_of_date = Load(OLD),
		old_info = ns.Json.Pack(pcall(C_AddOns.GetAddOnInfo, OLD)),
	}
end
