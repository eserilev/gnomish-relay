-- The saved data: chats, deletes, and settings. Messages.lua keeps the token and the outbox. It survives /reload, and a
-- saved-data wipe loses it (SPEC.md 7.6).

local _, ns = ...

local Store = {}
ns.Store = Store

local HISTORY_LIMIT = 200
local DEFAULT_AGENT = "claude"
local DEFAULT_MODE = "auto-edit"
local DEFAULT_FONT_SIZE = 14
-- The Permissions dropdown offers these. The bridge decides what each chat gets (SPEC.md 9.3).
local NEW_LEVELS = { ask = true, ["auto-edit"] = true, ["full-auto"] = true }
-- The order of Shift+Tab, as in Claude Code.
local MODES = { "ask", "auto-edit", "full-auto" }

function Store.Load()
	local db = ns.Messages.Db()
	db.chats = db.chats or {}
	-- Deleted chats that the bridge has not heard of yet.
	db.forget = db.forget or {}
	db.restored = db.restored or false
	db.whisperColor = db.whisperColor or "f0a860"
	-- The desktop requests that already got their whisper line.
	db.desktopWhispered = db.desktopWhispered or {}
	db.fontSize = db.fontSize or DEFAULT_FONT_SIZE
	if db.whisperOn == nil then
		db.whisperOn = true
	end
	if db.whisperSound == nil then
		db.whisperSound = true
	end
	-- The notifications of terminal sessions (SPEC.md 10.4). The saved ids show nothing twice.
	for _, key in ipairs({ "notifyOn", "notifyChat", "notifySound", "notifyToast" }) do
		if db[key] == nil then
			db[key] = true
		end
	end
	db.notifyFinished = db.notifyFinished or "over1"
	db.noticesShown = db.noticesShown or {}
	db.noticesCleared = db.noticesCleared or 0
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

-- The level that a new chat asks for.
function Store.NewLevel()
	return NEW_LEVELS[Store.db.newLevel] and Store.db.newLevel or DEFAULT_MODE
end

-- The next message goes at this mode. The header shows it at once, until a run of the
-- bridge says which level applied.
function Store.SetMode(chat, mode)
	if not NEW_LEVELS[mode] then
		return
	end
	chat.mode = mode
	chat.level = nil
end

function Store.NextMode(mode)
	for i, known in ipairs(MODES) do
		if known == mode then
			return MODES[i % #MODES + 1]
		end
	end
	return MODES[1]
end

function Store.Modes()
	return MODES
end

function Store.NewChat(agent)
	local name = "Chat " .. (#Store.db.chats + 1)
	local chat = {
		id = ns.Messages.RandomId(10),
		name = name,
		-- The name comes back when the chat goes back to the default folder.
		defaultName = name,
		agent = agent or ns.BridgeSettings.NewChatAgent(DEFAULT_AGENT),
		mode = Store.NewLevel(),
		cwd = "",
		history = {},
		fresh = true,
	}
	table.insert(Store.db.chats, chat)
	return chat
end

local function NameTaken(name, except)
	for _, chat in ipairs(Store.db.chats) do
		if chat ~= except and chat.name == name then
			return true
		end
	end
	return false
end

-- Two chats in folders with one name get "app" and "app 2".
local function FreeName(name, chat)
	local free, n = name, 1
	while NameTaken(free, chat) do
		n = n + 1
		free = name .. " " .. n
	end
	return free
end

-- The chat takes the name of its folder, and a chat in the default folder keeps
-- "Chat N" (SPEC.md 9.9). A new folder waits for the first message.
function Store.SetFolder(chat, folder, name, isNew)
	chat.cwd = folder
	chat.newFolder = isNew or nil
	if folder == "" then
		chat.name = chat.defaultName or chat.name
	else
		chat.name = FreeName(name, chat)
	end
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
	-- The answer to a git action comes from the desktop app, never from the agent.
	if message.git then
		Append(chat, { role = "note", id = id, text = text })
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
