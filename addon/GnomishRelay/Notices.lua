-- The notifications of terminal sessions (SPEC.md 10.4): the list, the filter, the chat
-- line, the sound, and the faster polls. NoticeFrames.lua draws the bell, the list, and
-- the toast. A notification carries no command, and nothing here sends a strip.

local _, ns = ...

local Notices = {}
ns.Notices = Notices

-- Enough to show nothing twice across a /reload, and small enough to stay small.
local SHOWN = 64
local SNIPPET = 120
local BUSY_POLL = 60
local OPEN_POLL = 180
local KINDS = { waiting = true, finished = true, failed = true }
local AGENTS = { claude = true, codex = true }
-- The bell of the chat line: the horn of a minimap event. The client has no bell texture.
Notices.ICON = ns.Atlases.BellText(14)
Notices.LINK = "gnomishrelaynotices"
-- The least length of finished work that shows, for each choice of the setting.
Notices.FINISHED = {
	{ value = "always", text = "Always", least = 0 },
	{ value = "over1", text = "Over 1 min", least = 60 },
	{ value = "over3", text = "Over 3 min", least = 180 },
	{ value = "never", text = "Never", least = math.huge },
}

local state = {
	-- The valid notices of the last live file, oldest first.
	all = {},
	-- The ones of `all` that pass the filter, newest first.
	list = {},
	busy = 0,
	open = 0,
	-- New notices whose sound and toast wait for the end of combat.
	held = {},
}

-- NoticeFrames.lua sets these.
Notices.OnChange = function() end
Notices.OnToast = function() end

function Notices.On()
	return ns.Store.db.notifyOn
end

function Notices.List()
	return state.list
end

function Notices.Sessions()
	return state.busy, state.open
end

function Notices.HasWaiting()
	for _, n in ipairs(state.list) do
		if n.kind == "waiting" then
			return true
		end
	end
	return false
end

-- Seconds since the bridge made the notice, from the clock of the bridge.
function Notices.Age(n)
	return n.age + (GetTime() - n.seenAt)
end

local function Valid(n)
	return type(n) == "table"
		and type(n.id) == "number"
		and type(n.at) == "number"
		and AGENTS[n.source]
		and KINDS[n.kind]
		and type(n.repo) == "string"
		and type(n.took) == "number"
		and n.took >= 0
		and type(n.text) == "string"
end

local function Least()
	for _, choice in ipairs(Notices.FINISHED) do
		if choice.value == ns.Store.db.notifyFinished then
			return choice.least
		end
	end
	return 60
end

-- `took` is 0 when the bridge saw no start of the turn, and counts as long work.
local function Passes(n)
	if n.kind == "waiting" then
		return true
	end
	local least = Least()
	if n.took == 0 then
		return least < math.huge
	end
	return n.took >= least
end

local function WasShown(id)
	for _, seen in ipairs(ns.Store.db.noticesShown) do
		if seen == id then
			return true
		end
	end
	return false
end

local function MarkShown(id)
	local shown = ns.Store.db.noticesShown
	table.insert(shown, id)
	if #shown > SHOWN then
		table.remove(shown, 1)
	end
end

local function IsContinuation(byte)
	return byte ~= nil and byte >= 0x80 and byte < 0xC0
end

-- Cuts at `most` bytes. The bridge doubles each "|" (S10), so a cut never ends inside
-- "||" or a character.
function Notices.Cut(text, most)
	if #text <= most then
		return text
	end
	local cut = text:sub(1, most)
	if IsContinuation(text:byte(most + 1)) then
		while IsContinuation(cut:byte(#cut)) do
			cut = cut:sub(1, -2)
		end
		cut = cut:sub(1, -2)
	end
	local pipes = #cut:match("|*$")
	if pipes % 2 == 1 then
		cut = cut:sub(1, -2)
	end
	return cut .. "..."
end

function Notices.Duration(seconds)
	if seconds < 60 then
		return seconds .. " s"
	elseif seconds < 3600 then
		return math.floor(seconds / 60) .. " min"
	end
	return math.floor(seconds / 3600) .. " h"
end

function Notices.Agent(n)
	return ns.Relay.AgentName(n.source)
end

-- "[Claude · gnomish-relay]". The repo is empty for a session outside any folder.
local function Tag(n)
	if n.repo == "" then
		return "[" .. Notices.Agent(n) .. "]"
	end
	return string.format("[%s · %s]", Notices.Agent(n), n.repo)
end

local function DoneText(n)
	if n.took == 0 then
		return n.kind == "failed" and "Failed" or "Finished"
	elseif n.kind == "failed" then
		return "Failed after " .. Notices.Duration(n.took)
	end
	return "Finished in " .. Notices.Duration(n.took)
end

-- A link of the addon, never a chat that takes an answer. A click opens the list.
local function Line(text)
	DEFAULT_CHAT_FRAME:AddMessage(
		string.format("|cff%s%s |H%s|h%s|h|r", ns.Store.db.whisperColor, Notices.ICON, Notices.LINK, text)
	)
end

-- "3 agents finished: a, b, c", or "3 agents done (1 failed): a, b, c".
local function GroupText(done)
	local repos, failed = {}, 0
	for _, n in ipairs(done) do
		table.insert(repos, n.repo ~= "" and n.repo or Notices.Agent(n))
		if n.kind == "failed" then
			failed = failed + 1
		end
	end
	local list = table.concat(repos, ", ")
	if failed == 0 then
		return string.format("%d agents finished: %s", #done, list)
	end
	return string.format("%d agents done (%d failed): %s", #done, failed, list)
end

local function ChatLines(new)
	local done = {}
	for _, n in ipairs(new) do
		if n.kind == "waiting" then
			Line(string.format("%s Waiting for you: %s", Tag(n), Notices.Cut(n.text, SNIPPET)))
		else
			table.insert(done, n)
		end
	end
	if #done == 1 then
		local n = done[1]
		Line(string.format("%s %s: %s", Tag(n), DoneText(n), Notices.Cut(n.text, SNIPPET)))
	elseif #done > 1 then
		Line(GroupText(done))
	end
end

local function InList(id)
	for _, n in ipairs(state.list) do
		if n.id == id then
			return n
		end
	end
end

-- The newest waiting notice of `new` that the list still holds.
local function NewestWaiting(new)
	for i = #new, 1, -1 do
		local n = InList(new[i].id)
		if n and n.kind == "waiting" then
			return n
		end
	end
end

-- One sound for each poll: the Battle.net toast for a waiting notice, else the whisper.
local function SoundAndToast(new)
	local db = ns.Store.db
	local still = {}
	for _, n in ipairs(new) do
		if InList(n.id) then
			table.insert(still, n)
		end
	end
	if #still == 0 then
		return
	end
	local waiting = NewestWaiting(still)
	if db.notifySound then
		PlaySound(waiting and SOUNDKIT.UI_BNET_TOAST or SOUNDKIT.TELL_MESSAGE)
	end
	if db.notifyToast and waiting then
		Notices.OnToast(waiting)
	end
end

local function Alert(new)
	if ns.Store.db.notifyChat then
		ChatLines(new)
	end
	if InCombatLockdown() then
		for _, n in ipairs(new) do
			table.insert(state.held, n)
		end
		return
	end
	SoundAndToast(new)
end

-- The sound and the toast come after combat, and only for notices still in the list.
function Notices.CombatEnded()
	local held = state.held
	state.held = {}
	if Notices.On() then
		SoundAndToast(held)
	end
end

local function Cleared(n)
	return n.id <= ns.Store.db.noticesCleared
end

-- The notices that pass the filter, newest first.
local function Filtered()
	local list = {}
	for _, n in ipairs(state.all) do
		if Passes(n) and not Cleared(n) then
			table.insert(list, 1, n)
		end
	end
	return list
end

-- A notice that the filter hides is marked as shown too, so a new setting never alerts old work.
local function IsNew(n)
	if WasShown(n.id) then
		return false
	end
	MarkShown(n.id)
	return Passes(n) and not Cleared(n)
end

-- `notices` is the table of the live file, and `bodyNow` the `now` of the body in the
-- same slot, so a clock difference between the desktop and the game has no effect.
function Notices.Apply(notices, bodyNow)
	local t = type(notices) == "table" and notices or {}
	state.busy = tonumber(t.busy) or 0
	state.open = tonumber(t.open) or 0
	state.all = {}
	local new = {}
	for _, n in ipairs(type(t.list) == "table" and t.list or {}) do
		if Valid(n) then
			n.age = math.max(0, (bodyNow or n.at) - n.at)
			n.seenAt = GetTime()
			table.insert(state.all, n)
			if IsNew(n) then
				table.insert(new, n)
			end
		end
	end
	state.list = Filtered()
	if Notices.On() and #new > 0 then
		Alert(new)
	end
	Notices.OnChange()
end

-- A new Finished work setting changes the list at once, with no alert.
function Notices.Refilter()
	state.list = Filtered()
	Notices.OnChange()
end

function Notices.Drop()
	state.all = {}
	state.list = {}
	state.busy = 0
	state.open = 0
	Notices.OnChange()
end

-- A cleared notice never comes back: ids only grow (SPEC.md 10.3).
function Notices.Clear()
	local db = ns.Store.db
	for _, n in ipairs(state.list) do
		db.noticesCleared = math.max(db.noticesCleared, n.id)
	end
	state.list = {}
	Notices.OnChange()
end

-- Seconds to the next poll while a terminal session is open, or nil.
-- Only the bridge ends a stale turn, so an offline bridge leaves `busy` as it was.
function Notices.PollEvery()
	if not Notices.On() or not ns.Transport.Online() then
		return nil
	elseif state.busy > 0 then
		return BUSY_POLL
	elseif state.open > 0 then
		return OPEN_POLL
	end
end
