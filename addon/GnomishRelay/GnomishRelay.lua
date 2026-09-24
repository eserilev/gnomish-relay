-- Gnomish Relay. Step 5 of the build order (SPEC.md 15): load a slot and show its replies.

local SLOTS = 1000
local PROTO = 1

local function Say(msg)
	print("|cff66ccff[Relay]|r " .. msg)
end

-- WoW reads "|" as the start of an escape code, and "||" shows one "|".
local function Plain(text)
	return (tostring(text):gsub("|", "||"))
end

local function SlotName(n)
	return string.format("GnomishRelay_S%04d", n)
end

-- Each slot loads one time per UI session (SPEC.md 7.2, rule 3).
local function NextSlot()
	for n = 1, SLOTS do
		if not C_AddOns.IsAddOnLoaded(SlotName(n)) then
			return n
		end
	end
end

local function UsedSlots()
	local used = 0
	for n = 1, SLOTS do
		if C_AddOns.IsAddOnLoaded(SlotName(n)) then
			used = used + 1
		end
	end
	return used
end

local function ShowReplies(data)
	if #data.replies == 0 then
		Say("no replies yet")
	end
	for _, reply in ipairs(data.replies) do
		Say(string.format("[%s #%d, %s] %s", Plain(reply.chat), reply.id, reply.status, Plain(reply.text)))
	end
end

local function Poll()
	local n = NextSlot()
	if not n then
		Say("All slots are used. Type /reload to free them.")
		return
	end
	local name = SlotName(n)
	C_AddOns.EnableAddOn(name)
	GnomishRelay_SlotData = nil
	local loaded, reason = C_AddOns.LoadAddOn(name)
	local data = GnomishRelay_SlotData
	GnomishRelay_SlotData = nil
	if not loaded then
		Say(
			string.format(
				"Slot %d did not load (%s). Run `gnomish-relay install` with the game closed.",
				n,
				tostring(reason)
			)
		)
		return
	end
	if type(data) ~= "table" or data.proto ~= PROTO then
		Say("The bridge and the addon versions do not match.")
		return
	end
	ShowReplies(data)
end

SLASH_GNOMISHRELAY1 = "/relay"
SlashCmdList.GNOMISHRELAY = function(arg)
	arg = strtrim(arg or ""):lower()
	if arg == "poll" then
		Poll()
	elseif arg == "status" then
		local used = UsedSlots()
		Say(string.format("%d of %d slots used, next slot %d", used, SLOTS, used + 1))
	else
		Say("/relay poll | status")
	end
end
