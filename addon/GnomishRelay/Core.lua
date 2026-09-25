-- Startup, slash commands, and the whisper line in game chat.

local addonName, ns = ...

local Relay = {}
ns.Relay = Relay

local AGENTS = {
	claude = { name = "Claude", color = "ff7d0a" },
	codex = { name = "Codex", color = "abd473" },
}
local SNIPPET = 120
-- Long enough for the first strip and the first polls after a login.
local BRIDGE_WAIT = 60

function Relay.AgentName(agent)
	local known = AGENTS[agent]
	return known and known.name or (agent:gsub("^%l", string.upper))
end

function Relay.AgentColor(agent)
	local known = AGENTS[agent]
	return known and known.color or "ffd100"
end

-- WoW reads "|" as the start of an escape code, and "||" shows one "|".
function Relay.Plain(text)
	return (tostring(text or ""):gsub("|", "||"))
end

local function Snippet(text)
	local line = tostring(text or ""):match("^[^\n]*")
	if #line > SNIPPET then
		line = line:sub(1, SNIPPET) .. "..."
	end
	return line
end

local function Whisper(chat, reply)
	local shown = ns.Window.Showing(chat.id)
	if not shown then
		chat.unread = true
	end
	DEFAULT_CHAT_FRAME:AddMessage(
		string.format(
			"|cff%s|Hgnomishrelay:%s|h[%s]|h whispers: [%s] %s|r",
			ns.Store.db.whisperColor,
			chat.id,
			Relay.AgentName(chat.agent),
			Relay.Plain(chat.name),
			Relay.Plain(Snippet(reply.text))
		)
	)
	PlaySound(SOUNDKIT.TELL_MESSAGE)
end

local function SetCVarValue(name, value)
	if C_CVar and C_CVar.SetCVar then
		C_CVar.SetCVar(name, value)
	else
		SetCVar(name, value)
	end
end

local function LastMessage(chat)
	for i = #chat.history, 1, -1 do
		if chat.history[i].role == "user" then
			return chat.history[i].id
		end
	end
end

local function Diag()
	local chat = ns.Store.Chat(ns.Store.db.selected or "")
	if chat then
		print(string.format("Gnomish Relay: chat %s, last message %s", chat.id, tostring(LastMessage(chat))))
	end
	local s = ns.Transport.Stats()
	print(
		string.format(
			"Gnomish Relay: slot %d, %d left, reported %s, %d open, %d in outbox, bridge %s",
			s.nextSlot,
			s.slotsLeft,
			tostring(s.reported),
			s.open,
			s.outbox,
			s.online and "online" or "offline"
		)
	)
	print(ns.Health.Line())
end

local function Command(arg)
	arg = strtrim(arg or "")
	if arg == "" then
		ns.Window.Toggle()
	elseif arg == "diag" then
		Diag()
	elseif arg == "poll" then
		ns.Transport.Poll()
	else
		print("/relay | /relay diag | /relay poll | /ai <message>")
	end
end

local function Ask(text)
	text = strtrim(text or "")
	if text ~= "" then
		ns.Window.Send(text)
	end
end

local events = CreateFrame("Frame")
events:RegisterEvent("ADDON_LOADED")
events:RegisterEvent("PLAYER_LOGIN")
events:SetScript("OnEvent", function(_, event, name)
	if event == "ADDON_LOADED" and name == addonName then
		ns.Store.Load()
	elseif event == "PLAYER_LOGIN" then
		local missing = ns.Health.Missing()
		if missing then
			print(string.format("Gnomish Relay: this game version has no %s. The relay is off.", missing))
			return
		end
		if not ns.key then
			print("Gnomish Relay: run gnomish-relay setup. Get it at github.com/eserilev/gnomish-relay")
			return
		end
		SetCVarValue("screenshotFormat", "png")
		ns.Transport.OnChange = function()
			ns.Window.Refresh()
			ns.Popup.Refresh()
		end
		ns.Transport.OnReply = function(chat, reply)
			Whisper(chat, reply)
			ns.Window.Refresh()
		end
		ns.Transport.Init()
		C_Timer.NewTicker(1, ns.Transport.Tick)
		C_Timer.After(BRIDGE_WAIT, function()
			if not ns.Transport.Online() then
				print("Gnomish Relay: bridge not running.")
			end
		end)
	end
end)

hooksecurefunc("SetItemRef", function(link)
	local chatId = type(link) == "string" and link:match("^gnomishrelay:([%w_-]+)$")
	if chatId then
		ns.Window.Open(chatId)
	end
end)

SLASH_GNOMISHRELAY1 = "/relay"
SlashCmdList.GNOMISHRELAY = Command

SLASH_GNOMISHRELAYASK1 = "/ai"
SlashCmdList.GNOMISHRELAYASK = Ask
