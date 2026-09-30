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
	if ns.Blocks.IsRendered(text) then
		text = ns.Blocks.Plain(text)
	end
	local line = tostring(text or ""):match("^[^\n]*")
	if #line > SNIPPET then
		line = line:sub(1, SNIPPET) .. "..."
	end
	return line
end

local function WhisperLine(chat, text)
	local db = ns.Store.db
	DEFAULT_CHAT_FRAME:AddMessage(
		string.format(
			"|cff%s|Hgnomishrelay:%s|h[%s]|h whispers: [%s] %s|r",
			db.whisperColor,
			chat.id,
			Relay.AgentName(chat.agent),
			Relay.Plain(chat.name),
			Relay.Plain(text)
		)
	)
	if db.whisperSound then
		PlaySound(SOUNDKIT.TELL_MESSAGE)
	end
end

-- A click on the line opens the Settings tab, where the rule can go (SPEC.md 6.6.5).
function Relay.RuleAdded(chatId, line)
	local chat = ns.Store.Chat(chatId)
	if not chat then
		return
	end
	ns.BridgeSettings.MarkOld()
	DEFAULT_CHAT_FRAME:AddMessage(
		string.format(
			"|cff%s|Hgnomishrelayrules|h[%s] whispers: [%s] Always allowed now: %s. Click to manage your rules.|h|r",
			ns.Store.db.whisperColor,
			Relay.AgentName(chat.agent),
			Relay.Plain(chat.name),
			Relay.Plain(line)
		)
	)
end

-- The reply line is a setting. A desktop request always gets its line: it is the
-- only notice in the game (SPEC.md 6.6.3).
local function Whisper(chat, reply)
	local shown = ns.Window.Showing(chat.id)
	if not shown then
		chat.unread = true
	end
	if ns.Store.db.whisperOn then
		WhisperLine(chat, Snippet(reply.text))
	end
end

-- The id comes from the bridge, so no agent text is in this line.
local function DesktopText(chat, notice)
	if notice.how == "command" then
		return "Approve on your desktop: run gnomish-relay approve " .. notice.id
	elseif notice.raise then
		return string.format("Approve on your desktop to let %s work at %s.", Relay.AgentName(chat.agent), notice.raise)
	end
	return "Approve on your desktop."
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

-- The lines of /relay diag. The Diag tab shows them too.
function Relay.DiagLines()
	local lines = {}
	local chat = ns.Store.Chat(ns.Store.db.selected or "")
	if chat then
		table.insert(
			lines,
			string.format("Gnomish Relay: chat %s, last message %s", chat.id, tostring(LastMessage(chat)))
		)
	end
	local s = ns.Transport.Stats()
	table.insert(
		lines,
		string.format(
			"Gnomish Relay: slot %d, %d left, reported %s, %d open, %d in outbox, desktop app %s",
			s.nextSlot,
			s.slotsLeft,
			tostring(s.reported),
			s.open,
			s.outbox,
			s.online and "online" or "offline"
		)
	)
	table.insert(lines, ns.Health.Line())
	return lines
end

local function Diag()
	for _, line in ipairs(Relay.DiagLines()) do
		print(line)
	end
end

local function Command(arg)
	arg = strtrim(arg or "")
	if arg == "" then
		ns.Window.Toggle()
	elseif arg == "diag" then
		Diag()
	elseif arg == "poll" then
		ns.Transport.Poll()
	elseif arg:match("^size %d+$") then
		ns.Window.SetFontSize(tonumber(arg:match("%d+")))
	else
		print(
			"Gnomish Relay commands: /relay opens the window, /relay diag, /relay poll, /relay size 12-20, /ai <message>"
		)
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
events:RegisterEvent("PLAYER_REGEN_ENABLED")
events:SetScript("OnEvent", function(_, event, name)
	if event == "ADDON_LOADED" and name == addonName then
		ns.Store.Load()
		ns.QuickActions.Load()
	elseif event == "PLAYER_REGEN_ENABLED" then
		ns.Notices.CombatEnded()
	elseif event == "PLAYER_LOGIN" then
		local missing = ns.Health.Missing()
		if missing then
			print(
				string.format(
					"Gnomish Relay is off: this version of the game has no %s. On your desktop, run gnomish-relay update.",
					missing
				)
			)
			return
		end
		if not ns.key then
			print(
				"Gnomish Relay isn't set up yet. Get the desktop app at github.com/eserilev/gnomish-relay, then run gnomish-relay setup."
			)
			return
		end
		SetCVarValue("screenshotFormat", "png")
		ns.Messages.OnChange = function()
			ns.Window.Refresh()
			ns.Popup.Refresh()
		end
		ns.Transport.OnReply = function(chat, reply)
			Whisper(chat, reply)
			ns.Window.Refresh()
		end
		ns.Transport.OnDesktop = function(chat, notice)
			WhisperLine(chat, DesktopText(chat, notice))
		end
		ns.NoticeFrames.Build()
		ns.Transport.Init()
		C_Timer.NewTicker(1, ns.Transport.Tick)
		C_Timer.After(BRIDGE_WAIT, function()
			if not ns.Transport.Online() then
				print("Gnomish Relay: the desktop app isn't running. On your desktop, run gnomish-relay restart.")
			end
		end)
	end
end)

hooksecurefunc("SetItemRef", function(link)
	if link == "gnomishrelayrules" then
		ns.Window.Open()
		ns.Window.ShowTab("settings")
		return
	end
	if link == ns.Notices.LINK then
		ns.NoticeFrames.OpenList()
		return
	end
	local chatId = type(link) == "string" and link:match("^gnomishrelay:([%w_-]+)$")
	if chatId then
		ns.Window.Open(chatId)
	end
end)

-- The key binding of Bindings.xml. WoW shows these names in its Key Bindings menu.
BINDING_HEADER_GNOMISHRELAY = "Gnomish Relay"
BINDING_NAME_GNOMISHRELAY_TOGGLE = "Toggle window"
function GnomishRelay_Toggle()
	ns.Window.Toggle()
end

SLASH_GNOMISHRELAY1 = "/relay"
SlashCmdList.GNOMISHRELAY = Command

SLASH_GNOMISHRELAYASK1 = "/ai"
SlashCmdList.GNOMISHRELAYASK = Ask
