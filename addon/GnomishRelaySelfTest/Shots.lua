-- Golden strips: known frames drawn by the real Strip.lua and Codec.lua, one screenshot
-- each. Each shot also measures the time from Screenshot() to its event, and when the
-- "Screen captured" text shows.

-- The strip frame of Strip.lua is reachable only by its global name.
--# selene: allow(global_usage)

local _, ns = ...

local Shots = {}
ns.Shots = Shots

-- PUBLIC. It signs only test strips, never a message. The bridge tests know it too.
Shots.TEST_KEY = "gnomish-relay public test key 01"
Shots.TIME = 1790211079
-- 13 + 62 = 75 bytes fill one data row, so the tag sits alone in the next row. 137 does
-- the same one row later. 0, 1, and 62 give each remainder of the cell groups.
local SIZES = { 0, 1, 62, 137, 500, 3200 }
local SCALES = { 0.64, 1 }
-- WoW names a screenshot by the second, so two shots never share one.
local GAP = 2
local RETRY = 1
local MAX_TRIES = 30

local current
local restoreScale

local function Bytes(seed, len)
	local out = {}
	for k = 1, len do
		out[k] = string.char((seed * 37 + k * 11) % 256)
	end
	return table.concat(out)
end

-- A payload as the relay sends it: two records, separators, and UTF-8 text.
local function Records()
	return ns.Codec.Payload({
		{
			token = "selftest",
			chat = "c1",
			id = 1,
			cwd = "/code",
			flags = "n;agent=claude",
			name = "Gnome",
			text = "Hello, w\195\182rld \226\128\162 \226\156\147\31kept",
		},
		{ token = "selftest", chat = "c2", id = 2, text = "second" },
	})
end

local function Plan(kind, name, payload, scale)
	return { kind = kind, name = name, payload = payload, scale = scale }
end

-- Each channel counts from 0 to 255 in its own direction and step, so every value shows
-- in every channel. Flat runs of four values then show a color shift apart from a blur:
-- each of these bytes gives one flat color in every mode. Black and white cells last
-- give sharp edges (SPEC.md 14.3.1).
local function LinePattern()
	local out = {}
	for k = 0, 255 do
		out[#out + 1] = string.char(k, 255 - k, (k * 97 + 13) % 256)
	end
	for _, value in ipairs({ 0, 255, 85, 170 }) do
		out[#out + 1] = string.char(value):rep(24)
	end
	for k = 1, 20 do
		out[#out + 1] = k % 2 == 0 and "\0\0\0" or "\255\255\255"
	end
	return table.concat(out)
end

local function LinePlan(mode)
	local step = Plan("line", "line-" .. mode, LinePattern())
	step.line = mode
	return step
end

function Shots.Golden(withScales)
	local plan = {}
	for i, len in ipairs(SIZES) do
		table.insert(plan, Plan("golden", string.format("len-%04d", len), Bytes(i, len)))
	end
	table.insert(plan, Plan("golden", "records", Records()))
	for mode in ipairs(ns.Codec.LINE_MODES) do
		table.insert(plan, LinePlan(mode))
	end
	table.insert(plan, Plan("hide_after_call", "hide-after-call", "probe: the strip hides right after Screenshot()"))
	local now = UIParent:GetScale()
	for _, scale in ipairs(withScales and SCALES or {}) do
		if math.abs(scale - now) > 0.01 then
			for i, len in ipairs(SIZES) do
				local name = string.format("len-%04d-scale-%d", len, math.floor(scale * 100))
				table.insert(plan, Plan("golden", name, Bytes(i, len), scale))
			end
		end
	end
	return plan
end

function Shots.Combat()
	return { Plan("combat", "combat", "a strip in combat") }
end

local function Since(ms)
	return current and current.called_ms and ms - current.called_ms
end

hooksecurefunc("Screenshot", function()
	if not current or current.called_ms then
		return
	end
	current.called_ms = ns.Timing.Ms()
	current.date = date("%m%d%y_%H%M%S")
	current.unix = time()
	local strip = _G[ns.App.strip]
	current.strip_effective_scale = strip and strip:GetEffectiveScale()
	current.ui_parent_scale = UIParent:GetScale()
	if current.kind == "hide_after_call" and strip then
		strip:Hide()
	end
end)

local events = CreateFrame("Frame")
events:RegisterEvent("SCREENSHOT_SUCCEEDED")
events:RegisterEvent("SCREENSHOT_FAILED")
events:SetScript("OnEvent", function(_, event)
	local since = Since(ns.Timing.Ms())
	if not since then
		return
	end
	table.insert(current.events, { event = event, ms = since })
end)

if ActionStatus then
	ActionStatus:HookScript("OnShow", function()
		local since = Since(ns.Timing.Ms())
		if since and not current.status_shown_ms then
			current.status_shown_ms = since
		end
	end)
end

local function RestoreScale()
	if restoreScale and not InCombatLockdown() then
		UIParent:SetScale(restoreScale)
		restoreScale = nil
	end
end

-- A fight can start before the restore. The restore then waits for its end.
local regen = CreateFrame("Frame")
regen:RegisterEvent("PLAYER_REGEN_ENABLED")
regen:SetScript("OnEvent", RestoreScale)

local function SetScale(scale)
	if not scale or InCombatLockdown() then
		return false
	end
	restoreScale = UIParent:GetScale()
	UIParent:SetScale(scale)
	return true
end

local function Entry(step, id)
	return {
		kind = step.kind,
		name = step.name,
		mode = step.line,
		frame_id = id,
		time = Shots.TIME,
		payload = ns.Codec.Hex(step.payload),
		events = ns.Json.NewList(),
	}
end

-- Strip.lua draws the line of the saved variables, for the screen of this moment.
local function SetLine(mode)
	local width, height = GetPhysicalScreenSize()
	ns.Saved().stripLine = mode and { mode = mode, width = width, height = height } or nil
end

-- Takes the shot of one step, and calls `done(entry)` after the gap.
local function Shoot(step, id, done)
	local entry = Entry(step, id)
	local frame = ns.Codec.Frame(Shots.TIME, id, step.payload, Shots.TEST_KEY)
	local tries = 0
	local function Try()
		tries = tries + 1
		entry.scaled = SetScale(step.scale)
		SetLine(step.line)
		current = entry
		local started = ns.Strip.Show(frame, function(ok)
			SetLine(nil)
			RestoreScale()
			entry.ok = ok
			C_Timer.After(GAP, function()
				current = nil
				done(entry)
			end)
		end)
		if started then
			return
		end
		current = nil
		SetLine(nil)
		RestoreScale()
		if tries >= MAX_TRIES then
			entry.ok, entry.error = false, "the strip corner stayed busy"
			done(entry)
			return
		end
		C_Timer.After(RETRY, Try)
	end
	Try()
end

-- Shoots every step in order, and calls `done(entries)`.
function Shots.Run(plan, firstId, done)
	local entries, index = {}, 1
	local function Next()
		local step = plan[index]
		if not step then
			done(entries)
			return
		end
		Shoot(step, firstId + index - 1, function(entry)
			table.insert(entries, entry)
			index = index + 1
			Next()
		end)
	end
	Next()
end
