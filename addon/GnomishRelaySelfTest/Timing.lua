-- The clocks and the timers: GetTime against time(), the real delay of C_Timer.After,
-- the steps of a ticker, and the order of two timers that are due at the same time.

local _, ns = ...

local Timing = {}
ns.Timing = Timing

local DELAYS = { 0, 0.01, 0.1, 1 }
local TICKS = 10
local TICK = 0.1

function Timing.Ms()
	return math.floor(GetTimePreciseSec() * 1000)
end

-- One sample of every clock at one moment. Compare two samples for the drift.
function Timing.Clocks()
	return {
		get_time_ms = math.floor(GetTime() * 1000),
		precise_ms = Timing.Ms(),
		time = time(),
		server_time = GetServerTime(),
		framerate = GetFramerate(),
	}
end

local function AfterDelays(done)
	local out, index = {}, 1
	local function Next()
		local delay = DELAYS[index]
		if not delay then
			done(out)
			return
		end
		local from, fromFrame = Timing.Ms(), GetTime()
		C_Timer.After(delay, function()
			table.insert(out, {
				asked_ms = math.floor(delay * 1000),
				precise_ms = Timing.Ms() - from,
				get_time_ms = math.floor((GetTime() - fromFrame) * 1000),
			})
			index = index + 1
			Next()
		end)
	end
	Next()
end

local function Ticker(done)
	local steps, last, ticker = {}, Timing.Ms(), nil
	ticker = C_Timer.NewTicker(TICK, function()
		local now = Timing.Ms()
		table.insert(steps, now - last)
		last = now
		if #steps == TICKS then
			ticker:Cancel()
			done(steps)
		end
	end)
end

local function SameTime(done)
	local order = {}
	for _, name in ipairs({ "first", "second", "third" }) do
		C_Timer.After(0.2, function()
			table.insert(order, name)
			if #order == 3 then
				done(order)
			end
		end)
	end
end

-- Calls `done(results)` after about three seconds.
function Timing.Measure(done)
	local results = {}
	AfterDelays(function(after)
		results.after = after
		Ticker(function(steps)
			results.ticker_steps_ms = steps
			SameTime(function(order)
				results.same_time_order = order
				done(results)
			end)
		end)
	end)
end
