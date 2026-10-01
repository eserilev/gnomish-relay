-- The git part of the chat header (SPEC.md 9.11): the Own branch box of a new chat in a
-- repository, and after the first reply the branch with Merge, Discard, and Checks.

local _, ns = ...

local GitBar = {}
ns.GitBar = GitBar

local GREY = "9d9d9d"
local BUTTON_WIDTH = 64
local TIP = "Work on a separate branch in a separate copy, so other chats don't touch these files."

local ui = {}

-- A folder inside a repository, as the last folder tree shows it.
local function InRepository(folder)
	local node = ns.Folders.Find(ns.Folders.Tree(), folder)
	while node do
		if node.repo then
			return true
		end
		node = node.parent
	end
	return false
end

local function SharesFolder(chat)
	for _, other in ipairs(ns.Store.Chats()) do
		if other ~= chat and other.cwd == chat.cwd and #other.history > 0 then
			return true
		end
	end
	return false
end

-- The choice waits for the first message, and starts on when another chat has the folder.
local function Choosing(chat)
	if #chat.history > 0 or chat.newFolder or not InRepository(chat.cwd) then
		return false
	end
	if chat.ownBranch == nil then
		chat.ownBranch = SharesFolder(chat) or nil
	end
	return true
end

-- Merge and Discard only for an own branch, and the branch name left of the buttons.
local function ShowBranch(branch)
	local own = branch ~= nil and branch.own
	ui.merge:SetShown(own)
	ui.discard:SetShown(own)
	ui.checks:SetShown(branch ~= nil)
	ui.branch:SetShown(branch ~= nil)
	if not branch then
		return
	end
	ui.branch:SetText(string.format("|cff%s%s|r", GREY, branch.name))
	ui.branch:ClearAllPoints()
	ui.branch:SetPoint("RIGHT", own and ui.merge or ui.checks, "LEFT", -8, 0)
	-- A long name gets cut with "..." at the left edge of the transcript.
	ui.branch:SetPoint("LEFT", ui.parent, "TOPLEFT", ui.left, ui.y - 10)
end

-- The branch of the chat comes from the last reply with a branch block.
function GitBar.Refresh(chat)
	ui.chat = chat
	local choosing = chat ~= nil and Choosing(chat)
	ui.box:SetShown(choosing)
	if choosing then
		ui.box:SetChecked(chat.ownBranch == true)
	end
	ShowBranch(not choosing and chat and chat.branch or nil)
end

-- A reply with a branch block names the branch of the chat folder now.
function GitBar.Take(chat, text)
	local git = ns.Blocks.Git(text)
	if git and git.branch then
		chat.branch = git.branch
	end
end

StaticPopupDialogs.GNOMISHRELAY_DISCARD = {
	text = "%s",
	button1 = "Discard",
	button2 = "Cancel",
	OnAccept = function(_, chat)
		ns.Transport.Git(chat, "discard", "")
		ns.Window.Refresh()
	end,
	timeout = 0,
	whileDead = true,
	hideOnEscape = true,
	preferredIndex = 3,
}

local function AskDiscard()
	local chat = ui.chat
	if not chat or not chat.branch then
		return
	end
	local question = string.format("Discard this chat's branch? This deletes %s and its folder.", chat.branch.name)
	StaticPopup_Show("GNOMISHRELAY_DISCARD", question, nil, chat)
end

local function Send(action)
	if ui.chat then
		ns.Transport.Git(ui.chat, action, "")
		ns.Window.Refresh()
	end
end

local function Button(parent, name, text, onClick)
	local button = CreateFrame("Button", name, parent, "UIPanelButtonTemplate")
	button:SetSize(BUTTON_WIDTH, 20)
	button:SetText(text)
	button:SetScript("OnClick", onClick)
	button:Hide()
	return button
end

local function BuildBox(parent, right, y)
	ui.box = CreateFrame("CheckButton", "GnomishRelayOwnBranch", parent, "UICheckButtonTemplate")
	ui.box:SetSize(22, 22)
	ui.box:SetPoint("TOPRIGHT", parent, "TOPRIGHT", -right - 70, y + 1)
	ui.box.label = ui.box:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	ui.box.label:SetPoint("LEFT", ui.box, "RIGHT", 2, 0)
	ui.box.label:SetText("Own branch")
	ui.box:SetScript("OnClick", function(self)
		if ui.chat then
			ui.chat.ownBranch = self:GetChecked() and true or false
		end
	end)
	ui.box:SetScript("OnEnter", function(self)
		GameTooltip:SetOwner(self, "ANCHOR_BOTTOM")
		GameTooltip:SetText(TIP, 1, 1, 1, 1, true)
		GameTooltip:Show()
	end)
	ui.box:SetScript("OnLeave", function()
		GameTooltip:Hide()
	end)
	ui.box:Hide()
end

-- The header of a chat has Pinned and Search at its right end, so the bar takes the row
-- above the header, `right` in from the right edge and `y` down from the top.
function GitBar.Build(parent, right, y)
	ui.parent, ui.left, ui.y = parent, right, y
	BuildBox(parent, right, y)
	ui.checks = Button(parent, "GnomishRelayGitChecks", "Checks", function()
		Send("checks")
	end)
	ui.checks:SetPoint("TOPRIGHT", parent, "TOPRIGHT", -right, y)
	ui.discard = Button(parent, "GnomishRelayGitDiscard", "Discard", AskDiscard)
	ui.discard:SetPoint("RIGHT", ui.checks, "LEFT", -4, 0)
	ui.merge = Button(parent, "GnomishRelayGitMerge", "Merge", function()
		Send("merge")
	end)
	ui.merge:SetPoint("RIGHT", ui.discard, "LEFT", -4, 0)
	ui.branch = parent:CreateFontString("GnomishRelayGitBranch", "OVERLAY", "GameFontHighlightSmall")
	ui.branch:SetJustifyH("RIGHT")
	ui.branch:SetWordWrap(false)
	ui.branch:Hide()
end
