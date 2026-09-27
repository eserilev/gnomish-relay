-- The self-test counts how often the game runs the file of a load-on-demand addon.

local name = ...

GnomishRelaySelfTestLoads = GnomishRelaySelfTestLoads or {}
GnomishRelaySelfTestLoads[name] = (GnomishRelaySelfTestLoads[name] or 0) + 1
