-- The login self-test and the health of each channel (SPEC.md 7.8). A client patch
-- can remove a function or block a channel. The addon then says so in one line, with
-- the title of its app.

local _, ns = ...

local Health = {}
ns.Health = Health

-- The client functions that the relay cannot work without, read at call time.
function Health.Required()
	local addons, timer, bits = C_AddOns or {}, C_Timer or {}, bit or {}
	return {
		{ "C_AddOns.EnableAddOn", addons.EnableAddOn },
		{ "C_AddOns.IsAddOnLoaded", addons.IsAddOnLoaded },
		{ "C_AddOns.LoadAddOn", addons.LoadAddOn },
		{ "C_Timer.After", timer.After },
		{ "C_Timer.NewTicker", timer.NewTicker },
		{ "CreateFrame", CreateFrame },
		{ "GetBuildInfo", GetBuildInfo },
		{ "GetPhysicalScreenSize", GetPhysicalScreenSize },
		{ "GetTime", GetTime },
		{ "InCombatLockdown", InCombatLockdown },
		{ "ReloadUI", ReloadUI },
		{ "Screenshot", Screenshot },
		{ "bit.band", bits.band },
		{ "bit.bnot", bits.bnot },
		{ "bit.bor", bits.bor },
		{ "bit.bxor", bits.bxor },
		{ "bit.lshift", bits.lshift },
		{ "bit.rshift", bits.rshift },
		{ "time", time },
	}
end

local status = {}

-- The chat line of each way a shot fails (Strip.lua). Only "failed" comes from the game
-- itself, so only that line names the disk.
local FAILURE_LINES = {
	failed = ": can't take screenshots. Free up disk space and check the Screenshots folder, then type /reload.",
	timeout = ": the game took too long to save a screenshot. Type /reload to try again.",
	refused = ": the game refused to take a screenshot. Type /reload to try again.",
}
-- The saved variables keep the last failures, so a player can send them for a bug report.
local MAX_FAILURES = 20

-- Any addon can change the saved variables, so a wrong shape starts a new list.
local function Failures()
	local saved = ns.Saved()
	if type(saved.shotFailures) ~= "table" then
		saved.shotFailures = {}
	end
	return saved.shotFailures
end

local function Remember(why)
	local failures = Failures()
	table.insert(failures, { at = time(), why = why })
	while #failures > MAX_FAILURES do
		table.remove(failures, 1)
	end
	status.failedAt, status.failedWhy = time(), why
end

-- The first required function that the client does not have, or nil.
function Health.Missing()
	for _, item in ipairs(Health.Required()) do
		if type(item[2]) ~= "function" then
			return item[1]
		end
	end
end

function Health.Shot(ok, why)
	if ok then
		status.out, status.outAt = "shot", time()
		return
	end
	why = FAILURE_LINES[why] and why or "failed"
	Remember(why)
	if status.out ~= "fail" then
		status.out = "fail"
		print(ns.App.title .. FAILURE_LINES[why])
	end
end

-- `free` is false after a long wait for the strip corner (SPEC.md 7.1). No flag tells
-- the bridge: while the corner is blocked, no strip reaches it.
function Health.Corner(free)
	if not free and not status.cornerBlocked then
		print(
			ns.App.title
				.. ": another addon is in the way of the colored bar. Turn off addons that take screenshots, then type /reload."
		)
	end
	status.cornerBlocked = not free
end

function Health.Slot(ok)
	if ok then
		status.inbound, status.inAt = "slots", time()
	elseif status.inbound ~= "missing" then
		status.inbound = "missing"
		print(ns.App.title .. ": some addon files are missing. Close the game, then run gnomish-relay install.")
	end
end

function Health.Blocked()
	return status.out == "fail" or status.cornerBlocked == true
end

local function Build()
	local _, build = GetBuildInfo()
	return tostring(build)
end

-- `build`, and the last result of each channel, for the report to the bridge.
function Health.Flags()
	-- Each app has its own version, because each app changes on its own (SPEC.md 7.7).
	local flags = { "build=" .. Build(), "ver=" .. ns.App.version }
	if status.out then
		table.insert(flags, "out=" .. status.out)
	end
	if status.inbound then
		table.insert(flags, "in=" .. status.inbound)
	end
	return flags
end

local function Ago(at)
	return at and string.format("%ds ago", time() - at) or "never"
end

function Health.Line()
	local line = string.format(
		"%s: build %s, last screenshot %s, last slot %s",
		ns.App.title,
		Build(),
		Ago(status.outAt),
		Ago(status.inAt)
	)
	if status.failedAt then
		line = line .. string.format(", last failed screenshot %s (%s)", Ago(status.failedAt), status.failedWhy)
	end
	return line
end
