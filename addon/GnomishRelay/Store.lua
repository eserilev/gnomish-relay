-- The saved data: token, chats, and the outbox. It survives /reload, and a
-- saved-data wipe loses it (SPEC.md 7.6).

local _, ns = ...

local Store = {}
ns.Store = Store

local HISTORY_LIMIT = 200
local ID_CHARS = "abcdefghijklmnopqrstuvwxyz0123456789"

local function RandomId(length)
	local out = {}
	for i = 1, length do
		local n = math.random(#ID_CHARS)
		out[i] = ID_CHARS:sub(n, n)
	end
	return table.concat(out)
end

function Store.Load()
	GnomishRelayDB = GnomishRelayDB or {}
	local db = GnomishRelayDB
	if not ns.Codec.IsValidId(db.token) then
		db.token = RandomId(16)
	end
	-- Ids start from the clock, so ids after a wipe never repeat older ones.
	db.nextId = db.nextId or (time() - 1700000000)
	db.chats = db.chats or {}
	db.outbox = db.outbox or {}
	db.restored = db.restored or false
	db.whisperColor = db.whisperColor or "f0a860"
	Store.db = db
end

function Store.Chats()
	return Store.db.chats
end

function Store.Chat(id)
	for _, chat in ipairs(Store.db.chats) do
		if chat.id == id then
			return chat
		end
	end
end

function Store.NewChat(agent)
	local chat = {
		id = RandomId(10),
		name = "Chat " .. (#Store.db.chats + 1),
		agent = agent or "claude",
		mode = "auto-edit",
		cwd = "",
		history = {},
		fresh = true,
	}
	table.insert(Store.db.chats, chat)
	return chat
end

local function Append(chat, entry)
	table.insert(chat.history, entry)
	if #chat.history > HISTORY_LIMIT then
		table.remove(chat.history, 1)
	end
end

function Store.AddMessage(chat, text)
	local message = { role = "user", id = Store.db.nextId, text = text }
	Store.db.nextId = Store.db.nextId + 1
	Append(chat, message)
	return message
end

function Store.Message(chat, id)
	for _, entry in ipairs(chat.history) do
		if entry.role == "user" and entry.id == id then
			return entry
		end
	end
end

-- Every sent message with no final reply, oldest first.
function Store.Open()
	local open = {}
	for _, chat in ipairs(Store.db.chats) do
		for _, entry in ipairs(chat.history) do
			if entry.role == "user" and not entry.answered then
				table.insert(open, { chat = chat, message = entry })
			end
		end
	end
	table.sort(open, function(a, b)
		return a.message.id < b.message.id
	end)
	return open
end

-- Returns false for a reply that is already in the history.
function Store.AddReply(chat, id, text, status)
	local message = Store.Message(chat, id)
	if not message or message.answered then
		return false
	end
	message.answered = true
	Append(chat, { role = status == "error" and "error" or "agent", id = id, text = text, agent = chat.agent })
	return true
end

-- Restored messages are history, not new work: they are never sent again.
local function RestoredHistory(entries)
	local history = {}
	for _, entry in ipairs(entries or {}) do
		table.insert(history, {
			role = entry.role,
			id = entry.id,
			text = entry.text,
			agent = entry.agent,
			answered = true,
		})
	end
	return history
end

-- A chat that is already here stays as it is, so a second copy of a bundle changes nothing.
function Store.MergeChats(chats)
	for _, incoming in ipairs(chats) do
		if ns.Codec.IsValidId(incoming.id) and not Store.Chat(incoming.id) then
			table.insert(Store.db.chats, {
				id = incoming.id,
				name = incoming.name or incoming.id,
				agent = incoming.agent or "claude",
				mode = incoming.mode or "auto-edit",
				cwd = incoming.cwd or "",
				history = RestoredHistory(incoming.history),
			})
		end
	end
end
