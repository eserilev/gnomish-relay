-- The git actions of the player in the transcript (SPEC.md 9.10): the label of each one,
-- and what its answer changes in the chat.

local _, ns = ...

local Changes = {}
ns.Changes = Changes

-- The label of a git message in the transcript.
local LABELS = { merge = "Merge", discard = "Discard" }

function Changes.Label(entry)
	if not entry.git then
		return entry.text
	end
	return LABELS[entry.git] or "Git"
end

-- The bridge answered a git message. Only a done Discard changes the branch.
function Changes.Answered(chat, action, status)
	if action == "discard" and status == "done" then
		chat.branch = nil
	end
end
