-- The names of the relay app for the shared transport in addon/transport
-- (SPEC.md 9.7, decision 5). Each app has its own, so two apps never share a global.

local _, ns = ...

ns.App = {
	title = "Gnomish Relay",
	-- The bridge checks it against the range of the relay (SPEC.md 7.7).
	version = 1,
	-- The bridge reads no chat of a hello.
	helloChat = "relay",
	slotPrefix = "GnomishRelay_S%04d",
	slotData = "GnomishRelay_SlotData",
	restore = "GnomishRelay_Restore",
	live = "GnomishRelay_Live",
	strip = "GnomishRelayStrip",
	saved = "GnomishRelayDB",
}
