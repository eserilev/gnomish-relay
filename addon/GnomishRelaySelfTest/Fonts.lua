-- The five assumptions of the Markdown window (SPEC.md 7.3.1 and 13.1): SimpleHTML
-- takes a font per text type, it measures its content, it shows |c codes, the body
-- font has the bullet and the no-break space, and SetFont reports a missing file.

local _, ns = ...

local Fonts = {}
ns.Fonts = Fonts

-- The same fonts as Transcript.lua of the relay.
local BODY_FONT = "Fonts\\ARIALN.TTF"
local HEADING_FONT = "Fonts\\FRIZQT__.TTF"
local RELAY_MONO = "Interface\\AddOns\\GnomishRelay\\JetBrainsMono-Regular.ttf"
local MISSING_FONT = "Interface\\AddOns\\GnomishRelaySelfTest\\NoSuchFont.ttf"
local TEXT_TYPES = { "h1", "h2", "h3", "p" }
local WIDTH = 120
local TWO_BLOCKS = "<html><body><p>one</p><p>two</p></body></html>"
local LONG = "a bold word and an italic word"
local PLAIN = "<html><body><p>" .. LONG .. " " .. LONG .. "</p></body></html>"
local COLORED = "<html><body><p>|cffffd100" .. LONG .. "|r |cffc0c8ff" .. LONG .. "|r</p></body></html>"

local host

local function Host()
	if not host then
		host = CreateFrame("Frame", nil, UIParent)
		host:SetSize(WIDTH, 400)
		host:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 0, 0)
		host:SetAlpha(0)
	end
	return host
end

local function NewHtml()
	local html = CreateFrame("SimpleHTML", nil, Host())
	html:SetPoint("TOPLEFT", Host(), "TOPLEFT", 0, 0)
	html:SetWidth(WIDTH)
	for _, textType in ipairs(TEXT_TYPES) do
		html:SetFont(textType, textType == "p" and BODY_FONT or HEADING_FONT, 14, "")
	end
	return html
end

local function HtmlSetFont()
	local html = CreateFrame("SimpleHTML", nil, Host())
	local out = {}
	for _, textType in ipairs(TEXT_TYPES) do
		out[textType] = ns.Json.Pack(pcall(html.SetFont, html, textType, BODY_FONT, 14, ""))
	end
	return out
end

local function SetFontReturns()
	local text = Host():CreateFontString(nil, "OVERLAY")
	return {
		present = ns.Json.Pack(pcall(text.SetFont, text, BODY_FONT, 14, "")),
		missing = ns.Json.Pack(pcall(text.SetFont, text, MISSING_FONT, 14, "")),
		relay_mono = ns.Json.Pack(pcall(text.SetFont, text, RELAY_MONO, 12, "")),
	}
end

local function Width(text, s)
	text:SetText(s)
	return text:GetUnboundedStringWidth()
end

-- A glyph that the font lacks draws as nothing or as a box, so its width tells.
local function Glyphs()
	local text = Host():CreateFontString(nil, "OVERLAY")
	text:SetFont(BODY_FONT, 14, "")
	local out = {
		x = Width(text, "x"),
		ten_letters = Width(text, "abcdefghij"),
		space = Width(text, " "),
		bullet = Width(text, "\226\128\162"),
		no_break_space = Width(text, "\194\160"),
		four_no_break_spaces_then_x = Width(text, ("\194\160"):rep(4) .. "x"),
		color_code = Width(text, "|cffffd100x|r"),
		doubled_pipe = Width(text, "||"),
	}
	text:SetText("x")
	out.line_height = text:GetStringHeight()
	return out
end

local function Height(html)
	-- A method call, not a method value, so the API gate sees the name.
	local ok, height = pcall(function()
		return html:GetContentHeight()
	end)
	return ok and height or tostring(height)
end

local function Heights(html, text)
	local ok, err = pcall(html.SetText, html, text)
	return { set_text_ok = ok, error = not ok and tostring(err) or nil, at_once = Height(html) }
end

-- Calls `done(results)`. The content height can come only after the next frame.
function Fonts.Measure(done)
	local blocks, plain, colored = NewHtml(), NewHtml(), NewHtml()
	local results = {
		simple_html_set_font = HtmlSetFont(),
		font_string_set_font = SetFontReturns(),
		glyphs = Glyphs(),
		two_blocks = Heights(blocks, TWO_BLOCKS),
		plain = Heights(plain, PLAIN),
		colored = Heights(colored, COLORED),
	}
	C_Timer.After(0, function()
		results.two_blocks.next_frame = Height(blocks)
		C_Timer.After(0.5, function()
			results.two_blocks.later = Height(blocks)
			results.plain.later = Height(plain)
			results.colored.later = Height(colored)
			done(results)
		end)
	end)
end
