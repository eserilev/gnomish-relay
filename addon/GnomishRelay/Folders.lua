-- The folder tree of the bridge and the rules of the folder browser (SPEC.md 9.9).

local _, ns = ...

local Folders = {}
ns.Folders = Folders

local MAX_NAME = 255
-- The game sends a folder back in each message of its chat.
local MAX_FOLDER = 255
local MAX_PATH = 1024
local CUT = "+"

local function HasControl(text)
	-- "\194[\128-\159]" is a control character of Unicode (C1) in UTF-8.
	return text:find("%c") ~= nil or text:find("\194[\128-\159]") ~= nil
end

-- The same rules as the bridge, so a name that passes here is never refused there.
-- Returns nil for a good name, else the short reason that the browser shows.
function Folders.CheckName(name, siblings)
	if name == "" or name == "." or name == ".." or #name > MAX_NAME then
		return "Name not allowed"
	end
	if name:find("[/\\]") or HasControl(name) then
		return "Name not allowed"
	end
	for _, sibling in ipairs(siblings) do
		if sibling:lower() == name:lower() then
			return "Already exists"
		end
	end
end

-- Each letter of `query` comes in `text` in order, without case.
function Folders.Matches(query, text)
	local at = 1
	local lower = text:lower()
	for i = 1, #query do
		local found = lower:find(query:sub(i, i):lower(), at, true)
		if not found then
			return false
		end
		at = found + 1
	end
	return true
end

local function Split(path, sep)
	local parts = {}
	for part in (path .. sep):gmatch("([^" .. sep .. "]*)" .. sep) do
		if part ~= "" then
			table.insert(parts, part)
		end
	end
	return parts
end

local function Copy(list)
	local out = {}
	for i, v in ipairs(list) do
		out[i] = v
	end
	return out
end

-- The path from `base` to `parts`. It is the form of the bridge, so the same folder
-- always gives the same text.
local function Relative(base, parts)
	local same = 0
	while same < #base and same < #parts and base[same + 1] == parts[same + 1] do
		same = same + 1
	end
	local out = {}
	for _ = same + 1, #base do
		table.insert(out, "..")
	end
	for i = same + 1, #parts do
		table.insert(out, parts[i])
	end
	return table.concat(out, "/")
end

-- A folder of the chat, as the player reads it: the default folder with `cwd` applied.
local function Apply(base, cwd)
	local parts = Copy(base)
	for _, part in ipairs(Split(cwd, "/")) do
		if part == ".." then
			table.remove(parts)
		elseif part ~= "." then
			table.insert(parts, part)
		end
	end
	return parts
end

local function Shown(tree, parts)
	return tree.lead .. table.concat(parts, "/")
end

local function AddNode(tree, number, parent, name, repo)
	local parts
	if parent == 0 then
		parts = Split(name, "/")
	else
		parts = Copy(tree.nodes[parent].parts)
		table.insert(parts, name)
	end
	local folder = Relative(tree.baseParts, parts)
	if #folder > MAX_FOLDER or tree.byFolder[folder] then
		return
	end
	local node = {
		name = parts[#parts] or name,
		parts = parts,
		folder = folder,
		repo = repo,
		children = {},
	}
	tree.nodes[number] = node
	tree.byFolder[folder] = node
	table.insert(tree.list, node)
	if parent == 0 then
		table.insert(tree.roots, node)
	else
		node.parent = tree.nodes[parent]
		table.insert(node.parent.children, node)
	end
end

-- A line that breaks a rule is left out, and so are its subfolders.
local function AddLine(tree, number, line)
	local parent, name, mark = line:match("^(%d+)\t([^\t]*)\t([^\t]*)$")
	parent = tonumber(parent)
	if not parent or parent >= number or (mark ~= "" and mark ~= "g") then
		return
	end
	if parent == 0 then
		if name == "" or #name > MAX_PATH or HasControl(name) then
			return
		end
	elseif not tree.nodes[parent] or Folders.CheckName(name, {}) then
		return
	end
	AddNode(tree, number, parent, name, mark == "g")
end

-- The first line is the default folder. Then one line per folder, breadth first:
-- `parent \t name \t mark`. A root has parent 0 and its whole path as its name.
function Folders.Parse(text)
	local tree = { nodes = {}, byFolder = {}, list = {}, roots = {}, cut = false }
	local number = 0
	for line in (tostring(text) .. "\n"):gmatch("([^\n]*)\n") do
		if number == 0 then
			tree.base = (#line <= MAX_PATH and not HasControl(line)) and line or ""
			tree.lead = tree.base:sub(1, 1) == "/" and "/" or ""
			tree.baseParts = Split(tree.base, "/")
		elseif line == CUT then
			tree.cut = true
		else
			AddLine(tree, number, line)
		end
		number = number + 1
	end
	return tree
end

local cache = {}

-- The last tree of the bridge, or nil before the first one.
function Folders.Tree()
	local saved = ns.Store.db.folders
	local text = saved and saved.text
	if type(text) ~= "string" then
		return nil
	end
	if cache.text ~= text then
		cache.text, cache.tree = text, Folders.Parse(text)
	end
	return cache.tree
end

function Folders.Find(tree, folder)
	return tree and tree.byFolder[folder]
end

-- The folder as the player reads it, for example `~/Code/app`.
function Folders.Display(tree, folder)
	if not tree or tree.base == "" then
		return folder
	end
	return Shown(tree, Apply(tree.baseParts, folder))
end

-- The last part of the folder, the name of a chat in it.
function Folders.Label(tree, folder)
	local node = Folders.Find(tree, folder)
	if node then
		return node.name
	end
	local parts = tree and Apply(tree.baseParts, folder) or Split(folder, "/")
	return parts[#parts] or folder
end

-- The folder above, as the player reads it, to tell two folders with one name apart.
function Folders.ParentDisplay(tree, folder)
	local parts = tree and Apply(tree.baseParts, folder) or Split(folder, "/")
	table.remove(parts)
	return (tree and tree.lead or "") .. table.concat(parts, "/")
end

local function Before(a, b)
	if a.repo ~= b.repo then
		return a.repo
	end
	if #a.folder ~= #b.folder then
		return #a.folder < #b.folder
	end
	return a.folder < b.folder
end

-- Repositories first, then the shorter paths.
function Folders.Search(tree, query, max)
	local found = {}
	for _, node in ipairs(tree and tree.list or {}) do
		if Folders.Matches(query, node.folder) then
			table.insert(found, node)
		end
	end
	table.sort(found, Before)
	for i = #found, max + 1, -1 do
		found[i] = nil
	end
	return found
end

local function AddRecent(recents, seen, tree, folder)
	if type(folder) ~= "string" or seen[folder] then
		return
	end
	if tree and not Folders.Find(tree, folder) then
		return
	end
	seen[folder] = true
	table.insert(recents, folder)
end

-- The folders of the newest chats, then of the Resume list. A folder that the last
-- tree does not have is gone, so it does not show.
function Folders.Recents(tree, skipChat, max)
	local db = ns.Store.db
	local recents, seen = {}, {}
	for i = #db.chats, 1, -1 do
		local chat = db.chats[i]
		if chat.id ~= skipChat and not chat.newFolder then
			AddRecent(recents, seen, tree, chat.cwd)
		end
	end
	for _, row in ipairs(db.sessions and db.sessions.rows or {}) do
		AddRecent(recents, seen, tree, row.folder)
	end
	for i = #recents, max + 1, -1 do
		recents[i] = nil
	end
	return recents
end

function Folders.ChildNames(node)
	local names = {}
	for _, child in ipairs(node.children) do
		table.insert(names, child.name)
	end
	return names
end

-- The folder of a new subfolder. A new name is never a part of the default folder, so
-- the text is already in the form of the bridge.
function Folders.Join(node, name)
	if node.folder == "" then
		return name
	end
	return node.folder .. "/" .. name
end
