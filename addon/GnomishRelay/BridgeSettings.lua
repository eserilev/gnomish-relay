-- The settings list of the bridge (SPEC.md 13.4): the values that the Settings and
-- Diag tabs show. The game only reads them. The saved variables keep the last list.

local _, ns = ...

local BridgeSettings = {}
ns.BridgeSettings = BridgeSettings

-- A list this old asks for a new one when a tab opens. Each ask costs a strip.
local FRESH_FOR = 600
local LEVELS = { ask = true, ["auto-edit"] = true, ["full-auto"] = true }

local function Lines(text)
	return (tostring(text) .. "\n"):gmatch("([^\n]*)\n")
end

-- A value can hold more tabs, so a line splits at its first tab only.
local function Split(line)
	local key, value = line:match("^([%l_]+)\t(.*)$")
	if not key or (value:gsub("\t", " ")):find("%c") then
		return nil
	end
	return key, value
end

local function Agent(value)
	local name, kind, level = value:match("^([^\t]*)\t([^\t]*)\t([^\t]*)$")
	if not ns.Codec.IsValidId(name) or not LEVELS[level] then
		return nil
	end
	return { name = name, kind = kind, level = level }
end

local function Folder(value)
	local folder, pattern = value:match("^([^\t]*)\t([^\t]*)$")
	if not folder or folder == "" or pattern == "" then
		return nil
	end
	return { folder = folder, pattern = pattern }
end

-- The keys that can come on many lines, and how each line of them reads.
local LISTS = {
	allowed_root = { field = "roots", Read = tostring },
	agent = { field = "agents", Read = Agent },
	allow = { field = "allow", Read = tostring },
	allow_folder = { field = "folders", Read = Folder },
}

-- Every other key once: the first value wins. A line that does not fit is left out.
function BridgeSettings.Parse(text)
	local parsed = { values = {}, roots = {}, agents = {}, allow = {}, folders = {}, cut = false }
	for line in Lines(text) do
		local key, value = Split(line)
		local list = LISTS[key]
		local item = list and list.Read(value)
		if line == "+" then
			parsed.cut = true
		elseif item then
			table.insert(parsed[list.field], item)
		elseif key and not list and not value:find("\t") and parsed.values[key] == nil then
			parsed.values[key] = value
		end
	end
	return parsed
end

local cache = {}

-- The last list, parsed once for each text. Nil before the first list.
function BridgeSettings.Last()
	local saved = ns.Store.db.settings
	if not saved or type(saved.text) ~= "string" then
		return nil
	end
	if cache.text ~= saved.text then
		cache.text, cache.parsed = saved.text, BridgeSettings.Parse(saved.text)
	end
	return cache.parsed
end

-- Seconds since the last list came, or nil.
function BridgeSettings.Age()
	local saved = ns.Store.db.settings
	return saved and saved.at and math.max(0, time() - saved.at) or nil
end

function BridgeSettings.Ask()
	ns.Transport.ListSettings()
end

function BridgeSettings.AskIfOld()
	local age = BridgeSettings.Age()
	if not age or age >= FRESH_FOR then
		BridgeSettings.Ask()
	end
end

function BridgeSettings.FindAgent(name)
	local last = BridgeSettings.Last()
	for _, agent in ipairs(last and last.agents or {}) do
		if agent.name == name then
			return agent
		end
	end
end

-- The agent of a new chat: the choice of the player while the bridge has it, else the
-- default agent of the bridge.
function BridgeSettings.NewChatAgent(default)
	local chosen = ns.Store.db.newAgent
	local last = BridgeSettings.Last()
	if not last then
		return chosen or default
	end
	if chosen and BridgeSettings.FindAgent(chosen) then
		return chosen
	end
	local bridgeDefault = last.values.default_agent
	return ns.Codec.IsValidId(bridgeDefault) and bridgeDefault or default
end
