-- Draws a frame as colored cells in the top-left corner and takes one screenshot
-- of it (SPEC.md 7.1). The strip shows only while the screenshot is taken.

local _, ns = ...

local Strip = {}
ns.Strip = Strip

local CELL = 4
local SHOT_DELAY = 0.1
local SHOT_TIMEOUT = 10

local frame
local textures = {}
local pending -- the callback of the screenshot in progress
-- True once our Screenshot() runs. The SCREENSHOT_* events also fire for the
-- screenshots of the player and of other addons, and those must not end our strip.
local shooting = false
local hideStatusUntil = 0

local function Build()
	frame = CreateFrame("Frame", "GnomishRelayStrip", UIParent)
	frame:SetFrameStrata("TOOLTIP")
	frame:SetFrameLevel(10000)
	frame:SetIgnoreParentScale(true)
	frame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 0, 0)
	frame:SetSize(ns.Codec.CELLS_PER_ROW * CELL, ns.Codec.MAX_ROWS * CELL)
	frame:Hide()
end

local function Texture(row, col)
	local index = (row - 1) * ns.Codec.CELLS_PER_ROW + col
	local t = textures[index]
	if not t then
		t = frame:CreateTexture(nil, "OVERLAY")
		t:SetSize(CELL, CELL)
		t:SetPoint("TOPLEFT", frame, "TOPLEFT", (col - 1) * CELL, -(row - 1) * CELL)
		textures[index] = t
	end
	return t
end

-- Bit 2 is red, bit 1 is green, bit 0 is blue.
local function Paint(t, cell)
	t:SetColorTexture(math.floor(cell / 4) % 2, math.floor(cell / 2) % 2, cell % 2)
	t:Show()
end

local function Draw(rows)
	for _, t in pairs(textures) do
		t:Hide()
	end
	for r, row in ipairs(rows) do
		for c, cell in ipairs(row) do
			Paint(Texture(r, c), cell)
		end
	end
	-- One UI unit is one physical pixel at this scale, so a cell is 4 pixels.
	local _, height = GetPhysicalScreenSize()
	frame:SetScale(768 / height)
	frame:Show()
end

local function Finish(ok)
	local done = pending
	pending = nil
	shooting = false
	hideStatusUntil = GetTime() + 1
	frame:Hide()
	if done then
		done(ok)
	end
end

function Strip.Busy()
	return pending ~= nil
end

-- Calls `done(ok)` once the screenshot has been taken or has failed.
function Strip.Show(frameBytes, done)
	if pending then
		return false
	end
	if not frame then
		Build()
	end
	pending = done
	Draw(ns.Codec.StripRows(frameBytes))
	C_Timer.After(SHOT_DELAY, function()
		-- The strip can have ended by a timeout in between. A shot now would have no strip.
		if pending ~= done then
			return
		end
		hideStatusUntil = GetTime() + SHOT_TIMEOUT
		shooting = true
		if not pcall(Screenshot) then
			Finish(false)
		end
	end)
	C_Timer.After(SHOT_TIMEOUT, function()
		if pending == done then
			Finish(false)
		end
	end)
	return true
end

-- The "Screen captured" text goes through ActionStatus. Only our shots hide it.
local function HideStatus()
	if ActionStatus and GetTime() < hideStatusUntil then
		ActionStatus:Hide()
	end
end

if ActionStatus then
	ActionStatus:HookScript("OnShow", HideStatus)
end

local events = CreateFrame("Frame")
events:RegisterEvent("SCREENSHOT_SUCCEEDED")
events:RegisterEvent("SCREENSHOT_FAILED")
events:SetScript("OnEvent", function(_, event)
	if pending and shooting then
		HideStatus()
		Finish(event == "SCREENSHOT_SUCCEEDED")
	end
end)
