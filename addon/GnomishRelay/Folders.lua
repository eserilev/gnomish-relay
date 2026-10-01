-- The folder tree of the bridge and the rules of the folder browser (SPEC.md 9.9).

local _, ns = ...

local Folders = {}
ns.Folders = Folders

local MAX_NAME = 255
-- The game sends a folder back in each message of its chat.
local MAX_FOLDER = 255
local MAX_PATH = 1024
local CUT = "+"
local NOT_WALKED = "?"
-- The listings of single folders that the browser keeps until /reload.
local MAX_LISTINGS = 64

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

-- `parent` is nil for a root. Returns nil for a folder that the tree already has.
local function NewNode(tree, parent, parts, name, repo)
	local folder = Relative(tree.baseParts, parts)
	if #folder > MAX_FOLDER or tree.byFolder[folder] then
		return nil
	end
	local node = {
		name = parts[#parts] or name,
		parts = parts,
		folder = folder,
		repo = repo,
		children = {},
		parent = parent,
	}
	tree.byFolder[folder] = node
	table.insert(tree.list, node)
	table.insert(parent and parent.children or tree.roots, node)
	return node
end

local function ChildParts(parent, name)
	local parts = Copy(parent.parts)
	table.insert(parts, name)
	return parts
end

local function AddNode(tree, number, parent, name, repo)
	local parts
	if parent == 0 then
		parts = Split(name, "/")
	else
		parts = ChildParts(tree.nodes[parent], name)
	end
	tree.nodes[number] = NewNode(tree, tree.nodes[parent], parts, name, repo)
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

-- `?2-4,9`: the folder lines whose subfolders the reply does not hold in full.
-- A range only marks lines that exist, so a wild range costs nothing.
local function MarkNotWalked(tree, ranges, lines)
	for range in (ranges .. ","):gmatch("([^,]*),") do
		local first, last = range:match("^(%d+)%-(%d+)$")
		first = tonumber(first or range)
		last = math.min(tonumber(last or range) or 0, lines)
		for number = first or 1, first and last or 0 do
			if tree.nodes[number] then
				tree.nodes[number].unwalked = true
			end
		end
	end
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
		elseif line:sub(1, 1) == NOT_WALKED then
			MarkNotWalked(tree, line:sub(2), number)
		else
			AddLine(tree, number, line)
		end
		number = number + 1
	end
	return tree
end

local function ByName(a, b)
	return a.name < b.name
end

-- A listing adds only below folders that the tree has, so it never widens the tree.
local function Merge(tree, text)
	local listing = Folders.Parse(text)
	if listing.base ~= tree.base then
		return
	end
	for _, node in ipairs(listing.list) do
		local known = tree.byFolder[node.folder]
		local parent = node.parent and tree.byFolder[node.parent.folder]
		if known then
			known.unwalked = node.unwalked
		elseif parent then
			local added = NewNode(tree, parent, ChildParts(parent, node.name), node.name, node.repo)
			if added then
				added.unwalked = node.unwalked
				table.sort(parent.children, ByName)
			end
		end
	end
end

local listings = {}
-- Counts the listings so far, so the cache of the tree knows a new one.
local listingCount = 0

-- The reply of a request for one folder (SPEC.md 9.9, "One folder").
function Folders.AddListing(text)
	table.insert(listings, tostring(text))
	listingCount = listingCount + 1
	if #listings > MAX_LISTINGS then
		table.remove(listings, 1)
	end
end

local cache = {}

-- The last tree of the bridge with the listings of single folders, or nil before the
-- first tree.
function Folders.Tree()
	local saved = ns.Store.db.folders
	local text = saved and saved.text
	if type(text) ~= "string" then
		return nil
	end
	if cache.text ~= text or cache.listingCount ~= listingCount then
		cache.text, cache.listingCount = text, listingCount
		cache.tree = Folders.Parse(text)
		for _, listing in ipairs(listings) do
			Merge(cache.tree, listing)
		end
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
