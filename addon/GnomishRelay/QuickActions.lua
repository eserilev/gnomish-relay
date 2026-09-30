-- The quick actions (SPEC.md 13.1): one list for all chats, in the saved variables.

local _, ns = ...

local QuickActions = {}
ns.QuickActions = QuickActions

QuickActions.MOST = 6
QuickActions.MESSAGE_BYTES = 1000

-- A name shows nowhere now. It keeps the saved list in the format of 0.3.0.
local DEFAULTS = {
	{ name = "Run tests", message = "Run the tests and tell me what fails" },
	{ name = "Fix tests", message = "Fix the failing tests" },
	{ name = "Git status", message = "Show git status" },
	{ name = "Summarize changes", message = "Summarize my uncommitted changes" },
	{ name = "Open PR", message = "Open a pull request for these changes" },
}

-- The messages of the 0.3.0 defaults: too long for a suggestion row.
local OLD_DEFAULTS = {
	"Run the tests. Tell me what passes and what fails. Change no code.",
	"Run the tests and fix each failure at its cause. Then run the tests again.",
	"Show the git status: the branch and the changed files. Change nothing.",
	"Summarize the changes that are not committed yet: what changed and why. Change nothing.",
	"Commit the changes on a new branch, push it, and open a pull request with a short title "
		.. "and description. Tell me the link.",
}

local function Defaults()
	local list = {}
	for i, action in ipairs(DEFAULTS) do
		list[i] = { name = action.name, message = action.message }
	end
	return list
end

local function IsAction(action)
	return type(action) == "table" and type(action.name) == "string" and type(action.message) == "string"
end

-- The saved variables can hold anything, so a list with one bad entry gives the defaults.
local function IsList(list)
	if type(list) ~= "table" or #list > QuickActions.MOST then
		return false
	end
	for _, action in ipairs(list) do
		if not IsAction(action) then
			return false
		end
	end
	return true
end

local function IsOldDefaults(list)
	if #list ~= #OLD_DEFAULTS then
		return false
	end
	for i, message in ipairs(OLD_DEFAULTS) do
		if list[i].message ~= message then
			return false
		end
	end
	return true
end

function QuickActions.Load()
	local db = ns.Store.db
	if not IsList(db.quickActions) or IsOldDefaults(db.quickActions) then
		db.quickActions = Defaults()
	end
end

function QuickActions.List()
	return ns.Store.db.quickActions
end

function QuickActions.Reset()
	ns.Store.db.quickActions = Defaults()
end

-- Returns the index of the new action, or nil when the list is full.
function QuickActions.Add()
	local list = QuickActions.List()
	if #list >= QuickActions.MOST then
		return nil
	end
	table.insert(list, { name = "New action", message = "" })
	return #list
end

function QuickActions.Remove(i)
	table.remove(QuickActions.List(), i)
end

-- A move past either end changes nothing.
function QuickActions.Move(i, step)
	local list = QuickActions.List()
	local j = i + step
	if not list[i] or not list[j] then
		return
	end
	list[i], list[j] = list[j], list[i]
end

-- An empty text keeps the old one.
function QuickActions.SetMessage(i, message)
	local action = QuickActions.List()[i]
	message = strtrim(message or "")
	if action and message ~= "" then
		action.message = message
	end
end

-- The actions that an empty chat suggests: those with a message.
function QuickActions.Ready()
	local ready = {}
	for _, action in ipairs(QuickActions.List()) do
		if action.message ~= "" then
			table.insert(ready, action)
		end
	end
	return ready
end
