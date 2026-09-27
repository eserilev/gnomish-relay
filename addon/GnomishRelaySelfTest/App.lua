-- The names of the self-test for the shared transport in addon/transport. The self-test
-- uses only the strip, the title, and the saved variables. It has no slots.

local _, ns = ...

ns.App = {
	title = "Gnomish Relay self-test",
	version = 1,
	helloChat = "selftest",
	slotPrefix = "GnomishRelaySelfTest_S%04d",
	slotData = "GnomishRelaySelfTest_SlotData",
	restore = "GnomishRelaySelfTest_Restore",
	live = "GnomishRelaySelfTest_Live",
	strip = "GnomishRelaySelfTestStrip",
	saved = "GnomishRelaySelfTestDB",
}
