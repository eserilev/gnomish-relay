-- The login self-test and the health of each channel (SPEC.md 7.8). A client patch
-- can remove a function or block a channel. The addon then says so in one line.

local _, ns = ...

local Health = {}
ns.Health = Health

-- The protocol version of this addon. The bridge checks it (SPEC.md 7.7).
Health.VERSION = 1

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

-- The first required function that the client does not have, or nil.
function Health.Missing()
	for _, item in ipairs(Health.Required()) do
		if type(item[2]) ~= "function" then
			return item[1]
		end
	end
end

function Health.Shot(ok)
	if ok then
		status.out, status.outAt = "shot", time()
	elseif status.out ~= "fail" then
		status.out = "fail"
		print("Gnomish Relay: screenshots are blocked.")
	end
end

function Health.Slot(ok)
	if ok then
		status.inbound, status.inAt = "slots", time()
	elseif status.inbound ~= "missing" then
		status.inbound = "missing"
		print("Gnomish Relay: slots are missing. Run gnomish-relay install with the game closed.")
	end
end

function Health.Blocked()
	return status.out == "fail"
end

local function Build()
	local _, build = GetBuildInfo()
	return tostring(build)
end

-- `build`, and the last result of each channel, for the report to the bridge.
function Health.Flags()
	local flags = { "build=" .. Build(), "ver=" .. Health.VERSION }
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
	return string.format(
		"Gnomish Relay: build %s, last screenshot %s, last slot %s",
		Build(),
		Ago(status.outAt),
		Ago(status.inAt)
	)
end
