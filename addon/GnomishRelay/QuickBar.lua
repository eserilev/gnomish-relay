-- The row of quick action buttons between the transcript and the input (SPEC.md 13.1).

local _, ns = ...

local QuickBar = {}
ns.QuickBar = QuickBar

local HEIGHT = 20
local GAP = 4
local PAD = 8

local ui = { buttons = {} }

local function ShowTooltip(button)
	if not button.action then
		return
	end
	GameTooltip:SetOwner(button, "ANCHOR_TOP")
	GameTooltip:SetText(button.action.name)
	GameTooltip:AddLine(button.action.message, 1, 1, 1, true)
	GameTooltip:Show()
end

-- A click is a typed message: Window.Send signs it and checks its size.
local function Send(button)
	if button.action then
		ns.Window.Send(button.action.message)
	end
end

local function NewButton(i)
	local button = CreateFrame("Button", "GnomishRelayQuick" .. i, ui.row)
	button:SetHeight(HEIGHT)
	local background = button:CreateTexture(nil, "BACKGROUND")
	background:SetAllPoints()
	background:SetColorTexture(0.1, 0.15, 0.2, 0.9)
	button:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight", "ADD")
	button.label = button:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
	button.label:SetPoint("CENTER", button, "CENTER", 0, 0)
	button.label:SetWordWrap(false)
	button:SetScript("OnClick", Send)
	button:SetScript("OnEnter", ShowTooltip)
	button:SetScript("OnLeave", function()
		GameTooltip:Hide()
	end)
	return button
end

local function TextWidth(text)
	ui.measure:SetText(text)
	return ui.measure:GetUnboundedStringWidth()
end

-- Each button gets the width of its name. When the names do not fit, all share the row.
local function Widths(actions)
	local widths, total = {}, -GAP
	for i, action in ipairs(actions) do
		widths[i] = TextWidth(action.name) + 2 * PAD
		total = total + widths[i] + GAP
	end
	if total <= ui.width then
		return widths
	end
	local each = math.floor((ui.width - GAP * (#actions - 1)) / #actions)
	for i in ipairs(widths) do
		widths[i] = each
	end
	return widths
end

local function Layout()
	local actions = ns.QuickActions.Ready()
	local widths = Widths(actions)
	local x = 0
	for i, button in ipairs(ui.buttons) do
		local action = actions[i]
		button.action = action
		button:SetShown(action ~= nil)
		if action then
			button:SetWidth(widths[i])
			button.label:SetWidth(widths[i] - PAD)
			button.label:SetText(action.name)
			button:SetPoint("TOPLEFT", ui.row, "TOPLEFT", x, 0)
			x = x + widths[i] + GAP
		end
	end
	return #actions
end

-- `shown` is false where the row has no room: another tab, the Resume picker, the banner,
-- or the byte counter of the input.
function QuickBar.Refresh(shown)
	if not ui.row then
		return
	end
	ui.row:SetShown(shown and Layout() > 0)
end

function QuickBar.Resize(width)
	ui.width = width
end

-- The row sits at `bottom` above the lower edge of `frame`, `left` in from each side.
function QuickBar.Build(frame, left, bottom, width)
	ui.row = CreateFrame("Frame", "GnomishRelayQuickBar", frame)
	ui.row:SetPoint("BOTTOMLEFT", frame, "BOTTOMLEFT", left, bottom)
	ui.row:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -left, bottom)
	ui.row:SetHeight(HEIGHT)
	ui.measure = ui.row:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
	ui.measure:Hide()
	for i = 1, ns.QuickActions.MOST do
		ui.buttons[i] = NewButton(i)
	end
	QuickBar.Resize(width)
	ui.row:Hide()
end
