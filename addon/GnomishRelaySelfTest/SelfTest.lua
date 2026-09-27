-- Runs the checks and keeps the results in the saved variables (SPEC.md 14.3).
-- `gnomish-relay selftest collect` reads them after a /reload: WoW writes the saved
-- variables only at a /reload or a logout.

local _, ns = ...

local SelfTest = {}
ns.SelfTest = SelfTest

local FORMAT = 1
-- PLAYER_ENTERING_WORLD comes before the world draws. A shot then can catch the loading screen.
local START_DELAY = 5
-- A fight needs a few seconds before the threat numbers mean anything.
local COMBAT_DELAY = 2
local COMBAT_FIRST_ID = 100

local running = false
local combatDone = false

local function Say(text)
	print("|cff69ccf0" .. ns.App.title .. ":|r " .. text)
end

local function Db()
	return ns.Saved()
end

local function BuildName()
	local build = ns.Client.Build()
	return build.version .. "." .. build.build
end

local function Store(key, value)
	Db()[key] = ns.Codec.Hex(ns.Json.Encode(value))
end

-- A session knows the load order only when a saved variables file existed at its start.
-- Returns true when this session is the first one that knows it.
local function StoreLoad()
	local db = Db()
	db.sessions = type(db.sessions) == "table" and db.sessions or {}
	db.sessions[ns.Load.kind or "other"] = {
		saved_at_file_load = ns.Load.saved_at_file_load,
		saved_at_addon_loaded = ns.Load.saved_at_addon_loaded,
		events = ns.Load.events,
	}
	Store("load", { sessions = db.sessions })
	local known = ns.Load.saved_at_addon_loaded == "table"
	local first = known and not db.load_known
	db.load_known = db.load_known or known
	return first
end

-- The game can differ from what the self-test expects. An error in one part is a result
-- too, and the other parts still run.
local function Safe(measure)
	local ok, value = pcall(measure)
	return ok and value or { error = tostring(value) }
end

local function WithPng(run, done)
	local before = C_CVar.GetCVar("screenshotFormat")
	local set = Safe(function()
		return ns.Client.SetScreenshotFormat("png")
	end)
	run(function(value)
		if before then
			pcall(C_CVar.SetCVar, "screenshotFormat", before)
		end
		done(value, set, before)
	end)
end

-- Calls `done(results)`. An error at the start of an async part ends that part at once.
local function Async(measure, done)
	local ok, err = pcall(measure, done)
	if not ok then
		done({ error = tostring(err) })
	end
end

local function Measure(withScales, done)
	local results = {
		format = FORMAT,
		placeholder = false,
		key = ns.Codec.Hex(ns.Shots.TEST_KEY),
		clocks_at_start = Safe(ns.Timing.Clocks),
		client = ns.Client.Build(),
		screen = Safe(ns.Client.Screen),
		lua = Safe(ns.Client.Lua),
		format_probe = ns.Client.FORMAT_PROBE,
		addons = Safe(ns.AddOns.Measure),
		secrets = Safe(ns.Secrets.Measure),
	}
	Async(ns.Fonts.Measure, function(fonts)
		results.fonts = fonts
		Async(ns.Timing.Measure, function(timing)
			results.timing = timing
			WithPng(function(finish)
				ns.Shots.Run(ns.Shots.Golden(withScales), 1, finish)
			end, function(shots, set, before)
				results.shots, results.set_screenshot_format, results.screenshot_format_before = shots, set, before
				results.clocks_at_end = Safe(ns.Timing.Clocks)
				done(results)
			end)
		end)
	end)
end

function SelfTest.Run(withScales)
	if running then
		Say("a run is in progress.")
		return
	end
	running = true
	Say("measuring the game. This takes about a minute. Stay out of combat.")
	ns.AddOns.EnableHelpers()
	Measure(withScales, function(results)
		Store("results", results)
		Db().build = BuildName()
		Db().format_probe = ns.Client.FORMAT_PROBE
		StoreLoad()
		running = false
		Say("done. Type /reload, then run: gnomish-relay selftest collect")
	end)
end

local function MeasureCombat()
	local combat = { secrets = Safe(ns.Secrets.MeasureInCombat) }
	WithPng(function(finish)
		ns.Shots.Run(ns.Shots.Combat(), COMBAT_FIRST_ID, finish)
	end, function(shots)
		combat.shots = shots
		Store("combat", combat)
		Say("the combat checks are done. Type /reload to keep them.")
	end)
end

function ns.OnEnterWorld()
	local firstToKnowLoad = StoreLoad()
	if Db().build ~= BuildName() then
		C_Timer.After(START_DELAY, function()
			SelfTest.Run(false)
		end)
	elseif firstToKnowLoad then
		Say("type /reload once more to keep the load order.")
	end
end

local combatEvents = CreateFrame("Frame")
combatEvents:RegisterEvent("PLAYER_REGEN_DISABLED")
combatEvents:SetScript("OnEvent", function()
	if combatDone or running then
		return
	end
	combatDone = true
	C_Timer.After(COMBAT_DELAY, MeasureCombat)
end)

SLASH_GNOMISHRELAYSELFTEST1 = "/grst"
SlashCmdList.GNOMISHRELAYSELFTEST = function(arg)
	SelfTest.Run(strtrim(arg or "") == "scale")
end
