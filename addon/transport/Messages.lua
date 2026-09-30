-- Messages out through the strip, replies in through the slots (SPEC.md 7). The rules
-- follow models/transport.qnt. Each app sets the hooks at the end of this file.
-- A chat is a table with an `id` field.

local _, ns = ...

local Messages = {}
ns.Messages = Messages

local LOW_SLOTS = 20
local REPORT_AHEAD = 20
local SHOWS = 3
local RETRY = 40
local LATE_POLL = 60
local IDLE_POLL = 600
local ONLINE_FOR = 720
-- The bridge writes a body every 60 s. A body this old at its poll means it stopped.
local STALE = 150
-- A body this old at its poll means the bridge missed a heartbeat.
local LATE = 90
local SCHEDULE = { 5, 10, 16, 24, 34, 46, 60, 80, 100, 130, 160, 200, 240, 300 }
local PROTO = 1
-- Room for the flags of Report(): `next`, `read` with up to 30 ids, `restored`, and
-- the health flags.
local REPORT_ROOM = 440
local TOO_LONG = "Too long to send."
local NOT_SENT = "Not sent. Send it again."
local BRIDGE_OFF = "Not sent: the bridge is not running. On the desktop, run gnomish-relay restart."
local BAD_KEY =
	"Not sent: the bridge does not know this key. On the desktop, run gnomish-relay setup, then type /reload."
-- The bridge accepts a frame up to 300 s old (S11). Keep a margin for the screenshot.
local FRESH_FOR = 270
-- A later body can still hold the final reply of an answered message. The default store
-- keeps this many answered messages, so the addon still reports such a reply as read.
local KEEP_ANSWERED = 64
local ID_CHARS = "abcdefghijklmnopqrstuvwxyz0123456789"

local state = {
	nextSlot = 1,
	reported = nil,
	helloDue = true,
	lastSend = nil,
	nextPoll = 0,
	shows = {},
	controls = {},
	bodyDone = {},
	-- The record of each message, taken at send time from our own code. Strips come
	-- from here, never from the saved variables, which any addon can change (SPEC.md 6.6.1).
	private = {},
	lastNow = nil,
	-- The age of the last body when the addon loaded it.
	bodyAge = nil,
	-- Strips that the bridge refused for their tag since its last good strip.
	badTags = 0,
	polled = false,
	missing = false,
	mismatch = false,
}

function Messages.RandomId(length)
	local out = {}
	for i = 1, length do
		local n = math.random(#ID_CHARS)
		out[i] = ID_CHARS:sub(n, n)
	end
	return table.concat(out)
end

-- The saved variables of the app, with the fields that this file needs.
function Messages.Db()
	local db = ns.Saved()
	if not ns.Codec.IsValidId(db.token) then
		db.token = Messages.RandomId(16)
	end
	-- Ids start from the clock, so ids after a wipe never repeat older ones.
	db.nextId = db.nextId or (time() - 1700000000)
	db.outbox = db.outbox or {}
	return db
end

function Messages.NewId()
	local db = Messages.Db()
	local id = db.nextId
	db.nextId = id + 1
	return id
end

function Messages.Init()
	state.nextSlot = ns.Slots.FirstFree()
	state.helloDue = true
	state.nextPoll = GetTime() + SCHEDULE[1]
end

function Messages.SlotsLeft()
	return ns.Slots.COUNT - state.nextSlot + 1
end

-- "checking" until the first poll, then "online", "slow", or "offline".
function Messages.Bridge()
	if state.lastNow == nil then
		return state.polled and "offline" or "checking"
	elseif state.bodyAge >= STALE or time() - state.lastNow >= ONLINE_FOR then
		return "offline"
	elseif state.bodyAge >= LATE then
		return "slow"
	end
	return "online"
end

function Messages.Online()
	local bridge = Messages.Bridge()
	return bridge == "online" or bridge == "slow"
end

function Messages.NextPollIn()
	return math.max(0, state.nextPoll - GetTime())
end

-- Where an open message is: "sending" with its shows so far and the most, "delivered",
-- or "reload". Nil for an answered message.
function Messages.Delivery(message)
	if message.answered then
		return nil
	elseif message.outbox then
		return "reload"
	elseif message.acked then
		return "delivered"
	end
	local shown = state.shows[message.id]
	return "sending", shown and shown.count or 0, SHOWS
end

function Messages.NeedsReload()
	return #Messages.Db().outbox > 0 or Messages.SlotsLeft() < LOW_SLOTS
end

function Messages.Problem()
	if state.missing then
		return "missing"
	elseif ns.Health.Blocked() then
		return "blocked"
	elseif state.mismatch then
		return "mismatch"
	end
end

function Messages.Stats()
	return {
		nextSlot = state.nextSlot,
		slotsLeft = Messages.SlotsLeft(),
		reported = state.reported,
		outbox = #Messages.Db().outbox,
		open = #Messages.Store.Open(),
		online = Messages.Online(),
		-- The clock of the bridge in the last body.
		bodyNow = state.lastNow,
	}
end

-- What every strip tells the bridge besides the messages.
local function Report()
	local flags = { "next=" .. state.nextSlot }
	local read = {}
	for id in pairs(state.bodyDone) do
		table.insert(read, id)
	end
	if #read > 0 then
		table.sort(read)
		table.insert(flags, "read=" .. table.concat(read, ","))
	end
	if Messages.Db().restored then
		table.insert(flags, "restored")
	end
	for _, flag in ipairs(ns.Health.Flags()) do
		table.insert(flags, flag)
	end
	return flags
end

local function MessageRecord(chat, message)
	local cwd, flags, name = Messages.Fields(chat, message)
	return {
		token = Messages.Db().token,
		chat = chat.id,
		id = message.id,
		cwd = cwd,
		flags = flags,
		name = name,
		text = message.text,
	}
end

local function Copy(t)
	local out = {}
	for k, v in pairs(t) do
		out[k] = v
	end
	return out
end

local function Sign(records, frameId)
	return ns.Codec.Frame(time(), frameId, ns.Codec.Payload(records), ns.key)
end

local function NotSent()
	if not Messages.Online() then
		return BRIDGE_OFF
	elseif state.badTags > 0 then
		return BAD_KEY
	end
	return NOT_SENT
end

-- The message ends as an error at once, so it never retries forever.
local function GiveUp(item, text)
	if item.message.answered then
		return
	end
	item.message.answered = true
	Messages.OnGiveUp(item.chat, item.message.id, text)
	Messages.OnChange()
end

-- The outbox holds a signed frame, so the bridge checks it as it checks a strip
-- (SPEC.md 7.5). Hex keeps every byte safe inside the saved variables file.
local function ToOutbox(item, frame, signedAt)
	item.message.outbox = true
	table.insert(Messages.Db().outbox, {
		chat = item.chat.id,
		id = item.message.id,
		frame = ns.Codec.Hex(frame),
		at = signedAt,
	})
	Messages.OnChange()
end

local function OutboxEntry(chatId, id)
	for i, entry in ipairs(Messages.Db().outbox) do
		if entry.chat == chatId and entry.id == id then
			return entry, i
		end
	end
end

local function RemoveFromOutbox(chatId, id)
	local _, i = OutboxEntry(chatId, id)
	if i then
		table.remove(Messages.Db().outbox, i)
	end
end

-- An outbox frame that the bridge has not taken in time is too old to take now.
local function ExpireOutbox(item)
	local chatId = item.chat.id
	local entry = OutboxEntry(chatId, item.message.id)
	if not entry or time() - entry.at >= FRESH_FOR then
		RemoveFromOutbox(chatId, item.message.id)
		item.message.outbox = nil
		GiveUp(item, NotSent())
	end
end

-- Open messages that the bridge has not acknowledged, and that are due for a strip.
-- `due` have their private record. `stored` come from before a /reload: only their
-- signed frame is left, and it goes out as it is.
local function Due(now)
	local due, stored = {}, {}
	for _, item in ipairs(Messages.Store.Open()) do
		local message = item.message
		local shown = state.shows[message.id]
		item.record = state.private[message.id]
		if message.outbox then
			ExpireOutbox(item)
		elseif message.acked then
			item.record = nil
		elseif not item.record and time() - (message.signedAt or 0) >= FRESH_FOR then
			GiveUp(item, NotSent())
		elseif not shown or now - shown.at >= RETRY then
			if shown and shown.count >= SHOWS then
				if item.record then
					ToOutbox(item, Sign({ item.record }, message.id), time())
				else
					ToOutbox(item, ns.Codec.FromHex(message.frame), message.signedAt)
				end
			elseif item.record then
				table.insert(due, item)
			else
				table.insert(stored, item)
			end
		end
	end
	return due, stored
end

local function JoinFlags(a, b)
	if a == "" then
		return b
	end
	return a .. ";" .. b
end

-- The report rides on the first record of a strip (SPEC.md 7.1.1).
local function Records(due)
	local report = table.concat(Report(), ";")
	local token = Messages.Db().token
	local records, ids, riders = {}, {}, {}
	local function Add(record)
		if #records == 0 then
			record.flags = JoinFlags(record.flags, report)
		end
		table.insert(records, record)
		if #records > ns.Codec.MAX_RECORDS or #ns.Codec.Payload(records) > ns.Codec.MAX_PAYLOAD then
			table.remove(records)
			return false
		end
		return true
	end
	for _, control in ipairs(state.controls) do
		Add({ token = token, chat = control.chat, id = control.id, flags = control.flags })
	end
	for _, rider in ipairs(Messages.Riders()) do
		if Add({ token = token, chat = rider.chat, id = rider.id, flags = rider.flags }) then
			table.insert(riders, rider)
		end
	end
	for _, item in ipairs(due) do
		if Add(Copy(item.record)) then
			table.insert(ids, item.message.id)
		elseif #records == 0 then
			GiveUp(item, TOO_LONG)
		else
			break
		end
	end
	if #records == 0 then
		Add({ token = token, chat = ns.App.helloChat, id = 0, flags = "h" })
	end
	return records, ids, riders
end

-- `reporting` is the slot that the strip reports, or nil for a stored frame.
local function ShowFrame(frame, ids, controls, reporting, riders)
	if reporting then
		-- A hello that comes due during the shot stays due.
		state.helloDue = false
	end
	ns.Strip.Show(frame, function(ok)
		ns.Health.Shot(ok)
		if not ok then
			state.helloDue = state.helloDue or reporting ~= nil
			return
		end
		for _, id in ipairs(ids) do
			local shown = state.shows[id] or { count = 0 }
			state.shows[id] = { count = shown.count + 1, at = GetTime() }
		end
		for _ = 1, controls do
			table.remove(state.controls, 1)
		end
		Messages.OnRidersShown(riders or {})
		if reporting then
			state.reported = reporting
		end
		Messages.OnChange()
	end)
end

-- A strip that only reports or asks for a list can wait for the end of a fight: a
-- strip is a flash of color and the hitch of a screenshot.
local function Urgent(due, stored)
	if #due > 0 or #stored > 0 then
		return true
	end
	for _, control in ipairs(state.controls) do
		if not control.later then
			return true
		end
	end
	return false
end

-- A player who never read about the strip takes its flash for a bug.
local function ExplainStripOnce()
	local db = Messages.Db()
	if not db.stripExplained then
		db.stripExplained = true
		print(
			ns.App.title
				.. ": the colored bar at the top left carries your messages to the desktop. It shows for half a second."
		)
	end
end

function Messages.ShowNextStrip()
	if ns.Strip.Busy() or not ns.key then
		return
	end
	local due, stored = Due(GetTime())
	if #stored == 0 and #due == 0 and #state.controls == 0 and not state.helloDue then
		return
	end
	if InCombatLockdown() and not Urgent(due, stored) then
		return
	end
	-- While another app holds the corner, nothing is signed and no show counts. So the
	-- retry timer and the shows wait too (SPEC.md 7.1).
	if not ns.Strip.TakeTurn() then
		return
	end
	ExplainStripOnce()
	if #stored > 0 then
		local message = stored[1].message
		ShowFrame(ns.Codec.FromHex(message.frame), { message.id }, 0, nil)
		return
	end
	local records, ids, riders = Records(due)
	ShowFrame(Sign(records, ids[1] or 0), ids, #state.controls, state.nextSlot, riders)
end

-- The bytes of text that one strip has room for in a message of `chat`.
function Messages.Room(chat)
	local record = MessageRecord(chat, { id = Messages.Db().nextId, text = "" })
	return ns.Codec.MAX_PAYLOAD - REPORT_ROOM - #ns.Codec.Payload({ record })
end

function Messages.Fits(chat, text)
	return #text <= Messages.Room(chat)
end

-- The next polls follow the schedule after a send (SPEC.md 7.3).
function Messages.StartPolls()
	state.lastSend = GetTime()
	state.nextPoll = state.lastSend + SCHEDULE[1]
end

-- Signs and sends a message that the store already holds.
function Messages.Queue(chat, message)
	local record = MessageRecord(chat, message)
	state.private[message.id] = record
	message.frame = ns.Codec.Hex(Sign({ record }, message.id))
	message.signedAt = time()
	Messages.StartPolls()
	Messages.ShowNextStrip()
	Messages.OnChange()
	return message
end

-- Returns nil for a message that does not fit in one strip.
function Messages.Send(chat, text)
	if not Messages.Fits(chat, text) then
		return nil
	end
	return Messages.Queue(chat, Messages.Store.Add(chat, text))
end

-- A control record goes out once, on the next strip. It is no message: it has no retry.
function Messages.Control(chat, id, flags)
	table.insert(state.controls, { chat = chat, id = id, flags = flags })
	Messages.ShowNextStrip()
end

-- A control that can wait. In combat it only rides on a strip that goes anyway.
function Messages.ControlLater(chat, id, flags)
	table.insert(state.controls, { chat = chat, id = id, flags = flags, later = true })
	Messages.ShowNextStrip()
end

-- The next strip goes out, also with no message in it.
function Messages.Hello()
	state.helloDue = true
end

-- Every record stays in the body until a `read` flag names it (SPEC.md 7.3).
local function ApplyReply(r, done)
	local chat, message = Messages.Store.Find(r.chat, r.id)
	if not message then
		if Messages.OnOther(r) then
			done[r.id] = true
		end
		return
	end
	message.acked = true
	if message.outbox then
		message.outbox = nil
		RemoveFromOutbox(chat.id, r.id)
	end
	Messages.OnStatus(chat, r.id, r.status)
	if r.status == "working" then
		return
	end
	done[r.id] = true
	if not message.answered then
		message.answered = true
		Messages.OnReply(chat, r.id, r.status, r.text)
	end
end

local function Apply(data)
	if type(data) ~= "table" or data.proto ~= PROTO then
		state.mismatch = true
		return
	end
	state.mismatch = false
	state.lastNow = tonumber(data.now)
	state.bodyAge = state.lastNow and time() - state.lastNow
	state.badTags = tonumber(data.badTags) or 0
	local done = {}
	for _, r in ipairs(data.replies or {}) do
		ApplyReply(r, done)
	end
	state.bodyDone = done
end

function Messages.Poll()
	if state.nextSlot > ns.Slots.COUNT then
		return
	end
	local loaded, data, restore, live = ns.Slots.Load(state.nextSlot)
	state.polled = true
	state.missing = not loaded
	ns.Health.Slot(loaded)
	if loaded then
		state.nextSlot = state.nextSlot + 1
		if not state.reported or state.nextSlot - state.reported >= REPORT_AHEAD then
			state.helloDue = true
		end
		Apply(data)
		Messages.OnPoll(restore, live)
	end
	Messages.OnChange()
end

local function NormalDelay(now)
	local waiting = Messages.Awaits()
	for _, item in ipairs(Messages.Store.Open()) do
		waiting = waiting or not item.message.outbox
	end
	if not waiting then
		return IDLE_POLL
	end
	local elapsed = now - (state.lastSend or -math.huge)
	for _, at in ipairs(SCHEDULE) do
		if at > elapsed then
			return at - elapsed
		end
	end
	return LATE_POLL
end

local function NextDelay(now)
	local delay = NormalDelay(now)
	local fast = Messages.PollEvery()
	return fast and math.min(delay, fast) or delay
end

function Messages.Tick()
	local now = GetTime()
	if now >= state.nextPoll then
		Messages.Poll()
		state.nextPoll = now + NextDelay(now)
	end
	Messages.ShowNextStrip()
end

-- The default store: one list of messages in the saved variables of the app. The relay
-- keeps its messages in its chats, and sets its own store.

local function Sent()
	local db = Messages.Db()
	db.sent = db.sent or {}
	return db.sent
end

-- Drops the oldest answered messages past KEEP_ANSWERED.
local function Prune(sent)
	local answered = 0
	for i = #sent, 1, -1 do
		if sent[i].answered then
			answered = answered + 1
			if answered > KEEP_ANSWERED then
				table.remove(sent, i)
			end
		end
	end
end

local DefaultStore = {}

function DefaultStore.Add(chat, text)
	local sent = Sent()
	local message = { chat = chat.id, id = Messages.NewId(), text = text }
	table.insert(sent, message)
	Prune(sent)
	return message
end

-- Every sent message with no final reply, oldest first.
function DefaultStore.Open()
	local open = {}
	for _, message in ipairs(Sent()) do
		if not message.answered then
			table.insert(open, { chat = { id = message.chat }, message = message })
		end
	end
	return open
end

function DefaultStore.Find(chatId, id)
	for _, message in ipairs(Sent()) do
		if message.chat == chatId and message.id == id then
			return { id = message.chat }, message
		end
	end
end

-- The hooks. The defaults suit an app with one list of messages and no riders.

-- `Add(chat, text)` returns a new message with an id from NewId(). `Open()` lists
-- `{ chat, message }` with no final reply, oldest first. `Find(chatId, id)` returns
-- the chat and the message.
Messages.Store = DefaultStore

-- The `cwd`, `flags`, and `name` fields of the record of a message.
Messages.Fields = function()
	return "", "", ""
end

-- Each final reply, once.
Messages.OnReply = function() end

-- A message that the addon gave up on. It is final, like a reply.
Messages.OnGiveUp = function(chat, id, text)
	Messages.OnReply(chat, id, "error", text)
end

-- Each record of a known message in each body, also `working` and a second copy.
Messages.OnStatus = function() end

-- A record of the body with no known message. True reports it as read.
Messages.OnOther = function()
	return false
end

-- Records `{ chat, id, flags }` that ride on a strip after the controls. Unlike a
-- control, a rider never starts a strip.
Messages.Riders = function()
	return {}
end

-- The records of Riders() that the last strip carried.
Messages.OnRidersShown = function() end

-- The restore bundle and the live file of each loaded slot.
Messages.OnPoll = function() end

-- True while the app waits for a reply that is no message, so the polls stay fast.
Messages.Awaits = function()
	return false
end

-- Seconds to the next poll while the app waits for something off the schedule, or nil.
Messages.PollEvery = function()
	return nil
end

Messages.OnChange = function() end
