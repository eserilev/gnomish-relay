-- The "Always Allowed" group of the Settings tab (SPEC.md 6.6.5): one row for each rule
-- of the settings list, with a remove button. A removal only narrows what runs, so the
-- game may send it.

local _, ns = ...

local RulesGroup = {}
ns.RulesGroup = RulesGroup

local ROWS = 6
local ROW_HEIGHT = 20
local GREY = "8d8778"
local CODE = "b8c8b8"

local ui = { rows = {}, offset = 0, removing = {}, shown = ROWS }

local function Plain(text)
	return ns.Relay.Plain(text)
end

local function LastUse(days)
	if days == 0 then
		return "today"
	elseif days == 1 then
		return "1 day ago"
	end
	return days .. " days ago"
end

-- The rules of the last list. A removed rule stays grey until a newer list comes.
local function Rules()
	local last = ns.BridgeSettings.Last()
	return last and last.rules or {}
end

local function IsRemoving(rule)
	local asked = ui.removing[rule.id]
	return asked ~= nil and (ns.BridgeSettings.Age() or 0) >= time() - asked
end

local function ShowRow(row, rule)
	if not rule then
		row:Hide()
		return
	end
	local removing = IsRemoving(rule)
	row.rule = rule
	row.pattern:SetText("|cff" .. CODE .. Plain(rule.pattern) .. "|r")
	row.folder:SetText(Plain(rule.folder))
	row.used:SetText("|cff" .. GREY .. (removing and "Removing..." or LastUse(rule.days)) .. "|r")
	row:SetAlpha(removing and 0.45 or 1)
	row:Show()
end

function RulesGroup.Refresh()
	if not ui.heading then
		return
	end
	local rules = Rules()
	ui.offset = math.max(0, math.min(ui.offset, #rules - ui.shown))
	ui.empty:SetShown(#rules == 0)
	for i, row in ipairs(ui.rows) do
		ShowRow(row, i <= ui.shown and rules[ui.offset + i] or nil)
	end
end

local function Remove(rule)
	ui.removing[rule.id] = time()
	ns.Transport.RemoveRule(rule.id)
	RulesGroup.Refresh()
end

local function Text(parent, font, x, width)
	local text = parent:CreateFontString(nil, "OVERLAY", font)
	text:SetPoint("LEFT", parent, "LEFT", x, 0)
	text:SetWidth(width)
	text:SetJustifyH("LEFT")
	text:SetWordWrap(false)
	return text
end

local function BuildRow(page, i)
	local row = CreateFrame("Frame", "GnomishRelayRule" .. i, page)
	row:SetSize(page:GetWidth() - 40, ROW_HEIGHT)
	row.pattern = Text(row, "GameFontHighlight", 0, 190)
	row.folder = Text(row, "GameFontHighlight", 200, 220)
	row.used = Text(row, "GameFontHighlightSmall", 430, 110)
	row.remove = CreateFrame("Button", "GnomishRelayRuleRemove" .. i, row, "UIPanelButtonTemplate")
	row.remove:SetSize(24, 18)
	row.remove:SetPoint("RIGHT", row, "RIGHT", 0, 0)
	row.remove:SetText("x")
	row.remove:SetScript("OnClick", function()
		if row.rule and not IsRemoving(row.rule) then
			Remove(row.rule)
		end
	end)
	row:Hide()
	return row
end

-- `y` is the top of the group on the page. The group shows `rows` rules at a time, so
-- it fits below the Notifications group (SPEC.md 13.1).
function RulesGroup.Place(y, rows)
	local page = ui.page
	ui.shown = rows
	ui.heading:ClearAllPoints()
	ui.heading:SetPoint("TOPLEFT", page, "TOPLEFT", 16, y)
	ui.empty:ClearAllPoints()
	ui.empty:SetPoint("TOPLEFT", page, "TOPLEFT", 20, y - 30)
	for i, row in ipairs(ui.rows) do
		row:ClearAllPoints()
		row:SetPoint("TOPLEFT", page, "TOPLEFT", 20, y - 28 - (i - 1) * ROW_HEIGHT)
	end
	ui.area:ClearAllPoints()
	ui.area:SetPoint("TOPLEFT", page, "TOPLEFT", 16, y - 24)
	ui.area:SetSize(page:GetWidth() - 32, rows * ROW_HEIGHT + 8)
	RulesGroup.Refresh()
end

function RulesGroup.Build(page, y)
	ui.page = page
	ui.heading = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
	ui.heading:SetText("Always allowed")
	local hint = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
	hint:SetPoint("LEFT", ui.heading, "RIGHT", 12, 0)
	hint:SetText("|cff" .. GREY .. "Rules expire after 30 days without use.|r")
	ui.empty = page:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
	ui.empty:SetText("|cff" .. GREY .. "No rules yet. Click Always allow in a popup to add one.|r")
	for i = 1, ROWS do
		ui.rows[i] = BuildRow(page, i)
	end
	ui.area = CreateFrame("Frame", "GnomishRelayRules", page)
	ui.area:EnableMouseWheel(true)
	ui.area:SetScript("OnMouseWheel", function(_, delta)
		ui.offset = ui.offset - delta
		RulesGroup.Refresh()
	end)
	RulesGroup.Place(y, ROWS)
end
