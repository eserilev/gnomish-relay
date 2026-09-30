-- The relay on top of the shared Messages.lua: chats, sessions, folders, deletes, the
-- restore bundle, the live file, and the permission answers (SPEC.md 7, 9.3, 9.6, and 9.9).

local _, ns = ...

local Messages = ns.Messages
local Transport = {}
ns.Transport = Transport

local LIST_CHAT = "relay"
local FOLDER_CHAT = "folders"
local SETTINGS_CHAT = "settings"

-- The bridge writes one of these as the first line of each run, and no agent line
-- can start with "Level:" (SPEC.md 9.3). The config can lower the level of the chat.
local LEVELS = {
	["Level: ask"] = "ask",
	["Level: ask (config)"] = "ask (config)",
	["Level: auto-edit"] = "auto-edit",
	["Level: auto-edit (config)"] = "auto-edit (config)",
	["Level: full-auto"] = "full-auto",
}

-- The bridge writes this line right after the level line while a run waits for the
-- desktop, and no agent line can start with "Desktop:" (SPEC.md 6.6.3).
local DESKTOP_STATES = {
	wait = "Approve on your desktop",
	approved = "Approved on your desktop",
	denied = "Denied on your desktop",
	none = "No answer on your desktop",
}
local DESKTOP_HOW = { dialog = true, command = true }
-- The bridge writes this line as the only line of a message that waits for other chats,
-- and no agent line can start with "Waiting:" (SPEC.md 8.2).
local WAITING = "Waiting: "
local RAISE_LEVELS = { ["auto-edit"] = true, ["full-auto"] = true }
local DESKTOP_POLL = 5
local DESKTOP_POLLS = 24
-- A popup waits for the next poll, so a working run polls often. It costs 4 slots a minute.
local WORKING_POLL = 15
-- Enough to whisper once per request across a /reload, and small enough to stay small.
local WHISPERED = 16

local state = {
	working = {},
	-- The id of the open request of each list, by its chat.
	listing = {},
	-- The permission requests of the last live file, and the ones this session answered.
	requests = {},
	answered = {},
	-- The polls so far of each open desktop request, by its id.
	desktopPolls = {},
	lowSlotsTold = false,
}

Transport.OnReply = function() end
-- A new desktop request of a chat: `notice` has `id`, `how`, and `raise`.
Transport.OnDesktop = function() end

Transport.Init = Messages.Init
Transport.Tick = Messages.Tick
Transport.Poll = Messages.Poll
Transport.SlotsLeft = Messages.SlotsLeft
Transport.Online = Messages.Online
Transport.Bridge = Messages.Bridge
Transport.NeedsReload = Messages.NeedsReload
Transport.Problem = Messages.Problem
Transport.Stats = Messages.Stats
Transport.NextPollIn = Messages.NextPollIn
Transport.Delivery = Messages.Delivery
Transport.Room = Messages.Room
Transport.Send = Messages.Send

function Transport.Working(chatId)
	return state.working[chatId]
end

local function Fields(chat, message)
	local flags = { "agent=" .. chat.agent, "level=" .. chat.mode }
	if chat.fresh then
		table.insert(flags, "n")
	end
	-- The bridge makes the folder only now, so a chat that never sends leaves none.
	if chat.fresh and chat.newFolder then
		table.insert(flags, "mkdir=1")
	end
	if message.attach then
		table.insert(flags, "attach=" .. chat.attach)
	end
	return chat.cwd, table.concat(flags, ";"), chat.name
end

-- The list chats have no messages: their records are the replies to a list control.
local function Find(chatId, id)
	if chatId == LIST_CHAT or chatId == FOLDER_CHAT or chatId == SETTINGS_CHAT then
		return nil
	end
	local chat = ns.Store.Chat(chatId)
	return chat, chat and ns.Store.Message(chat, id)
end

-- The first message of a resumed chat has no text. It asks the bridge to attach the
-- session to the chat.
function Transport.Attach(chat)
	local message = ns.Store.AddMessage(chat, "")
	message.attach = true
	return Messages.Queue(chat, message)
end

local function List(chat, flags)
	state.listing[chat] = Messages.NewId()
	Messages.ControlLater(chat, state.listing[chat], flags)
	Messages.StartPolls()
end

-- The list is the reply to a message of the chat "relay", one session per line.
function Transport.ListSessions()
	List(LIST_CHAT, "list")
end

-- The list is the reply to a message of the chat "folders": the folder tree (Folders.lua).
function Transport.ListFolders()
	List(FOLDER_CHAT, "list=folders")
end

-- The list is the reply to a message of the chat "settings" (BridgeSettings.lua).
function Transport.ListSettings()
	List(SETTINGS_CHAT, "list=settings")
end

-- The bridge removes the rule before it answers the list in the same strip.
function Transport.RemoveRule(id)
	Messages.Control(SETTINGS_CHAT, 0, "rule=remove:" .. id)
	Transport.ListSettings()
end

function Transport.ListingSessions()
	return state.listing[LIST_CHAT] ~= nil
end

function Transport.ListingFolders()
	return state.listing[FOLDER_CHAT] ~= nil
end

local function Listing()
	return next(state.listing) ~= nil
end

function Transport.Delete(chat)
	state.working[chat.id] = nil
	ns.Store.DeleteChat(chat.id)
	Messages.Hello()
	Messages.ShowNextStrip()
	Messages.OnChange()
end

function Transport.Stop(chat)
	Messages.Control(chat.id, 0, "stop")
end

-- A delete rides on each strip until one goes out while the bridge is online. A strip
-- can go out while the bridge is off.
local function Forgets()
	local records = {}
	for _, chatId in ipairs(ns.Store.db.forget) do
		table.insert(records, { chat = chatId, id = 0, flags = "d" })
	end
	return records
end

local function Forgotten(records)
	if not Messages.Online() then
		return
	end
	local forget = ns.Store.db.forget
	for _, record in ipairs(records) do
		for i = #forget, 1, -1 do
			if forget[i] == record.chat then
				table.remove(forget, i)
			end
		end
	end
end

local function Field(text)
	return text ~= nil and text or ""
end

local function Cells(line)
	local f = {}
	for part in (line .. "\t"):gmatch("([^\t]*)\t") do
		table.insert(f, part)
	end
	return f
end

local function Lines(text)
	return (tostring(text) .. "\n"):gmatch("([^\n]*)\n")
end

-- Agent, session, age, active, chat, folder, folder name, title (SPEC.md 9.6).
local function ParseSessions(text)
	local rows = {}
	for line in Lines(text) do
		local f = Cells(line)
		local session, age = f[2], tonumber(f[3])
		local valid = ns.Codec.IsValidId(f[1]) and age and type(session) == "string"
		if valid and #session <= 64 and not session:find("[^%w_-]") then
			table.insert(rows, {
				agent = f[1],
				session = session,
				age = age,
				active = f[4] == "1",
				chat = f[5] ~= "" and f[5] or nil,
				folder = Field(f[6]),
				repo = Field(f[7]),
				title = Field(f[8]),
			})
		end
	end
	return rows
end

-- The saved variables key, the field, and the parser of the reply of each list chat.
-- The folder tree stays text: a parsed tree has loops, and saved variables cannot.
local LISTS = {
	[LIST_CHAT] = { key = "sessions", field = "rows", Parse = ParseSessions },
	[FOLDER_CHAT] = { key = "folders", field = "text", Parse = tostring },
	[SETTINGS_CHAT] = { key = "settings", field = "text", Parse = tostring },
}

-- An older list that comes after a newer one changes nothing. Returns whether the
-- record is final, so the addon reports it as read.
local function ApplyList(r)
	local list = LISTS[r.chat]
	if not list or r.status == "working" then
		return false
	end
	local db = ns.Store.db
	local last = db[list.key]
	if last and last.id and last.id >= r.id then
		return true
	end
	if r.id == state.listing[r.chat] then
		state.listing[r.chat] = nil
	end
	local entry = { id = r.id, at = time() }
	if r.status == "error" then
		entry[list.field] = last and last[list.field]
		entry.error = r.text
	else
		entry[list.field] = list.Parse(r.text)
	end
	db[list.key] = entry
	return true
end

local function ApplyStatus(chat, id, status)
	chat.fresh = nil
	chat.newFolder = nil
	local working = state.working[chat.id]
	if status == "working" then
		if not working or working.id ~= id then
			state.working[chat.id] = { id = id, since = GetTime() }
		end
	elseif working and working.id == id then
		state.working[chat.id] = nil
	end
end

-- The reply to an attach brings the last exchange, and gets no whisper.
local function ApplyReply(chat, id, status, text)
	local message = ns.Store.Message(chat, id)
	ns.Store.AddReply(chat, id, text, status)
	if not message.attach then
		Transport.OnReply(chat, { id = id, status = status, text = text })
	end
end

local function ApplyRestore(restore)
	local db = ns.Store.db
	if type(restore) ~= "table" or restore.token ~= db.token or db.restored then
		return
	end
	ns.Store.MergeChats(type(restore.chats) == "table" and restore.chats or {})
	db.restored = true
	Messages.Hello()
end

local KINDS = { allow_once = true, allow_always = true, reject_once = true, reject_always = true }

local function ValidRequest(r)
	if type(r) ~= "table" or not ns.Codec.IsValidId(r.request) or type(r.text) ~= "string" then
		return false
	end
	if not ns.Store.Chat(r.chat) or type(r.options) ~= "table" then
		return false
	end
	for _, option in ipairs(r.options) do
		if type(option) ~= "table" or not ns.Codec.IsValidId(option.id) or not KINDS[option.kind] then
			return false
		end
	end
	return #r.options > 0
end

-- `Desktop: <state> <id> <how>`, and ` raise <level>` for a raise. Nil for any other line.
local function ParseDesktop(line)
	local waiting, id, how, rest = tostring(line):match("^Desktop: (%l+) (%x+) (%l+)(.*)$")
	if not DESKTOP_STATES[waiting] or #id ~= 12 or not DESKTOP_HOW[how] then
		return nil
	end
	local raise = rest:match("^ raise ([%l-]+)$")
	if rest ~= "" and not RAISE_LEVELS[raise] then
		return nil
	end
	return { state = waiting, id = id, how = how, raise = raise }
end

local function Whispered(id)
	local db = ns.Store.db
	for _, seen in ipairs(db.desktopWhispered) do
		if seen == id then
			return true
		end
	end
	table.insert(db.desktopWhispered, id)
	if #db.desktopWhispered > WHISPERED then
		table.remove(db.desktopWhispered, 1)
	end
	return false
end

local function QueuedLine(lines)
	local first = lines[1]
	if type(first) == "string" and first:sub(1, #WAITING) == WAITING then
		return first
	end
	return nil
end

-- The desktop line has a fixed place: right after the level line. The row shows its
-- meaning, not the raw line.
local function ApplyDesktop(chat, working, lines)
	local at = LEVELS[lines[1]] and 2 or 1
	local notice = ParseDesktop(lines[at])
	working.desktop = notice
	if not notice then
		return
	end
	lines[at] = DESKTOP_STATES[notice.state]
	if notice.state == "wait" and not Whispered(notice.id) then
		Transport.OnDesktop(chat, notice)
	end
end

local function CountDesktopPolls()
	local polls = {}
	for _, working in pairs(state.working) do
		local notice = working.desktop
		if notice and notice.state == "wait" then
			polls[notice.id] = (state.desktopPolls[notice.id] or 0) + 1
		end
	end
	state.desktopPolls = polls
end

-- Fast polls while a desktop request waits, at most DESKTOP_POLLS for each request.
local function PollEvery()
	for _, count in pairs(state.desktopPolls) do
		if count < DESKTOP_POLLS then
			return DESKTOP_POLL
		end
	end
	if next(state.working) then
		return WORKING_POLL
	end
	return ns.Notices.PollEvery()
end

-- Progress goes to the run in progress of its chat. Requests wait for an answer.
local function ApplyLive(live)
	if type(live) ~= "table" then
		return
	end
	for _, p in ipairs(type(live.progress) == "table" and live.progress or {}) do
		local working = type(p) == "table" and state.working[p.chat]
		local chat = working and ns.Store.Chat(p.chat)
		if chat and working.id == p.id and type(p.lines) == "table" then
			working.queued = QueuedLine(p.lines)
			working.progress = working.queued and {} or p.lines
			if LEVELS[p.lines[1]] then
				chat.level = LEVELS[p.lines[1]]
			end
			ApplyDesktop(chat, working, p.lines)
		end
	end
	CountDesktopPolls()
	local requests = {}
	for _, r in ipairs(type(live.permissions) == "table" and live.permissions or {}) do
		if ValidRequest(r) then
			table.insert(requests, r)
		end
	end
	state.requests = requests
	ns.Notices.Apply(live.notices, Messages.Stats().bodyNow)
end

-- The "Reload soon" banner shows only in the window, and notifications need no window.
local function TellLowSlotsOnce()
	if state.lowSlotsTold or not Messages.SlotsLow() then
		return
	end
	state.lowSlotsTold = true
	print(ns.App.title .. ": slots run low. Type /reload to keep replies and notifications.")
end

local function ApplySlot(restore, live)
	ApplyRestore(restore)
	ApplyLive(live)
	TellLowSlotsOnce()
	-- No later poll takes a notice away, so the bell shows nothing stale.
	if Messages.SlotsLeft() == 0 then
		ns.Notices.Drop()
	end
	if #ns.Store.db.forget > 0 and Messages.Online() then
		Messages.Hello()
	end
end

-- The oldest request that this session has not answered.
function Transport.Request()
	for _, r in ipairs(state.requests) do
		if not state.answered[r.request] then
			return r
		end
	end
end

function Transport.RequestCount()
	local count = 0
	for _, r in ipairs(state.requests) do
		if not state.answered[r.request] then
			count = count + 1
		end
	end
	return count
end

-- The line of the bridge while the message waits for other chats to finish, or nil.
function Transport.Queued(chatId)
	local working = state.working[chatId]
	return working and working.queued
end

-- True while a popup of the chat waits for the player.
function Transport.WaitsForAnswer(chatId)
	for _, r in ipairs(state.requests) do
		if r.chat == chatId and not state.answered[r.request] then
			return true
		end
	end
	return false
end

-- The hash tells the bridge which text the user saw (SPEC.md 9.3).
-- The popup text that an answer vouches for. "Always allow" also shows its rule line,
-- so its hash binds the rule that the player saw (SPEC.md 6.6.5).
local function Shown(request, optionId)
	for _, option in ipairs(request.options) do
		if option.id == optionId and option.kind == "allow_always" then
			return request.text .. "\n" .. tostring(option.label)
		end
	end
	return request.text
end

function Transport.Answer(request, optionId)
	local hash = ns.Codec.Hex(ns.Sha256(Shown(request, optionId))):sub(1, 16)
	state.answered[request.request] = true
	Messages.Control(request.chat, 0, string.format("perm=%s:%s:%s", request.request, optionId, hash))
	Messages.OnChange()
end

Messages.Store = { Add = ns.Store.AddMessage, Open = ns.Store.Open, Find = Find }
Messages.Fields = Fields
Messages.OnStatus = ApplyStatus
Messages.OnReply = ApplyReply
Messages.OnGiveUp = function(chat, id, text)
	ns.Store.AddReply(chat, id, text, "error")
end
Messages.OnOther = ApplyList
Messages.Riders = Forgets
Messages.OnRidersShown = Forgotten
Messages.OnPoll = ApplySlot
Messages.Awaits = Listing
Messages.PollEvery = PollEvery
