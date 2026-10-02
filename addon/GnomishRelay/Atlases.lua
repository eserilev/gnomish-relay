-- Atlases that some WoW clients lack (SPEC.md 7.9). SetAtlas with a missing name draws
-- nothing and raises no error, so each one has a stand-in that every client has.

local _, ns = ...

local Atlases = {}
ns.Atlases = Atlases

-- TBC Anniversary has neither of these.
local BELL = "minimap-genericevent-hornicon"
local BELL_SMALL = "minimap-genericevent-hornicon-small"
local BELL_STAND_IN = "communities-icon-notification"
local PARCHMENT = "QuestBG-Parchment"
local PARCHMENT_STAND_IN = "Interface\\QuestFrame\\QuestBG"

local function Has(atlas)
	return C_Texture.GetAtlasInfo(atlas) ~= nil
end

function Atlases.SetBell(texture)
	texture:SetAtlas(Has(BELL) and BELL or BELL_STAND_IN)
end

-- The bell inside a chat line.
function Atlases.BellText(size)
	local atlas = Has(BELL_SMALL) and BELL_SMALL or BELL_STAND_IN
	return string.format("|A:%s:%d:%d|a", atlas, size, size)
end

function Atlases.SetParchment(texture)
	if Has(PARCHMENT) then
		texture:SetAtlas(PARCHMENT)
	else
		texture:SetTexture(PARCHMENT_STAND_IN)
	end
end
