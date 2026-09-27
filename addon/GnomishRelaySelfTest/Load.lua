-- The first file of the self-test. It sees the saved variables before any other code
-- does, and it keeps the order of the login events.

local addonName, ns = ...

ns.Load = {
	-- WoW can load the saved variables before or after the files of the addon run.
	saved_at_file_load = type(GnomishRelaySelfTestDB),
	events = {},
}

local started = GetTimePreciseSec()

local function Note(event, detail)
	local ms = math.floor((GetTimePreciseSec() - started) * 1000)
	table.insert(ns.Load.events, { event = event, detail = detail, ms = ms })
end

local function EnterWorld(initialLogin, reloadingUi)
	if ns.Load.kind then
		return
	end
	ns.Load.kind = initialLogin and "login" or reloadingUi and "reload" or "other"
	Note("PLAYER_ENTERING_WORLD", ns.Load.kind)
	if ns.OnEnterWorld then
		ns.OnEnterWorld()
	end
end

local events = CreateFrame("Frame")
events:RegisterEvent("ADDON_LOADED")
events:RegisterEvent("VARIABLES_LOADED")
events:RegisterEvent("PLAYER_LOGIN")
events:RegisterEvent("PLAYER_ENTERING_WORLD")
events:SetScript("OnEvent", function(_, event, a, b)
	if event == "PLAYER_ENTERING_WORLD" then
		EnterWorld(a, b)
	elseif event == "ADDON_LOADED" and a == addonName then
		ns.Load.saved_at_addon_loaded = type(GnomishRelaySelfTestDB)
		Note(event, "self")
	elseif event ~= "ADDON_LOADED" then
		Note(event)
	end
end)
