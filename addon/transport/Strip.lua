-- Draws a frame as colored cells in the top-left corner and takes one screenshot
-- of it (SPEC.md 7.1). With a line from the bridge, the cells are 1 or 2 pixels (7.1.3). The strip shows only while the screenshot is taken.
-- Every app of the shared transport draws in the same corner, so the apps take turns
-- through one shared global. models/corner.qnt checks the rules of the turns.

-- Every copy of the shared transport must find the corner under one name, so only _G
-- can reach it.
--# selene: allow(global_usage)

local _, ns = ...

local Strip = {}
ns.Strip = Strip

local CELL = 4
local SHOT_DELAY = 0.1
local SHOT_TIMEOUT = 10
-- A new shape of the value needs a new name: an older copy of this file can run in
-- another addon at the same time.
local CORNER = "GnomishStripCorner"
-- Longer than SHOT_TIMEOUT, so the hold ends only for an addon that stopped with an error.
local HOLD = 12
-- The event of our own shot can still come after the strip ended. During the tail it
-- finds no strip of another app to end.
local TAIL = 2
-- A waiter asks again every second, so an older mark belongs to an app that stopped.
local WAIT_FRESH = 3
-- An honest wait is at most 15 s: our tail, one strip and tail of another app, and a Tick.
local BLOCK_AFTER = 30

local frame
local textures = {}
local lineTextures = {}
-- The frame id of the last line. A strip with the same id is a retry, so the line did
-- not reach the bridge (SPEC.md 7.1.3).
local lastLineId
local failedLineId
local lineFailures = 0
local LINE_FAILURES_OFF = 2
local pending -- the callback of the screenshot in progress
-- True once our Screenshot() runs. The SCREENSHOT_* events also fire for the
-- screenshots of the player and of other addons, and those must not end our strip.
local shooting = false
local hideStatusUntil = 0
-- True from our take of the corner to the end of our strip.
local turn = false
-- Our own wait for the corner. The shared copy of it is readable by every addon.
local wait

local function Me()
	return ns.App.strip
end

-- rawget and rawset skip any metatable that another addon puts on the value.
local function Corner()
	local corner = rawget(_G, CORNER)
	if type(corner) ~= "table" then
		corner = {}
		rawset(_G, CORNER, corner)
	end
	if type(rawget(corner, "waits")) ~= "table" then
		rawset(corner, "waits", {})
	end
	return corner
end

-- Any other addon can write the value, so a field of a wrong type counts as missing.
local function Number(t, key)
	local value = type(t) == "table" and rawget(t, key)
	return type(value) == "number" and value or nil
end

local function Holds(corner, now)
	return rawget(corner, "holder") == Me() and (Number(corner, "endsAt") or 0) > now
end

local function HeldByOther(corner, now)
	local holder = rawget(corner, "holder")
	return holder ~= nil and holder ~= Me() and (Number(corner, "endsAt") or 0) > now
end

local function Waiting(now)
	return wait ~= nil and now - wait.at < WAIT_FRESH
end

-- The turn rule: an app that waits longer goes first.
local function OtherWaitsLonger(corner, now)
	local waits = rawget(corner, "waits")
	for who, mark in pairs(waits) do
		local since, at = Number(mark, "since"), Number(mark, "at")
		local fresh = who ~= Me() and since and at and now - at < WAIT_FRESH
		if fresh and not (Waiting(now) and wait.since <= since) then
			return true
		end
	end
	return false
end

local function MarkWait(corner, now)
	if not Waiting(now) then
		wait = { since = now }
	end
	wait.at = now
	rawset(rawget(corner, "waits"), Me(), { since = wait.since, at = now })
	if now - wait.since >= BLOCK_AFTER then
		ns.Health.Corner(false)
	end
end

local function Take(corner, now)
	rawset(corner, "holder", Me())
	rawset(corner, "endsAt", now + HOLD)
	rawset(rawget(corner, "waits"), Me(), nil)
	wait = nil
	ns.Health.Corner(true)
end

local function Release(now)
	local corner = Corner()
	if rawget(corner, "holder") == Me() then
		rawset(corner, "endsAt", now + TAIL)
	end
end

-- Returns whether this app holds the corner now. If not, it waits in line.
function Strip.TakeTurn()
	local now = GetTime()
	local corner = Corner()
	if turn and Holds(corner, now) then
		return true
	end
	turn = not HeldByOther(corner, now) and not OtherWaitsLonger(corner, now)
	if turn then
		Take(corner, now)
	else
		MarkWait(corner, now)
	end
	return turn
end

local function Build()
	frame = CreateFrame("Frame", ns.App.strip, UIParent)
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

local function LineTexture(index)
	local t = lineTextures[index]
	if not t then
		t = frame:CreateTexture(nil, "OVERLAY")
		-- The snap rounds a float error of the scale to the nearest pixel.
		t:SetSnapToPixelGrid(true)
		lineTextures[index] = t
	end
	return t
end

local function PaintLine(rows, size)
	local width = ns.Codec.LINE_CELLS_PER_ROW
	for r, row in ipairs(rows) do
		for c, color in ipairs(row) do
			local t = LineTexture((r - 1) * width + c)
			t:SetSize(size, size)
			t:SetPoint("TOPLEFT", frame, "TOPLEFT", (c - 1) * size, -(r - 1) * size)
			t:SetColorTexture(color[1] / 255, color[2] / 255, color[3] / 255)
			t:Show()
		end
	end
end

local function PaintStrip(rows)
	for r, row in ipairs(rows) do
		for c, cell in ipairs(row) do
			Paint(Texture(r, c), cell)
		end
	end
end

local function HideAll()
	for _, t in pairs(textures) do
		t:Hide()
	end
	for _, t in pairs(lineTextures) do
		t:Hide()
	end
end

local function FrameId(frameBytes)
	local hi, lo = frameBytes:byte(8, 9)
	return (hi or 0) * 256 + (lo or 0)
end

-- Any addon can write the saved variables, so a value of a wrong shape counts as none.
local function SavedLine()
	local line = ns.Saved().stripLine
	if type(line) ~= "table" or not ns.Codec.LINE_MODES[line.mode] then
		return nil
	end
	local width, height = GetPhysicalScreenSize()
	if line.width ~= width or line.height ~= height then
		return nil
	end
	return line.mode
end

-- A retry of a line draws the old strip, which always reads.
local function IsLineRetry(id)
	if id == 0 or id ~= lastLineId then
		return false
	end
	if id ~= failedLineId then
		failedLineId = id
		lineFailures = lineFailures + 1
	end
	return true
end

-- The mode of the line for this frame, or nil for the old strip (SPEC.md 7.1.3).
local function LineMode(frameBytes)
	local mode = lineFailures < LINE_FAILURES_OFF and SavedLine()
	if not mode then
		return nil
	end
	local id = FrameId(frameBytes)
	if IsLineRetry(id) then
		return nil
	end
	lastLineId = id
	return mode
end

-- `line` comes from the slot body of the bridge. Nil removes the line.
function Strip.TakeLine(line)
	local valid = type(line) == "table"
		and ns.Codec.LINE_MODES[line.mode]
		and type(line.width) == "number"
		and type(line.height) == "number"
	ns.Saved().stripLine = valid and { mode = line.mode, width = line.width, height = line.height } or nil
end

local function Draw(frameBytes)
	HideAll()
	local mode = LineMode(frameBytes)
	if mode then
		PaintLine(ns.Codec.LineRows(frameBytes, mode), ns.Codec.LINE_MODES[mode].size)
	else
		PaintStrip(ns.Codec.StripRows(frameBytes))
	end
	-- One UI unit is one physical pixel at this scale, as in PixelUtil.
	local _, height = GetPhysicalScreenSize()
	frame:SetScale(768 / height)
	frame:Show()
end

-- The corner goes before the callback, so an error in the callback keeps no corner.
local function Finish(ok)
	local done = pending
	pending = nil
	shooting = false
	turn = false
	hideStatusUntil = GetTime() + 1
	frame:Hide()
	Release(GetTime())
	if done then
		done(ok)
	end
end

function Strip.Busy()
	return pending ~= nil
end

-- Calls `done(ok)` once the screenshot has been taken or has failed. Returns false,
-- and draws nothing, while another strip is in progress or another app holds the corner.
function Strip.Show(frameBytes, done)
	if pending or not Strip.TakeTurn() then
		return false
	end
	if not frame then
		Build()
	end
	pending = done
	Draw(frameBytes)
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

-- The "Screen captured" text goes through ActionStatus. Only our shots hide it. Each
-- app hooks it, and each hook hides the text of its own shots.
local function HideStatus()
	if ActionStatus and GetTime() < hideStatusUntil then
		ActionStatus:Hide()
	end
end

if ActionStatus then
	ActionStatus:HookScript("OnShow", HideStatus)
end

-- An event carries no owner. While we hold the corner and wait for our shot, no other
-- app shoots, so the event is ours or the player's.
local function OurShot()
	return pending and shooting and rawget(Corner(), "holder") == Me()
end

local events = CreateFrame("Frame")
events:RegisterEvent("SCREENSHOT_SUCCEEDED")
events:RegisterEvent("SCREENSHOT_FAILED")
events:SetScript("OnEvent", function(_, event)
	if OurShot() then
		HideStatus()
		Finish(event == "SCREENSHOT_SUCCEEDED")
	end
end)
