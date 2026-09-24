-- Spike: how long does the game stall when it loads a big slot body?
-- The answer sets the size limit of a slot body (SPEC.md 7.3, S12).
-- Each slot loads one time per UI session, so /reload before a second run.

local SIZES = { "100K", "1M", "5M" }

local function Say(msg)
	print("|cff66ccff[GR size]|r " .. msg)
end

local function Measure(size)
	local name = "GRSpikeBody" .. size
	C_AddOns.EnableAddOn(name)
	collectgarbage("collect")
	local memBefore = collectgarbage("count")
	local start = debugprofilestop()
	local loaded, reason = C_AddOns.LoadAddOn(name)
	local ms = debugprofilestop() - start
	local kb = collectgarbage("count") - memBefore
	if not loaded then
		return { size = size, error = tostring(reason) }
	end
	local replies = GRSpikeBodyData and GRSpikeBodyData.replies and #GRSpikeBodyData.replies or 0
	GRSpikeBodyData = nil
	return { size = size, ms = ms, memKb = kb, replies = replies }
end

SLASH_GRSIZE1 = "/grsize"
SlashCmdList.GRSIZE = function()
	GRSpikeSizeResults = GRSpikeSizeResults or {}
	for _, size in ipairs(SIZES) do
		local r = Measure(size)
		r.time = date("%Y-%m-%d %H:%M:%S")
		table.insert(GRSpikeSizeResults, r)
		if r.error then
			Say(string.format("%s: load failed (%s). Did you /reload first?", size, r.error))
		else
			Say(string.format("%s: %.1f ms, %d KB of Lua memory, %d replies", size, r.ms, r.memKb, r.replies))
		end
	end
end
