-- The saved data: chats, deletes, and settings. Messages.lua keeps the token and the outbox. It survives /reload, and a
-- saved-data wipe loses it (SPEC.md 7.6).

local _, ns = ...

local Store = {}
ns.Store = Store

local HISTORY_LIMIT = 200
local DEFAULT_AGENT = "claude"
local DEFAULT_MODE = "auto-edit"

function Store.Load()
	local db = ns.Messages.Db()
	db.chats = db.chats or {}
	-- Deleted chats that the bridge has not heard of yet.
	db.forget = db.forget or {}
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
		id = ns.Messages.RandomId(10),
		name = "Chat " .. (#Store.db.chats + 1),
		agent = agent or DEFAULT_AGENT,
		mode = DEFAULT_MODE,
		cwd = "",
		history = {},
		fresh = true,
	}
	table.insert(Store.db.chats, chat)
	return chat
end

-- A chat that continues a saved session of an agent. Its first message asks the
-- bridge to attach it, and the reply brings the last exchange.
function Store.ResumeChat(row)
	local chat = {
		id = ns.Messages.RandomId(10),
		name = row.title ~= "" and row.title or row.repo,
		agent = row.agent,
		mode = DEFAULT_MODE,
		cwd = row.folder,
		history = {},
		attach = row.session,
	}
	table.insert(Store.db.chats, chat)
	return chat
end

function Store.DeleteChat(id)
	local db = Store.db
	for i, chat in ipairs(db.chats) do
		if chat.id == id then
			table.remove(db.chats, i)
			break
		end
	end
	for i = #db.outbox, 1, -1 do
		if db.outbox[i].chat == id then
			table.remove(db.outbox, i)
		end
	end
	if db.selected == id then
		db.selected = nil
	end
	table.insert(db.forget, id)
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

-- The reply to an attach holds the last prompt on its first line, and the answer below.
local function AddExchange(chat, id, text)
	local prompt, answer = tostring(text):match("^([^\n]*)\n?(.*)$")
	if prompt ~= "" then
		Append(chat, { role = "user", text = prompt, answered = true })
	end
	if answer ~= "" then
		Append(chat, { role = "agent", id = id, text = answer, agent = chat.agent })
	end
end

-- Messages.lua marks the message as answered, and calls this once for each message.
function Store.AddReply(chat, id, text, status)
	local message = Store.Message(chat, id)
	if message.attach and status ~= "error" then
		AddExchange(chat, id, text)
		return
	end
	Append(chat, { role = status == "error" and "error" or "agent", id = id, text = text, agent = chat.agent })
end

local ROLES = { user = true, agent = true, error = true }

-- Restored messages are history, not new work: they are never sent again.
local function RestoredHistory(entries)
	local history = {}
	for _, entry in ipairs(type(entries) == "table" and entries or {}) do
		if type(entry) == "table" and ROLES[entry.role] then
			table.insert(history, {
				role = entry.role,
				id = entry.id,
				text = entry.text,
				agent = entry.agent,
				answered = true,
			})
		end
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
				agent = ns.Codec.IsValidId(incoming.agent) and incoming.agent or DEFAULT_AGENT,
				mode = incoming.mode == "ask" and "ask" or DEFAULT_MODE,
				cwd = incoming.cwd or "",
				history = RestoredHistory(incoming.history),
			})
		end
	end
end
