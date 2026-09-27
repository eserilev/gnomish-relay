-- The client itself: its build, the screen, the CVars of a screenshot, and the Lua
-- and `bit` behavior that the SHA code and the fake game rest on.

-- The probe of hooksecurefunc needs a global name that does not exist.
--# selene: allow(global_usage)

local _, ns = ...

local Client = {}
ns.Client = Client

local CVARS = { "screenshotFormat", "screenshotQuality", "uiScale", "useUiScale", "renderScale" }
local NO_SUCH_GLOBAL = "GnomishRelaySelfTestNoSuchFunction"

-- Known values in the saved variables. The committed file then shows how WoW writes them.
Client.FORMAT_PROBE = {
	quote = '"',
	backslash = "\\",
	newline = "a\nb",
	carriage_return = "a\rb",
	control = "\1\2\31\127",
	nul = "a\0b",
	high = "\200\255",
	big = 9007199254740991,
	negative = -12,
	fraction = 0.1,
	yes = true,
	list = { 1, "two", false },
}

function Client.Build()
	local version, build, date, interface = GetBuildInfo()
	return { version = version, build = build, date = date, interface = interface }
end

local function CVars()
	local out = {}
	for _, name in ipairs(CVARS) do
		out[name] = C_CVar.GetCVar(name)
	end
	return out
end

function Client.Screen()
	local width, height = GetPhysicalScreenSize()
	return {
		physical = { width, height },
		ui = { GetScreenWidth(), GetScreenHeight() },
		ui_parent_scale = UIParent:GetScale(),
		ui_parent_effective_scale = UIParent:GetEffectiveScale(),
		cvars = CVars(),
	}
end

local function HookMissing()
	local result = ns.Json.Pack(pcall(hooksecurefunc, NO_SUCH_GLOBAL, function() end))
	local made = type(rawget(_G, NO_SUCH_GLOBAL))
	rawset(_G, NO_SUCH_GLOBAL, nil)
	return { returns = result, global_after = made }
end

function Client.Lua()
	return {
		version = _VERSION,
		quoted_control = string.format("%q", "\1\n\0"),
		bit_bnot_0 = bit.bnot(0),
		bit_lshift_1_31 = bit.lshift(1, 31),
		bit_band_minus_1 = bit.band(-1, -1),
		bit_rshift_minus_1_28 = bit.rshift(-1, 28),
		get_time_same_in_one_handler = GetTime() == GetTime(),
		hooksecurefunc_missing = HookMissing(),
	}
end

-- Returns what SetCVar gives back, and the value that GetCVar reads after it.
function Client.SetScreenshotFormat(value)
	local returns = ns.Json.Pack(pcall(C_CVar.SetCVar, "screenshotFormat", value))
	return { returns = returns, read_back = C_CVar.GetCVar("screenshotFormat") }
end
