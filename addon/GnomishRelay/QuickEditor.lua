-- The editor of the quick actions (SPEC.md 13.1). The window shows it in place of the
-- Settings page while it is open.

local _, ns = ...

local QuickEditor = {}
ns.QuickEditor = QuickEditor

local ROW_HEIGHT = 30
local NAME_WIDTH = 140
local BUTTON_WIDTH = 80
local GREY = "8d8778"

local ui = { rows = {}, open = false }

local function Changed()
	QuickEditor.Refresh()
	ns.Window.Refresh()
end

local function ShownText(box)
	local action = ns.QuickActions.List()[box.row]
	return action and action[box.field] or ""
end

-- An empty or unchanged text keeps the old one, and the box shows it again.
local function Save(box)
	local text = box:GetText()
	if box.field == "name" then
		ns.QuickActions.Rename(box.row, text)
	else
		ns.QuickActions.SetMessage(box.row, text)
	end
	box:SetText(ShownText(box))
	ns.Window.Refresh()
end

-- A click on a button leaves the focus in the box, and Changed() then draws over the text.
local function SaveFocused()
	for _, row in ipairs(ui.rows) do
		for _, box in ipairs({ row.name, row.message }) do
			if box:HasFocus() then
				box:ClearFocus()
			end
		end
	end
end

local function NewBox(name, parent, row, field, bytes)
	local box = CreateFrame("EditBox", name, parent, "InputBoxTemplate")
	box:SetHeight(22)
	box:SetAutoFocus(false)
	box:SetMaxBytes(bytes)
	box.row, box.field = row, field
	box:SetScript("OnEnterPressed", function(self)
		self:ClearFocus()
	end)
	box:SetScript("OnEditFocusLost", Save)
	box:SetScript("OnEscapePressed", function(self)
		self:SetText(ShownText(self))
		self:ClearFocus()
	end)
	return box
end

local function NewButton(name, parent, text, width, onClick)
	local button = CreateFrame("Button", name, parent, "UIPanelButtonTemplate")
	button:SetSize(width, 22)
	button:SetText(text)
	button:SetScript("OnClick", onClick)
	return button
end

local function BuildRow(i)
	local prefix = "GnomishRelayQuickEdit"
	local row = CreateFrame("Frame", prefix .. "Row" .. i, ui.frame)
	row:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 20, -44 - (i - 1) * ROW_HEIGHT)
	row:SetPoint("TOPRIGHT", ui.frame, "TOPRIGHT", -20, -44 - (i - 1) * ROW_HEIGHT)
	row:SetHeight(ROW_HEIGHT)
	local remove = NewButton(prefix .. "Remove" .. i, row, "Remove", BUTTON_WIDTH, function()
		SaveFocused()
		ns.QuickActions.Remove(i)
		Changed()
	end)
	remove:SetPoint("RIGHT", row, "RIGHT", 0, 0)
	local down = NewButton(prefix .. "Down" .. i, row, "Move down", BUTTON_WIDTH, function()
		SaveFocused()
		ns.QuickActions.Move(i, 1)
		Changed()
	end)
	down:SetPoint("RIGHT", remove, "LEFT", -4, 0)
	local up = NewButton(prefix .. "Up" .. i, row, "Move up", BUTTON_WIDTH, function()
		SaveFocused()
		ns.QuickActions.Move(i, -1)
		Changed()
	end)
	up:SetPoint("RIGHT", down, "LEFT", -4, 0)
	row.name = NewBox(prefix .. "Name" .. i, row, i, "name", ns.QuickActions.NAME_BYTES)
	row.name:SetPoint("LEFT", row, "LEFT", 6, 0)
	row.name:SetWidth(NAME_WIDTH)
	row.message = NewBox(prefix .. "Message" .. i, row, i, "message", ns.QuickActions.MESSAGE_BYTES)
	row.message:SetPoint("LEFT", row.name, "RIGHT", 12, 0)
	row.message:SetPoint("RIGHT", up, "LEFT", -12, 0)
	row.up, row.down = up, down
	return row
end

function QuickEditor.Refresh()
	if not ui.frame or not ui.frame:IsShown() then
		return
	end
	local list = ns.QuickActions.List()
	for i, row in ipairs(ui.rows) do
		local action = list[i]
		row:SetShown(action ~= nil)
		if action then
			row.name:SetText(action.name)
			row.message:SetText(action.message)
			row.up:SetEnabled(i > 1)
			row.down:SetEnabled(i < #list)
		end
	end
	ui.add:SetShown(#list < ns.QuickActions.MOST)
end

function QuickEditor.IsOpen()
	return ui.open
end

function QuickEditor.Open()
	ui.open = true
	ns.Window.Refresh()
end

function QuickEditor.Close()
	ui.open = false
end

local function BuildButtons()
	local y = -44 - ns.QuickActions.MOST * ROW_HEIGHT - 8
	ui.add = NewButton("GnomishRelayQuickEditAdd", ui.frame, "Add", 90, function()
		SaveFocused()
		local i = ns.QuickActions.Add()
		Changed()
		if i then
			ui.rows[i].message:SetFocus()
		end
	end)
	ui.add:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 26, y)
	local reset = NewButton("GnomishRelayQuickEditReset", ui.frame, "Reset", 90, function()
		SaveFocused()
		ns.QuickActions.Reset()
		Changed()
	end)
	reset:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 26 + 96, y)
	local done = NewButton("GnomishRelayQuickEditDone", ui.frame, "Done", 90, function()
		QuickEditor.Close()
		ns.Window.Refresh()
	end)
	done:SetPoint("BOTTOMRIGHT", ui.frame, "BOTTOMRIGHT", -16, 12)
end

-- `page` is an inset in the place of the Settings page.
function QuickEditor.Build(page)
	ui.frame = page
	local heading = ui.frame:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
	heading:SetPoint("TOPLEFT", ui.frame, "TOPLEFT", 16, -12)
	heading:SetText("Quick actions")
	local hint = ui.frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	hint:SetPoint("LEFT", heading, "RIGHT", 12, 0)
	hint:SetText("|cff" .. GREY .. "One click sends the message to the open chat.|r")
	for i = 1, ns.QuickActions.MOST do
		ui.rows[i] = BuildRow(i)
	end
	BuildButtons()
end
