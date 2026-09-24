-- Spike: can an addon call Screenshot() with no key press, and does the image
-- keep a 4-pixel color strip exact? Results go to chat and to GRSpikeShotResults.

local CELL = 4
local CELLS = 200
-- Bit 2 = red, bit 1 = green, bit 0 = blue, as in the real strip.
local PATTERN = { 0, 1, 2, 3, 4, 5, 6, 7 }

local strip
local shot -- the screenshot in progress: { mode, started, blockedBy }

local function Say(msg)
	print("|cff66ccff[GR shot]|r " .. msg)
end

local function GetCVarValue(name)
	if C_CVar and C_CVar.GetCVar then return C_CVar.GetCVar(name) end
	return GetCVar(name)
end

local function SetCVarValue(name, value)
	if C_CVar and C_CVar.SetCVar then return C_CVar.SetCVar(name, value) end
	return SetCVar(name, value)
end

local function CellColor(v)
	return math.floor(v / 4) % 2, math.floor(v / 2) % 2, v % 2
end

-- Row 1 repeats the pattern. Row 2 repeats it backwards, so a shifted read fails.
local function EnsureStrip()
	if strip then return strip end
	strip = CreateFrame("Frame", nil, UIParent)
	strip:SetFrameStrata("TOOLTIP")
	strip:SetFrameLevel(10000)
	local _, physH = GetPhysicalScreenSize()
	strip:SetIgnoreParentScale(true)
	-- One UI unit is one physical pixel at this scale.
	strip:SetScale(768 / physH)
	strip:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 0, 0)
	strip:SetSize(CELLS * CELL, 2 * CELL)
	for row = 0, 1 do
		for i = 0, CELLS - 1 do
			local n = #PATTERN
			local v = row == 0 and PATTERN[i % n + 1] or PATTERN[n - i % n]
			local t = strip:CreateTexture(nil, "OVERLAY")
			t:SetSize(CELL, CELL)
			t:SetPoint("TOPLEFT", strip, "TOPLEFT", i * CELL, -row * CELL)
			t:SetColorTexture(CellColor(v))
		end
	end
	strip:Hide()
	return strip
end

local function Record(result)
	GRSpikeShotResults = GRSpikeShotResults or {}
	result.time = date("%Y-%m-%d %H:%M:%S")
	result.format = GetCVarValue("screenshotFormat")
	result.quality = GetCVarValue("screenshotQuality")
	result.screen = table.concat({ GetPhysicalScreenSize() }, "x")
	table.insert(GRSpikeShotResults, result)
end

local function TakeShot(mode)
	if shot then
		Say("A screenshot is already in progress.")
		return
	end
	EnsureStrip():Show()
	shot = { mode = mode }
	local function Fire()
		shot.started = GetTimePreciseSec()
		local before = debugprofilestop()
		local ok, err = pcall(Screenshot)
		shot.callMs = debugprofilestop() - before
		if not ok then
			Say("Screenshot() raised an error: " .. tostring(err))
			Record({ mode = mode, outcome = "error", err = tostring(err) })
			strip:Hide()
			shot = nil
		end
	end
	if mode == "key" then
		-- Inside the slash command, so it counts as a key press.
		Fire()
	else
		-- A timer breaks the link to the key press.
		C_Timer.After(0.25, Fire)
	end
	C_Timer.After(10, function()
		if shot and shot.mode == mode then
			Say("No screenshot event after 10 s.")
			Record({ mode = mode, outcome = "timeout", blockedBy = shot.blockedBy })
			strip:Hide()
			shot = nil
		end
	end)
end

-- Blizzard shows "Screen captured" through the ActionStatus frame.
-- Hide it for our shots only. A normal Print Screen still shows it.
local hideStatusUntil = 0
if ActionStatus then
	ActionStatus:HookScript("OnShow", function(self)
		if GetTime() < hideStatusUntil then self:Hide() end
	end)
end

local events = CreateFrame("Frame")
events:RegisterEvent("SCREENSHOT_SUCCEEDED")
events:RegisterEvent("SCREENSHOT_FAILED")
events:RegisterEvent("ADDON_ACTION_BLOCKED")
events:RegisterEvent("ADDON_ACTION_FORBIDDEN")
events:SetScript("OnEvent", function(_, event, ...)
	if event == "ADDON_ACTION_BLOCKED" or event == "ADDON_ACTION_FORBIDDEN" then
		local addon, func = ...
		if addon == "GRSpikeShot" then
			Say(event .. ": " .. tostring(func))
			if shot then shot.blockedBy = event .. ":" .. tostring(func) end
		end
		return
	end
	if not shot or not shot.started then return end
	hideStatusUntil = GetTime() + 3
	if ActionStatus and ActionStatus:IsShown() then ActionStatus:Hide() end
	local ms = (GetTimePreciseSec() - shot.started) * 1000
	local outcome = event == "SCREENSHOT_SUCCEEDED" and "ok" or "failed"
	Say(string.format("%s (%s): call %.1f ms, event after %.0f ms, format %s",
		outcome, shot.mode, shot.callMs or -1, ms, tostring(GetCVarValue("screenshotFormat"))))
	Record({ mode = shot.mode, outcome = outcome, callMs = shot.callMs, eventMs = ms })
	strip:Hide()
	shot = nil
end)

SLASH_GRSHOT1 = "/grshot"
SlashCmdList.GRSHOT = function(arg)
	arg = strtrim(arg or ""):lower()
	if arg == "" or arg == "timer" then
		TakeShot("timer")
	elseif arg == "key" then
		TakeShot("key")
	elseif arg == "png" or arg == "jpeg" or arg == "jpg" or arg == "tga" then
		SetCVarValue("screenshotFormat", arg == "jpg" and "jpeg" or arg)
		Say("screenshotFormat is now " .. tostring(GetCVarValue("screenshotFormat")))
	elseif arg == "show" then
		EnsureStrip():SetShown(not strip:IsShown())
	elseif arg == "status" then
		Say("screenshotFormat = " .. tostring(GetCVarValue("screenshotFormat"))
			.. ", screenshotQuality = " .. tostring(GetCVarValue("screenshotQuality"))
			.. ", screen = " .. table.concat({ GetPhysicalScreenSize() }, "x"))
	else
		Say("/grshot [timer|key|png|jpeg|show|status]")
	end
end
