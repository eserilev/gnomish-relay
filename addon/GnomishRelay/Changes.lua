-- The blocks of the bridge under a reply (SPEC.md 9.11): the change summary with Commit
-- and Revert, the test line, and the CI line. The bridge doubled every | of their fields.

local _, ns = ...

local Changes = {}
ns.Changes = Changes

local GOLD = "ffd100"
local GREY = "9d9d9d"
local GREEN = "40ff40"
local RED = "ff4040"
local MINUS = "\226\136\146"
local BUTTON_WIDTH = 64
local ROW = 16
-- A commit message is one line.
local MAX_MESSAGE = 200

local ui = {}

local function Pool(create)
	return { free = {}, used = {}, create = create }
end

local function Acquire(pool)
	local widget = table.remove(pool.free) or pool.create()
	table.insert(pool.used, widget)
	widget:ClearAllPoints()
	widget:Show()
	return widget
end

local function Place(canvas, widget, x, y)
	widget:SetPoint("TOPLEFT", canvas.child, "TOPLEFT", x, -y)
end

local function Line(canvas, text, x, y, width)
	local line = Acquire(canvas.pools.text)
	line:SetWidth(width)
	line:SetText(text)
	Place(canvas, line, x, y)
	return y + math.max(line:GetStringHeight(), ROW - 2)
end

local function Counts(added, removed)
	return string.format("|cff%s+%d|r |cff%s%s%d|r", GREEN, added or 0, RED, MINUS, removed or 0)
end

local function Plural(count, one, many)
	return count == 1 and one or string.format(many, count)
end

-- "412 passed, 2 failed": each number that failed is red.
local function Tally(parts)
	local words = {}
	for _, part in ipairs(parts) do
		local count, word, bad = part[1], part[2], part[3]
		if count and count > 0 then
			local text = string.format("%d %s", count, word)
			table.insert(words, bad and string.format("|cff%s%s|r", RED, text) or text)
		end
	end
	return table.concat(words, ", ")
end

function Changes.TestsText(tests)
	return "Tests: "
		.. Tally({ { tests.passed, "passed" }, { tests.failed, "failed", true }, { tests.skipped, "skipped" } })
end

function Changes.CiText(ci)
	if (ci.passed or 0) + (ci.failed or 0) + (ci.running or 0) == 0 then
		return "CI: no checks on this pull request"
	end
	local text = "CI: " .. Tally({ { ci.passed, "passed" }, { ci.failed, "failed", true }, { ci.running, "running" } })
	if ci.names ~= "" then
		text = text .. " (" .. ci.names .. ")"
	end
	return text
end

-- The label of a git message in the transcript: `Commit "fix the test"`.
local LABELS = { commit = "Commit", revert = "Revert", merge = "Merge", discard = "Discard", checks = "Checks" }

function Changes.Label(entry)
	if not entry.git then
		return entry.text
	end
	local action = entry.git:match("^(%l+)")
	local label = LABELS[action] or "Git"
	if action == "commit" and entry.text ~= "" then
		return string.format('%s "%s"', label, entry.text)
	end
	return label
end

local function FileLine(file)
	local counts
	if file.kind == "A" then
		counts = string.format("|cff%snew|r", GREEN)
	elseif file.kind == "D" then
		counts = string.format("|cff%sremoved|r", RED)
	elseif file.added then
		counts = Counts(file.added, file.removed)
	else
		counts = ""
	end
	return string.format("|cff%s%s|r  %s", GREY, file.path, counts)
end

local DONE = { committed = "Committed", reverted = "Reverted", sending = "Sending..." }

local function Buttons(canvas, chat, entry, width, y)
	local done = DONE[entry.gitState or ""]
	if done then
		local label = Acquire(canvas.pools.text)
		label:SetWidth(2 * BUTTON_WIDTH)
		label:SetText(string.format("|cff%s%s|r", GREY, done))
		Place(canvas, label, width - 2 * BUTTON_WIDTH, y)
		return
	end
	for i, action in ipairs({ "commit", "revert" }) do
		local button = Acquire(canvas.pools.buttons)
		button:SetText(LABELS[action])
		button.chat, button.entry, button.action = chat, entry, action
		Place(canvas, button, width - (3 - i) * (BUTTON_WIDTH + 4), y - 2)
	end
end

local function DrawSummary(canvas, chat, entry, git, y, width)
	local summary = git.summary
	local head = string.format(
		"|cff%s%s|r  %s",
		GOLD,
		Plural(summary.files, "1 file changed", "%d files changed"),
		Counts(summary.added, summary.removed)
	)
	Buttons(canvas, chat, entry, width, y)
	y = Line(canvas, head, 0, y, width - 2 * (BUTTON_WIDTH + 4)) + 2
	for _, file in ipairs(git.files) do
		y = Line(canvas, FileLine(file), 12, y, width - 12)
	end
	if (git.more or 0) > 0 then
		y = Line(canvas, string.format("|cff%sand %d more|r", GREY, git.more), 12, y, width - 12)
	end
	return y
end

-- The blocks under an entry, or nothing for an entry with none. Returns the new bottom.
function Changes.Draw(canvas, chat, entry, y, width)
	local git = ns.Blocks.Git(entry.text)
	if not git then
		return y
	end
	if git.summary then
		y = DrawSummary(canvas, chat, entry, git, y + 2, width)
	end
	if git.tests then
		y = Line(canvas, Changes.TestsText(git.tests), 0, y, width)
	end
	if git.ci then
		y = Line(canvas, Changes.CiText(git.ci), 0, y, width)
	end
	return y
end

-- The runs whose summary names this message, so a later draw shows the new state.
local function Mark(chat, id, state)
	local first
	for _, entry in ipairs(chat.history) do
		if entry.id == id and (entry.role == "agent" or entry.role == "error") then
			entry.gitState = state
			first = first or entry
		end
	end
	if first then
		ns.Transcript.Redraw(first)
	end
end

-- The bridge answered a git message. Only a done reply changes a summary or the branch.
function Changes.Answered(chat, action, status)
	local verb, id = tostring(action):match("^(%l+):(%d+)$")
	id = tonumber(id)
	if id then
		local state = status == "done" and (verb == "commit" and "committed" or "reverted") or nil
		Mark(chat, id, state)
	elseif action == "discard" and status == "done" then
		chat.branch = nil
	end
end

local function Send(chat, action, text)
	ns.Transport.Git(chat, action, text or "")
	ns.Window.Refresh()
end

-- Commit and Revert name the run by the id of its reply, which is the id of its message.
local function Act(chat, entry, action, text)
	Mark(chat, entry.id, "sending")
	Send(chat, action .. ":" .. entry.id, text)
end

StaticPopupDialogs.GNOMISHRELAY_REVERT = {
	text = "%s",
	button1 = "Revert",
	button2 = "Cancel",
	OnAccept = function(_, target)
		Act(target.chat, target.entry, "revert")
	end,
	timeout = 0,
	whileDead = true,
	hideOnEscape = true,
	preferredIndex = 3,
}

function Changes.AskRevert(chat, entry)
	local git = ns.Blocks.Git(entry.text)
	local files = git and git.summary and git.summary.files or 0
	local question = string.format(
		"Revert the changes of this reply? This puts back %s as they were before it.",
		Plural(files, "1 file", "%d files")
	)
	StaticPopup_Show("GNOMISHRELAY_REVERT", question, nil, { chat = chat, entry = entry })
end

-- The first line of the message that started the run: the player's own words.
local function FirstLine(chat, id)
	local message = ns.Store.Message(chat, id)
	local text = message and message.text or ""
	return (text:match("^[^\n]*") or ""):sub(1, 72)
end

-- Grey, not disabled: Commit sends nothing for an empty message anyway, and Disable of a
-- button is a protected function of the client.
local function RefreshCommit()
	local text = strtrim(ui.message:GetText() or "")
	ui.commit:SetAlpha(text == "" and 0.45 or 1)
end

function Changes.CloseCommit()
	ui.dialog:Hide()
	ui.message:ClearFocus()
	ui.target = nil
end

-- A deleted chat takes its open Commit dialog with it.
function Changes.Forget(chat)
	if ui.target and ui.target.chat == chat then
		Changes.CloseCommit()
	end
end

local function Commit()
	local text = strtrim(ui.message:GetText() or "")
	local target = ui.target
	if text == "" or not target then
		return
	end
	Changes.CloseCommit()
	Act(target.chat, target.entry, "commit", text)
end

function Changes.AskCommit(chat, entry)
	ui.target = { chat = chat, entry = entry }
	ui.message:SetText(FirstLine(chat, entry.id))
	ui.dialog:Show()
	ui.message:SetFocus()
	RefreshCommit()
end

local buttons = 0

local function NewButton(child)
	buttons = buttons + 1
	local button = CreateFrame("Button", "GnomishRelayChangeButton" .. buttons, child, "UIPanelButtonTemplate")
	button:SetSize(BUTTON_WIDTH, 20)
	button:SetScript("OnClick", function(self)
		if self.action == "commit" then
			Changes.AskCommit(self.chat, self.entry)
		else
			Changes.AskRevert(self.chat, self.entry)
		end
	end)
	return button
end

local function NewText(child)
	local text = child:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	text:SetJustifyH("LEFT")
	return text
end

local function BuildDialog()
	local dialog = CreateFrame("Frame", "GnomishRelayCommit", UIParent)
	dialog:SetSize(420, 110)
	dialog:SetPoint("CENTER", UIParent, "CENTER", 0, 80)
	dialog:SetFrameStrata("DIALOG")
	dialog:EnableMouse(true)
	local border = CreateFrame("Frame", nil, dialog, "DialogBorderDarkTemplate")
	border:SetAllPoints()
	local title = dialog:CreateFontString("GnomishRelayCommitTitle", "OVERLAY", "GameFontNormal")
	title:SetPoint("TOP", dialog, "TOP", 0, -12)
	title:SetText("Commit changes")
	ui.message = CreateFrame("EditBox", "GnomishRelayCommitMessage", dialog, "InputBoxTemplate")
	ui.message:SetSize(380, 22)
	ui.message:SetPoint("TOP", title, "BOTTOM", 0, -12)
	ui.message:SetAutoFocus(false)
	ui.message:SetMaxBytes(MAX_MESSAGE)
	ui.message:SetScript("OnTextChanged", RefreshCommit)
	ui.message:SetScript("OnEnterPressed", Commit)
	ui.message:SetScript("OnEscapePressed", Changes.CloseCommit)
	ui.commit = CreateFrame("Button", "GnomishRelayCommitButton", dialog, "UIPanelButtonTemplate")
	ui.commit:SetSize(90, 22)
	ui.commit:SetPoint("BOTTOMRIGHT", dialog, "BOTTOM", -6, 12)
	ui.commit:SetText("Commit")
	ui.commit:SetScript("OnClick", Commit)
	local cancel = CreateFrame("Button", "GnomishRelayCommitCancel", dialog, "UIPanelButtonTemplate")
	cancel:SetSize(90, 22)
	cancel:SetPoint("BOTTOMLEFT", dialog, "BOTTOM", 6, 12)
	cancel:SetText("Cancel")
	cancel:SetScript("OnClick", Changes.CloseCommit)
	dialog:Hide()
	table.insert(UISpecialFrames, "GnomishRelayCommit")
	ui.dialog = dialog
end

-- `child` is the scroll child of a transcript. The transcript gives back the widgets of
-- the pools of the canvas, with its own.
function Changes.Canvas(child)
	local function Of(create)
		return Pool(function()
			return create(child)
		end)
	end
	return { child = child, pools = { text = Of(NewText), buttons = Of(NewButton) } }
end

function Changes.Build()
	BuildDialog()
end
