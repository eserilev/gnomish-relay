-- Messages out through the strip, replies in through the slots (SPEC.md 7).
-- The rules follow models/transport.qnt.

local _, ns = ...

local Transport = {}
ns.Transport = Transport

local SLOTS = 1000
local LOW_SLOTS = 20
local REPORT_AHEAD = 20
local SHOWS = 3
local RETRY = 40
local LATE_POLL = 60
local IDLE_POLL = 600
local ONLINE_FOR = 720
local SCHEDULE = { 5, 10, 16, 24, 34, 46, 60, 80, 100, 130, 160, 200, 240, 300 }
local PROTO = 1
-- Room for the flags of Report(): `next`, `read` with up to 30 ids, and `restored`.
local REPORT_ROOM = 400
local TOO_LONG = "Too long to send."
local NOT_SENT = "Not sent. Send it again."
-- The bridge accepts a frame up to 300 s old (S11). Keep a margin for the screenshot.
local FRESH_FOR = 270

local state = {
	nextSlot = 1,
	reported = nil,
	helloDue = true,
	lastSend = nil,
	nextPoll = 0,
	shows = {},
	controls = {},
	bodyDone = {},
	working = {},
	-- The record of each message, taken at send time from our own code. Strips come
	-- from here, never from GnomishRelayDB, which any addon can change (SPEC.md 6.6.1).
	private = {},
	lastNow = nil,
	missing = false,
	mismatch = false,
}

Transport.OnChange = function() end
Transport.OnReply = function() end

local function SlotName(n)
	return string.format("GnomishRelay_S%04d", n)
end

function Transport.Init()
	local n = 1
	while n <= SLOTS and C_AddOns.IsAddOnLoaded(SlotName(n)) do
		n = n + 1
	end
	state.nextSlot = n
	state.helloDue = true
	state.nextPoll = GetTime() + SCHEDULE[1]
end

function Transport.SlotsLeft()
	return SLOTS - state.nextSlot + 1
end

function Transport.Working(chatId)
	return state.working[chatId]
end

function Transport.Online()
	return state.lastNow ~= nil and time() - state.lastNow < ONLINE_FOR
end

function Transport.NeedsReload()
	return #ns.Store.db.outbox > 0 or Transport.SlotsLeft() < LOW_SLOTS
end

function Transport.Problem()
	if state.missing then
		return "missing"
	elseif state.mismatch then
		return "mismatch"
	end
end

function Transport.Stats()
	return {
		nextSlot = state.nextSlot,
		slotsLeft = Transport.SlotsLeft(),
		reported = state.reported,
		outbox = #ns.Store.db.outbox,
		open = #ns.Store.Open(),
		online = Transport.Online(),
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
	if ns.Store.db.restored then
		table.insert(flags, "restored")
	end
	return flags
end

local function ChatFlags(chat)
	local flags = { "agent=" .. chat.agent, "level=" .. chat.mode }
	if chat.fresh then
		table.insert(flags, "n")
	end
	return flags
end

local function MessageRecord(chat, message)
	return {
		token = ns.Store.db.token,
		chat = chat.id,
		id = message.id,
		cwd = chat.cwd,
		flags = table.concat(ChatFlags(chat), ";"),
		name = chat.name,
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

-- The message ends as an error at once, so it never retries forever.
local function GiveUp(item, text)
	ns.Store.AddReply(item.chat, item.message.id, text, "error")
end

-- The outbox holds a signed frame, so the bridge checks it as it checks a strip
-- (SPEC.md 7.5). Hex keeps every byte safe inside the saved variables file.
local function ToOutbox(item, frame, signedAt)
	item.message.outbox = true
	table.insert(ns.Store.db.outbox, {
		chat = item.chat.id,
		id = item.message.id,
		frame = ns.Codec.Hex(frame),
		at = signedAt,
	})
end

local function OutboxEntry(chatId, id)
	for i, entry in ipairs(ns.Store.db.outbox) do
		if entry.chat == chatId and entry.id == id then
			return entry, i
		end
	end
end

local function RemoveFromOutbox(chatId, id)
	local _, i = OutboxEntry(chatId, id)
	if i then
		table.remove(ns.Store.db.outbox, i)
	end
end

-- An outbox frame that the bridge has not taken in time is too old to take now.
local function ExpireOutbox(item)
	local entry = OutboxEntry(item.chat.id, item.message.id)
	if not entry or time() - entry.at >= FRESH_FOR then
		RemoveFromOutbox(item.chat.id, item.message.id)
		item.message.outbox = nil
		GiveUp(item, NOT_SENT)
	end
end

-- Open messages that the bridge has not acknowledged, and that are due for a strip.
-- `due` have their private record. `stored` come from before a /reload: only their
-- signed frame is left, and it goes out as it is.
local function Due(now)
	local due, stored = {}, {}
	for _, item in ipairs(ns.Store.Open()) do
		local message = item.message
		local shown = state.shows[message.id]
		item.record = state.private[message.id]
		if message.outbox then
			ExpireOutbox(item)
		elseif message.acked then
			item.record = nil
		elseif not item.record and time() - (message.signedAt or 0) >= FRESH_FOR then
			GiveUp(item, NOT_SENT)
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
	local records, ids = {}, {}
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
		Add({ token = control.token, chat = control.chat, id = control.id, flags = control.flags })
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
		Add({ token = ns.Store.db.token, chat = "relay", id = 0, flags = "h" })
	end
	return records, ids
end

-- `reporting` is the slot that the strip reports, or nil for a stored frame.
local function ShowFrame(frame, ids, controls, reporting)
	if reporting then
		-- A hello that comes due during the shot stays due.
		state.helloDue = false
	end
	ns.Strip.Show(frame, function(ok)
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
		if reporting then
			state.reported = reporting
		end
		Transport.OnChange()
	end)
end

function Transport.ShowNextStrip()
	if ns.Strip.Busy() or not ns.key then
		return
	end
	local due, stored = Due(GetTime())
	if #stored > 0 then
		local message = stored[1].message
		ShowFrame(ns.Codec.FromHex(message.frame), { message.id }, 0, nil)
		return
	end
	if #due == 0 and #state.controls == 0 and not state.helloDue then
		return
	end
	local records, ids = Records(due)
	ShowFrame(Sign(records, ids[1] or 0), ids, #state.controls, state.nextSlot)
end

function Transport.Fits(chat, text)
	local record = MessageRecord(chat, { id = ns.Store.db.nextId, text = text })
	return #ns.Codec.Payload({ record }) + REPORT_ROOM <= ns.Codec.MAX_PAYLOAD
end

-- Returns nil for a message that does not fit in one strip.
function Transport.Send(chat, text)
	if not Transport.Fits(chat, text) then
		return nil
	end
	local message = ns.Store.AddMessage(chat, text)
	local record = MessageRecord(chat, message)
	state.private[message.id] = record
	message.frame = ns.Codec.Hex(Sign({ record }, message.id))
	message.signedAt = time()
	state.lastSend = GetTime()
	state.nextPoll = state.lastSend + SCHEDULE[1]
	Transport.ShowNextStrip()
	Transport.OnChange()
	return message
end

function Transport.Stop(chat)
	table.insert(state.controls, { token = ns.Store.db.token, chat = chat.id, id = 0, flags = "stop" })
	Transport.ShowNextStrip()
end

local function ApplyReply(r, done)
	local chat = ns.Store.Chat(r.chat)
	local message = chat and ns.Store.Message(chat, r.id)
	if not message then
		return
	end
	message.acked = true
	chat.fresh = nil
	if message.outbox then
		message.outbox = nil
		RemoveFromOutbox(chat.id, r.id)
	end
	if r.status == "working" then
		local working = state.working[chat.id]
		if not working or working.id ~= r.id then
			working = { id = r.id, since = GetTime() }
			state.working[chat.id] = working
		end
		working.progress = r.progress or {}
		return
	end
	done[r.id] = true
	if state.working[chat.id] and state.working[chat.id].id == r.id then
		state.working[chat.id] = nil
	end
	if ns.Store.AddReply(chat, r.id, r.text, r.status) then
		Transport.OnReply(chat, r)
	end
end

local function ApplyRestore(restore)
	local db = ns.Store.db
	if type(restore) ~= "table" or restore.token ~= db.token or db.restored then
		return
	end
	ns.Store.MergeChats(type(restore.chats) == "table" and restore.chats or {})
	db.restored = true
	state.helloDue = true
end

local function Apply(data)
	if type(data) ~= "table" or data.proto ~= PROTO then
		state.mismatch = true
		return
	end
	state.mismatch = false
	state.lastNow = data.now
	local done = {}
	for _, r in ipairs(data.replies or {}) do
		ApplyReply(r, done)
	end
	state.bodyDone = done
end

function Transport.Poll()
	if state.nextSlot > SLOTS then
		return
	end
	local name = SlotName(state.nextSlot)
	C_AddOns.EnableAddOn(name)
	GnomishRelay_SlotData = nil
	GnomishRelay_Restore = nil
	local loaded = C_AddOns.LoadAddOn(name)
	local data, restore = GnomishRelay_SlotData, GnomishRelay_Restore
	GnomishRelay_SlotData = nil
	GnomishRelay_Restore = nil
	state.missing = not loaded
	if loaded then
		state.nextSlot = state.nextSlot + 1
		if not state.reported or state.nextSlot - state.reported >= REPORT_AHEAD then
			state.helloDue = true
		end
		Apply(data)
		ApplyRestore(restore)
	end
	Transport.OnChange()
end

local function NextDelay(now)
	for _, item in ipairs(ns.Store.Open()) do
		if not item.message.outbox then
			local elapsed = now - (state.lastSend or -math.huge)
			for _, at in ipairs(SCHEDULE) do
				if at > elapsed then
					return at - elapsed
				end
			end
			return LATE_POLL
		end
	end
	return IDLE_POLL
end

function Transport.Tick()
	local now = GetTime()
	if now >= state.nextPoll then
		Transport.Poll()
		state.nextPoll = now + NextDelay(now)
	end
	Transport.ShowNextStrip()
end
