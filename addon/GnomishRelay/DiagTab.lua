-- The Diag tab of the window (SPEC.md 13.1): what the bridge allows, from the last
-- settings list (SPEC.md 13.4), and the lines of /relay diag. It only reads.

local _, ns = ...

local DiagTab = {}
ns.DiagTab = DiagTab

local LINES = 26
local LINE_HEIGHT = 17
local LABEL_WIDTH = 150
local GOLD = "ffd100"
local GREY = "8d8778"

local ui = {}

local function Heading(text)
	return { heading = text }
end

local function Row(label, value)
	return { label = label, value = value }
end

-- A value of the bridge is grey while the bridge is offline: it can be old.
local function BridgeRow(label, value)
	return { label = label, value = value, bridge = true }
end

-- The label shows on the first row of a group only.
local function Group(label, values, rows)
	for i, value in ipairs(values) do
		table.insert(rows, BridgeRow(i == 1 and label or "", value))
	end
end

local function Plain(text)
	return ns.Relay.Plain(text)
end

-- The allow table: the patterns of every chat, then the patterns of each folder.
local function Commands(last)
	local values = {}
	for _, pattern in ipairs(last.allow) do
		table.insert(values, Plain(pattern))
	end
	for _, rule in ipairs(last.folders) do
		table.insert(values, Plain(rule.pattern) .. "  in " .. Plain(rule.folder))
	end
	if last.cut then
		table.insert(values, "...")
	end
	return values
end

local function Agents(last)
	local values = {}
	for _, agent in ipairs(last.agents) do
		table.insert(values, Plain(agent.name) .. "  " .. Plain(agent.level))
	end
	return values
end

local function Roots(last)
	local values = {}
	for _, root in ipairs(last.roots) do
		table.insert(values, Plain(root))
	end
	return values
end

local function BridgeRows(last, rows)
	local v = last.values
	table.insert(rows, Row("Status", ns.SettingsTab.Status()))
	Group("Allowed", Roots(last), rows)
	table.insert(rows, BridgeRow("Default folder", Plain(v.default_cwd or "")))
	Group("Agents", Agents(last), rows)
	Group("Commands", Commands(last), rows)
	local timeouts =
		string.format("%s min · approvals %s min", v.timeout_minutes or "?", v.permission_timeout_minutes or "?")
	table.insert(rows, BridgeRow("Timeout", Plain(timeouts)))
	table.insert(rows, BridgeRow("Sandbox", Plain(v.sandbox or "")))
end

local function StoryRows(v, rows)
	if not v.story_model then
		return
	end
	table.insert(rows, Heading("Timeways"))
	table.insert(rows, BridgeRow("Model", Plain(v.story_model)))
	local budget = string.format("10 calls / %s min", v.story_budget_window_minutes or "?")
	table.insert(rows, BridgeRow("Budget", Plain(budget)))
end

local HOOK_WORDS =
	{ on = "on", off = "off", moved = "moved: run gnomish-relay hooks install", disabled = "hooks turned off" }

local function Hooks(last)
	local parts = {}
	for _, hook in ipairs(last.hooks) do
		table.insert(parts, ns.Relay.AgentName(hook.agent) .. " " .. HOOK_WORDS[hook.state])
	end
	return table.concat(parts, " · ")
end

local function LastNotification()
	local newest = ns.Notices.List()[1]
	if not newest then
		return "none"
	end
	return ns.Notices.Duration(math.floor(ns.Notices.Age(newest))) .. " ago"
end

-- Only after `gnomish-relay hooks install` (SPEC.md 10.4).
local function NoticeRows(last, rows)
	if not ns.BridgeSettings.HooksOn() then
		return
	end
	local busy, open = ns.Notices.Sessions()
	table.insert(rows, Heading("Notifications"))
	table.insert(rows, BridgeRow("Hooks", Plain(Hooks(last))))
	table.insert(rows, BridgeRow("Sessions", string.format("%d running · %d open", busy, open)))
	table.insert(rows, BridgeRow("Last notification", LastNotification()))
end

local function Rows()
	local rows = { Heading("Desktop app") }
	local last = ns.BridgeSettings.Last()
	if not last then
		table.insert(rows, Row("Status", ns.SettingsTab.Status()))
	else
		BridgeRows(last, rows)
		StoryRows(last.values, rows)
		NoticeRows(last, rows)
	end
	table.insert(rows, Heading("Versions"))
	local bridge = last and last.values.version or "?"
	table.insert(rows, BridgeRow("", string.format("Desktop app %s · protocol %d", Plain(bridge), ns.App.version)))
	table.insert(rows, Heading("Transport"))
	for _, line in ipairs(ns.Relay.DiagLines()) do
		table.insert(rows, Row("", Plain(line)))
	end
	return rows
end

local function ShowRow(line, row, offline)
	if not row then
		line:Hide()
		return
	end
	if row.heading then
		line.label:SetText("|cff" .. GOLD .. row.heading .. "|r")
		line.value:SetText("")
	else
		line.label:SetText(row.label)
		local grey = offline and row.bridge
		line.value:SetText(grey and "|cff" .. GREY .. row.value .. "|r" or row.value)
	end
	line:Show()
end

function DiagTab.Refresh()
	if not ui.page or not ui.page:IsShown() then
		return
	end
	local rows = Rows()
	ui.offset = math.max(0, math.min(ui.offset, #rows - LINES))
	local offline = not ns.Transport.Online()
	for i, line in ipairs(ui.lines) do
		ShowRow(line, rows[ui.offset + i], offline)
	end
end

function DiagTab.Build(page)
	ui.page = page
	ui.offset = 0
	ui.lines = {}
	for i = 1, LINES do
		local line = CreateFrame("Frame", "GnomishRelayDiagLine" .. i, page)
		line:SetSize(page:GetWidth() - 24, LINE_HEIGHT)
		line:SetPoint("TOPLEFT", page, "TOPLEFT", 16, -10 - (i - 1) * LINE_HEIGHT)
		line.label = line:CreateFontString(nil, "OVERLAY", "GameFontNormal")
		line.label:SetPoint("LEFT", line, "LEFT", 0, 0)
		line.label:SetJustifyH("LEFT")
		line.value = line:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
		line.value:SetPoint("LEFT", line, "LEFT", LABEL_WIDTH, 0)
		line.value:SetWidth(page:GetWidth() - LABEL_WIDTH - 30)
		line.value:SetJustifyH("LEFT")
		line.value:SetWordWrap(false)
		line:Hide()
		ui.lines[i] = line
	end
	page:EnableMouseWheel(true)
	page:SetScript("OnMouseWheel", function(_, delta)
		ui.offset = ui.offset - delta * 3
		DiagTab.Refresh()
	end)
end
