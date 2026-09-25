-- The saved variables of the app. WoW writes them at /reload and at logout, and the
-- bridge reads the outbox frames in them (SPEC.md 7.5).

-- Each global name comes from ns.App, so only _G can reach the global.
--# selene: allow(global_usage)

local _, ns = ...

function ns.Saved()
	local name = ns.App.saved
	_G[name] = _G[name] or {}
	return _G[name]
end
