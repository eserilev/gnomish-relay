-- Spike: do the client file-load rules (SPEC.md 7.2) hold under Wine?
-- Run the phases in order: /grfiles before, then spikes/mutate.sh, then
-- /grfiles after, then /reload, then /grfiles reload.

local SND = "Interface\\AddOns\\GRSpikeFiles\\snd\\"

local function Say(msg)
	print("|cff66ccff[GR files]|r " .. msg)
end

local function Plays(file)
	local ok, willPlay, handle = pcall(PlaySoundFile, SND .. file, "Master")
	if not ok then return "error: " .. tostring(willPlay) end
	if willPlay and handle then StopSound(handle) end
	return willPlay and true or false
end

local function Load(name)
	C_AddOns.EnableAddOn(name)
	local loaded, reason = C_AddOns.LoadAddOn(name)
	if loaded then return true end
	return tostring(reason)
end

local function SlotData(key)
	return GRSpikeSlotData and GRSpikeSlotData[key]
end

local phases = {
	before = function()
		return {
			{ "rule 4: empty wav does not play", false, Plays("empty.wav") },
			{ "rule 4: valid wav plays", true, Plays("valid.wav") },
			{ "setup: flip_on (empty) does not play yet", false, Plays("flip_on.wav") },
			{ "setup: flip_off (valid) plays", true, Plays("flip_off.wav") },
			{ "setup: late.wav does not exist yet", false, Plays("late.wav") },
		}
	end,
	after = function()
		local results = {
			{ "signal: flip_on plays after it became valid", true, Plays("flip_on.wav") },
			{ "rule 5: flip_off still plays after it became empty", true, Plays("flip_off.wav") },
			{ "rule 1: late.wav (made after launch) is not found", false, Plays("late.wav") },
			{ "rule 1: addon made after launch is missing", "MISSING", Load("GRSpikeLate") },
			{ "rule 2: slot load succeeds", true, Load("GRSpikeSlot1") },
		}
		table.insert(results, { "rule 2: slot reads the file from disk at load", "mutated", SlotData("slot1") })
		Load("GRSpikeSlot2")
		Load("GRSpikeSlot2")
		table.insert(results, { "rule 3: second load of a slot does not run it again", 1, SlotData("slot2count") })
		return results
	end,
	reload = function()
		return {
			{ "rule 3: after /reload the slot is not loaded", false, C_AddOns.IsAddOnLoaded("GRSpikeSlot2") },
			{ "rule 3: after /reload the slot loads again", true, Load("GRSpikeSlot2") },
			{ "rule 5: flip_off still plays after /reload", true, Plays("flip_off.wav") },
		}
	end,
}

local function Run(phase)
	local results = phases[phase]()
	GRSpikeFilesResults = GRSpikeFilesResults or {}
	local failed = 0
	for _, r in ipairs(results) do
		local name, expected, actual = r[1], r[2], r[3]
		local pass = actual == expected
		if not pass then failed = failed + 1 end
		Say(string.format("%s %s (expected %s, got %s)",
			pass and "|cff00ff00PASS|r" or "|cffff4040FAIL|r", name, tostring(expected), tostring(actual)))
		table.insert(GRSpikeFilesResults, {
			phase = phase, name = name, pass = pass,
			expected = tostring(expected), actual = tostring(actual),
			time = date("%Y-%m-%d %H:%M:%S"),
		})
	end
	Say(string.format("%s: %d of %d passed.", phase, #results - failed, #results))
end

SLASH_GRFILES1 = "/grfiles"
SlashCmdList.GRFILES = function(arg)
	arg = strtrim(arg or ""):lower()
	if phases[arg] then
		Run(arg)
	else
		Say("/grfiles before | after | reload")
	end
end
