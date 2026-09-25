-- A small fake of the WoW API, enough to run the addon outside the game.
-- It returns a `wow` table that tests use to drive time and look inside.
-- `api` is addon/tests/api.lua: the real API of the client. An object of the fake
-- refuses every method or child key that its real kind and template do not have.

local api = ...

local wow = {
	now = 1000,
	epoch = 1790211079,
	timers = {},
	printed = {},
	sounds = {},
	cvars = {},
	loaded = {},
	body = nil,
	restore = nil,
	live = nil,
	slotsInstalled = true,
	shotsBlocked = false,
	shots = {},
	reloads = 0,
	combat = false,
	textures = {},
	frames = {},
}

local Object = {}
local methods = {}

-- A known capitalized field with no fake is a no-op that can be called or indexed
-- further, like `frame.PortraitContainer.portrait` or `frame:SetFrameStrata()`. WoW
-- names methods and child frames that way. A lowercase field is data, and stays nil.
local Nothing = setmetatable({}, {
	__call = function() end,
	__index = function(self)
		return self
	end,
})

local function AddWidget(names, widget, seen)
	local w = api.widgets[widget]
	if not w or seen[widget] then
		return
	end
	seen[widget] = true
	for _, m in ipairs(w.methods) do
		names[m] = true
	end
	for _, parent in ipairs(w.inherits) do
		AddWidget(names, parent, seen)
	end
end

local namesOf = {}

-- An intrinsic frame such as ScrollingMessageFrame is in `templates` under its kind.
local function Names(kind, template)
	local id = kind .. "/" .. tostring(template)
	if namesOf[id] then
		return namesOf[id]
	end
	local names, seen = {}, {}
	AddWidget(names, kind, seen)
	local t = api.templates[template or kind]
	if t then
		AddWidget(names, t.base, seen)
		for _, n in ipairs(t.names) do
			names[n] = true
		end
	end
	namesOf[id] = names
	return names
end

Object.__index = function(o, key)
	if not key:match("^%u") then
		return nil
	end
	if not rawget(o, "names")[key] then
		error(string.format("%s has no %s in WoW Forever %s", o.template or o.kind, key, api.build), 2)
	end
	return methods[key] or Nothing
end

local function New(kind, name, parent, template)
	local o = setmetatable({
		kind = kind,
		template = template,
		names = Names(kind, template),
		name = name,
		parent = parent,
		shown = true,
		scripts = {},
	}, Object)
	if name then
		_G[name] = o
	end
	table.insert(wow.frames, o)
	return o
end

function methods:Show()
	self.shown = true
	if self.scripts.OnShow then
		self.scripts.OnShow(self)
	end
end

function methods:Hide()
	self.shown = false
end

function methods:SetShown(shown)
	if shown then
		self:Show()
	else
		self:Hide()
	end
end

function methods:IsShown()
	return self.shown
end

function methods:IsVisible()
	local o = self
	while o do
		if not o.shown then
			return false
		end
		o = o.parent
	end
	return true
end

function methods:SetScript(name, fn)
	self.scripts[name] = fn
end

function methods:GetScript(name)
	return self.scripts[name]
end

function methods:HookScript(name, fn)
	local old = self.scripts[name]
	self.scripts[name] = function(...)
		if old then
			old(...)
		end
		fn(...)
	end
end

function methods:RegisterEvent(event)
	self.events = self.events or {}
	self.events[event] = true
end

function methods:CreateTexture()
	local t = New("Texture", nil, self)
	table.insert(wow.textures, t)
	return t
end

function methods:CreateFontString(name)
	return New("FontString", name, self)
end

function methods:SetColorTexture(r, g, b)
	self.color = { r, g, b }
end

function methods:SetPoint(_, _, _, x, y)
	self.x, self.y = x or 0, y or 0
end

function methods:SetText(text)
	self.text = text
end

function methods:GetText()
	return self.text
end

function methods:AddMessage(text)
	self.lines = self.lines or {}
	table.insert(self.lines, text)
end

function methods:Clear()
	self.lines = {}
end

function methods:SetValue(v)
	self.value = v
end

function methods:Click()
	self.scripts.OnClick(self, "LeftButton")
end

function wow.Fire(event, ...)
	for _, f in ipairs(wow.frames) do
		if f.events and f.events[event] and f.scripts.OnEvent then
			f.scripts.OnEvent(f, event, ...)
		end
	end
end

-- Moves the clock forward and runs every timer that comes due.
function wow.Advance(seconds)
	local stop = wow.now + seconds
	while true do
		table.sort(wow.timers, function(a, b)
			return a.at < b.at
		end)
		local due = wow.timers[1]
		if not due or due.at > stop then
			break
		end
		table.remove(wow.timers, 1)
		wow.now = due.at
		if not due.cancelled then
			due.fn()
			if due.every then
				due.at = wow.now + due.every
				table.insert(wow.timers, due)
			end
		end
	end
	wow.now = stop
end

-- The saved variables file as WoW writes it at /reload: `name = { ["key"] = value, ... }`.
function wow.Save(name)
	local out = {}
	local function Write(value, indent)
		if type(value) == "table" then
			table.insert(out, "{\n")
			local keys = {}
			for k in pairs(value) do
				table.insert(keys, k)
			end
			table.sort(keys, function(x, y)
				return tostring(x) < tostring(y)
			end)
			for _, k in ipairs(keys) do
				local key = type(k) == "number" and "[" .. k .. "]" or string.format("[%q]", k)
				table.insert(out, indent .. "\t" .. key .. " = ")
				Write(value[k], indent .. "\t")
				table.insert(out, ",\n")
			end
			table.insert(out, indent .. "}")
		elseif type(value) == "string" then
			table.insert(out, string.format("%q", value))
		else
			table.insert(out, tostring(value))
		end
	end
	table.insert(out, name .. " = ")
	Write(_G[name], "")
	table.insert(out, "\n")
	return table.concat(out)
end

-- The cells of the strip on screen, by row, from the colors of the visible textures.
local function StripCells()
	local rows = {}
	for _, t in ipairs(wow.textures) do
		if t.parent.name == "GnomishRelayStrip" and t:IsVisible() and t.color then
			local row, col = -t.y / 4 + 1, t.x / 4 + 1
			rows[row] = rows[row] or {}
			rows[row][col] = t.color[1] * 4 + t.color[2] * 2 + t.color[3]
		end
	end
	return rows
end

UIParent = New("Frame", "UIParent")
DEFAULT_CHAT_FRAME = New("ScrollingMessageFrame", "ChatFrame1")
function DEFAULT_CHAT_FRAME:AddMessage(text, r, g, b)
	table.insert(wow.printed, text)
end
GameTooltip = New("GameTooltip", "GameTooltip")
UIErrorsFrame = New("MessageFrame", "UIErrorsFrame")
ActionStatus = New("Frame", "ActionStatus")
UISpecialFrames = {}
SOUNDKIT = { TELL_MESSAGE = 3081 }
ChatFontNormal, GameFontNormal = {}, {}
SlashCmdList = {}

function CreateFrame(kind, name, parent, template)
	return New(kind, name, parent, template)
end

function print(...)
	table.insert(wow.printed, table.concat({ ... }, " "))
end

function GetTime()
	return wow.now
end

function time()
	return wow.epoch + math.floor(wow.now - 1000)
end

function strtrim(s)
	return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end

function GetBuildInfo()
	return "1.60.1", "70009", "Sep 24 2026", 16001
end

function GetPhysicalScreenSize()
	return 1280, 720
end

function SetCVar(name, value)
	wow.cvars[name] = value
end

function InCombatLockdown()
	return wow.combat
end

function ReloadUI()
	wow.reloads = wow.reloads + 1
end

function PlaySound(id)
	table.insert(wow.sounds, id)
end

function hooksecurefunc(name, fn)
	local old = _G[name] or function() end
	_G[name] = function(...)
		local r = { old(...) }
		fn(...)
		return unpack(r)
	end
end

function Screenshot()
	if wow.shotsBlocked then
		C_Timer.After(0.4, function()
			wow.Fire("SCREENSHOT_FAILED")
		end)
		return
	end
	table.insert(wow.shots, StripCells())
	C_Timer.After(0.4, function()
		wow.Fire("SCREENSHOT_SUCCEEDED")
	end)
end

C_Timer = {}

function C_Timer.After(delay, fn)
	table.insert(wow.timers, { at = wow.now + delay, fn = fn })
end

function C_Timer.NewTicker(every, fn)
	local timer = { at = wow.now + every, fn = fn, every = every }
	table.insert(wow.timers, timer)
	return {
		Cancel = function()
			timer.cancelled = true
		end,
	}
end

C_AddOns = {}

function C_AddOns.IsAddOnLoaded(name)
	return wow.loaded[name] == true
end

function C_AddOns.EnableAddOn() end

-- A slot runs the body and the restore file that the test put there, one time per UI session.
function C_AddOns.LoadAddOn(name)
	if not wow.slotsInstalled then
		return false, "MISSING"
	end
	if not wow.loaded[name] then
		wow.loaded[name] = true
		if wow.body then
			assert(loadstring(wow.body))()
		end
		if wow.restore then
			assert(loadstring(wow.restore))()
		end
		if wow.live then
			assert(loadstring(wow.live))()
		end
	end
	return true
end

return wow
