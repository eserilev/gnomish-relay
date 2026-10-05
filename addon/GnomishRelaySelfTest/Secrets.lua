-- Secret values: the tests T1, T3, T6, T7, and T8 of the tank addon spec (section 2).
-- A check that needs combat, a group, or a target says so, and measures nothing.

local _, ns = ...

local Secrets = {}
ns.Secrets = Secrets

-- A secret value must never reach the saved variables, so only its secrecy is kept.
local function Reading(fn, ...)
	local ok, value = pcall(fn, ...)
	if not ok then
		return { status = "error", error = tostring(value) }
	end
	local secret = issecretvalue and issecretvalue(value)
	if secret then
		return { status = "measured", secret = true }
	end
	return { status = "measured", secret = false, value = value }
end

local function Needs(what)
	return { status = "needs " .. what }
end

local function Threat()
	if not InCombatLockdown() then
		return Needs("combat")
	end
	if not UnitExists("target") then
		return Needs("target")
	end
	return Reading(function()
		local _, _, percent = UnitDetailedThreatSituation("player", "target")
		return percent
	end)
end

local function NameplateHealth()
	if not UnitExists("nameplate1") then
		return Needs("nameplate")
	end
	return Reading(UnitHealth, "nameplate1")
end

local function PartyPower()
	if not IsInGroup() then
		return Needs("group")
	end
	return Reading(UnitPower, "party1")
end

local function PartyRole()
	if not IsInGroup() then
		return Needs("group")
	end
	return Reading(UnitGroupRolesAssigned, "party1")
end

local function CombatLogRestricted()
	return Reading(C_CombatLog.IsCombatLogRestricted)
end

-- The event has HasRestrictions, so the register can fail. With a restricted combat log,
-- Forever 1.60.1.70205 blocks it with the "blocked an action" popup, and pcall sees no
-- error. So the self-test registers only an open combat log.
local function CombatLogEvent()
	local asked, restricted = pcall(C_CombatLog.IsCombatLogRestricted)
	if not asked or restricted ~= false then
		return Needs("an open combat log")
	end
	local ok, err = pcall(function()
		CreateFrame("Frame"):RegisterEvent("COMBAT_LOG_EVENT_UNFILTERED")
	end)
	return { status = "measured", registered = ok, error = not ok and tostring(err) or nil }
end

function Secrets.Measure()
	return {
		has_issecretvalue = type(issecretvalue) == "function",
		t1_threat_percent = Threat(),
		t3_nameplate_health = NameplateHealth(),
		t6_party_power = PartyPower(),
		t7_party_role = PartyRole(),
		t8_combat_log_restricted = CombatLogRestricted(),
		t8_combat_log_event = CombatLogEvent(),
		player_health = Reading(UnitHealth, "player"),
		player_power = Reading(UnitPower, "player"),
		player_role = Reading(UnitGroupRolesAssigned, "player"),
		player_threat = Reading(UnitDetailedThreatSituation, "player", "target"),
	}
end

-- In combat, with the target of the player, a few seconds after the pull.
function Secrets.MeasureInCombat()
	return {
		t1_threat_percent = Threat(),
		t3_nameplate_health = NameplateHealth(),
		t8_combat_log_restricted = CombatLogRestricted(),
	}
end
