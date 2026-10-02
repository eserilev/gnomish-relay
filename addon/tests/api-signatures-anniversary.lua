-- The documented WoW Classic: TBC Anniversary 2.5.6.69795 API that GnomishRelay uses.
-- Written by scripts/wow-api.sh from Blizzard_APIDocumentationGenerated. Do not edit.
-- A patch can change the arguments, returns, or secret flags and keep the name. The diff shows it.
-- The scan does not know the type of each object, so methods has each widget type with a called name.
return {
	build = "2.5.6.69795",
	functions = {
		["C_AddOns.DisableAddOn"] = {
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
				{ Name = "character", Type = "cstring", Nilable = false, Default = "0" },
			},
		},
		["C_AddOns.EnableAddOn"] = {
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
				{ Name = "character", Type = "cstring", Nilable = false, Default = "0" },
			},
		},
		["C_AddOns.IsAddOnLoaded"] = {
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
			},
			Returns = {
				{ Name = "loadedOrLoading", Type = "bool", Nilable = false },
				{ Name = "loaded", Type = "bool", Nilable = false },
			},
		},
		["C_AddOns.LoadAddOn"] = {
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
			},
			Returns = {
				{ Name = "loaded", Type = "bool", Nilable = true },
				{ Name = "value", Type = "string", Nilable = true },
			},
		},
		["C_CVar.SetCVar"] = {
			IsNotReadOnly = true,
			IsNotSecure = true,
			IsValidAndPublic = true,
			Arguments = {
				{ Name = "name", Type = "cstring", Nilable = false },
				{ Name = "value", Type = "cstring", Nilable = true },
				{ Name = "scriptCVar", Type = "cstring", Nilable = true },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["C_Texture.GetAtlasInfo"] = {
			MayReturnNothing = true,
			Arguments = {
				{ Name = "atlas", Type = "textureAtlas", Nilable = false },
			},
			Returns = {
				{ Name = "info", Type = "AtlasInfo", Nilable = false },
			},
		},
		["C_Timer.After"] = {
			Arguments = {
				{ Name = "seconds", Type = "number", Nilable = false },
				{ Name = "callback", Type = "TimerCallback", Nilable = false },
			},
		},
		["C_Timer.NewTicker"] = {
			Arguments = {
				{ Name = "seconds", Type = "number", Nilable = false },
				{ Name = "callback", Type = "TickerCallback", Nilable = false },
				{ Name = "iterations", Type = "number", Nilable = true },
			},
			Returns = {
				{ Name = "cbObject", Type = "TickerCallback", Nilable = false },
			},
		},
		GetCursorPosition = {
			Returns = {
				{ Name = "posX", Type = "number", Nilable = false },
				{ Name = "posY", Type = "number", Nilable = false },
			},
		},
		Screenshot = {},
	},
	methods = {
		["DurationTextBindingObjectAPI:SetEnabled"] = {
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false },
			},
		},
		["FrameAPICharacterModelBase:SetRotation"] = {
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
				{ Name = "animate", Type = "bool", Nilable = false, Default = true },
			},
		},
		["FrameAPICooldown:SetRotation"] = {
			Arguments = {
				{ Name = "rotationRadians", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:Hide"] = {
			Arguments = {},
		},
		["FrameAPIModelSceneFrameActorBase:IsShown"] = {
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetAlpha"] = {
			Arguments = {
				{ Name = "alpha", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetScale"] = {
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetShown"] = {
			Arguments = {
				{ Name = "show", Type = "bool", Nilable = false, Default = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:Show"] = {
			Arguments = {},
		},
		["FrameAPISimpleCheckout:ClearFocus"] = {
			Arguments = {},
		},
		["FrameAPISimpleCheckout:SetFocus"] = {
			Arguments = {},
		},
		["FrameAPITooltip:SetText"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "alpha", Type = "number", Nilable = false, Default = 1 },
				{ Name = "wrap", Type = "bool", Nilable = false, Default = false },
			},
		},
		["LuaColorCurveObjectAPI:GetPoint"] = {
			Arguments = {
				{ Name = "index", Type = "luaIndex", Nilable = false },
			},
			Returns = {
				{ Name = "point", Type = "LuaColorCurvePoint", Nilable = true },
			},
		},
		["LuaCurveObjectAPI:GetPoint"] = {
			Arguments = {
				{ Name = "index", Type = "luaIndex", Nilable = false },
			},
			Returns = {
				{ Name = "point", Type = "vector2", Mixin = "Vector2DMixin", Nilable = true },
			},
		},
		["SimpleAnimAPI:HookScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = false },
				{ Name = "bindingType", Type = "number", Nilable = true },
			},
		},
		["SimpleAnimAPI:SetScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = true },
			},
		},
		["SimpleAnimGroupAPI:HookScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = false },
				{ Name = "bindingType", Type = "number", Nilable = true },
			},
		},
		["SimpleAnimGroupAPI:SetScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = true },
			},
		},
		["SimpleAnimScaleAPI:SetScale"] = {
			Arguments = {
				{ Name = "scaleX", Type = "number", Nilable = false },
				{ Name = "scaleY", Type = "number", Nilable = false },
			},
		},
		["SimpleBrowserAPI:ClearFocus"] = {
			Arguments = {},
		},
		["SimpleBrowserAPI:SetFocus"] = {
			Arguments = {},
		},
		["SimpleButtonAPI:GetText"] = {
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleButtonAPI:RegisterForClicks"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "buttons", Type = "ClickButton", Nilable = false, StrideIndex = 1 },
			},
		},
		["SimpleButtonAPI:SetEnabled"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleButtonAPI:SetHighlightTexture"] = {
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
				{ Name = "blendMode", Type = "BlendMode", Nilable = true },
			},
		},
		["SimpleButtonAPI:SetNormalTexture"] = {
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
		},
		["SimpleButtonAPI:SetPushedTexture"] = {
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
		},
		["SimpleButtonAPI:SetText"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
			},
		},
		["SimpleCheckboxAPI:GetChecked"] = {
			Arguments = {},
			Returns = {
				{ Name = "checked", Type = "bool", Nilable = false },
			},
		},
		["SimpleCheckboxAPI:SetChecked"] = {
			Arguments = {
				{ Name = "checked", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:ClearFocus"] = {
			Arguments = {},
		},
		["SimpleEditBoxAPI:GetText"] = {
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:HasFocus"] = {
			Arguments = {},
			Returns = {
				{ Name = "hasFocus", Type = "bool", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:HighlightText"] = {
			Arguments = {
				{ Name = "start", Type = "number", Nilable = false, Default = 0 },
				{ Name = "stop", Type = "number", Nilable = false, Default = -1 },
			},
		},
		["SimpleEditBoxAPI:SetAltArrowKeyMode"] = {
			Arguments = {
				{ Name = "altMode", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetAutoFocus"] = {
			Arguments = {
				{ Name = "autoFocus", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetCursorPosition"] = {
			Arguments = {
				{ Name = "cursorPosition", Type = "number", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetEnabled"] = {
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetFocus"] = {
			Arguments = {},
		},
		["SimpleEditBoxAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			Arguments = {
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetFontObject"] = {
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetJustifyH"] = {
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetMaxBytes"] = {
			Arguments = {
				{ Name = "maxBytes", Type = "number", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetText"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetTextColor"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontAPI:SetAlpha"] = {
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
			},
		},
		["SimpleFontAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			Arguments = {
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleFontAPI:SetFontObject"] = {
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleFontAPI:SetJustifyH"] = {
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleFontAPI:SetTextColor"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontStringAPI:GetStringHeight"] = {
			Arguments = {},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleFontStringAPI:GetText"] = {
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleFontStringAPI:GetUnboundedStringWidth"] = {
			Arguments = {},
			Returns = {
				{ Name = "width", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			Arguments = {
				{ Name = "fontFile", Type = "FontAsset", Nilable = false },
				{ Name = "fontHeight", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = true },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetFontObject"] = {
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetJustifyH"] = {
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetMaxLines"] = {
			Arguments = {
				{ Name = "maxLines", Type = "number", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetNonSpaceWrap"] = {
			Arguments = {
				{ Name = "wrap", Type = "bool", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetRotation"] = {
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetText"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
			},
		},
		["SimpleFontStringAPI:SetTextColor"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontStringAPI:SetWordWrap"] = {
			Arguments = {
				{ Name = "wrap", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:CreateFontString"] = {
			Arguments = {
				{ Name = "name", Type = "cstring", Nilable = true },
				{ Name = "drawLayer", Type = "DrawLayer", Nilable = true },
				{ Name = "templateName", Type = "cstring", Nilable = true },
			},
			Returns = {
				{ Name = "line", Type = "SimpleFontString", Nilable = false },
			},
		},
		["SimpleFrameAPI:CreateTexture"] = {
			Arguments = {
				{ Name = "name", Type = "cstring", Nilable = true },
				{ Name = "drawLayer", Type = "DrawLayer", Nilable = true },
				{ Name = "templateName", Type = "cstring", Nilable = true },
				{ Name = "subLevel", Type = "number", Nilable = true },
			},
			Returns = {
				{ Name = "texture", Type = "SimpleTexture", Nilable = false },
			},
		},
		["SimpleFrameAPI:GetEffectiveScale"] = {
			Arguments = {},
			Returns = {
				{ Name = "effectiveScale", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:Hide"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleFrameAPI:IsShown"] = {
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:Raise"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleFrameAPI:RegisterEvent"] = {
			Arguments = {
				{ Name = "eventName", Type = "cstring", Nilable = false },
			},
			Returns = {
				{ Name = "registered", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:RegisterForDrag"] = {
			Arguments = {
				{ Name = "buttons", Type = "MouseButton", Nilable = false, StrideIndex = 1 },
			},
		},
		["SimpleFrameAPI:SetAlpha"] = {
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetClampRectInsets"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "left", Type = "uiUnit", Nilable = false },
				{ Name = "right", Type = "uiUnit", Nilable = false },
				{ Name = "top", Type = "uiUnit", Nilable = false },
				{ Name = "bottom", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetClampedToScreen"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "clampedToScreen", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetFrameLevel"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "frameLevel", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetFrameStrata"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "strata", Type = "FrameStrata", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetIgnoreParentScale"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "ignore", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetMovable"] = {
			Arguments = {
				{ Name = "movable", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetResizable"] = {
			Arguments = {
				{ Name = "resizable", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetResizeBounds"] = {
			Arguments = {
				{ Name = "minWidth", Type = "uiUnit", Nilable = false },
				{ Name = "minHeight", Type = "uiUnit", Nilable = false },
				{ Name = "maxWidth", Type = "uiUnit", Nilable = true },
				{ Name = "maxHeight", Type = "uiUnit", Nilable = true },
			},
		},
		["SimpleFrameAPI:SetScale"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetShown"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "shown", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleFrameAPI:SetToplevel"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "topLevel", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetUserPlaced"] = {
			Arguments = {
				{ Name = "userPlaced", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:Show"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleFrameAPI:StartSizing"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "resizePoint", Type = "FramePoint", Nilable = true },
				{ Name = "alwaysStartFromMouse", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleFrameAPI:StopMovingOrSizing"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleHTMLAPI:GetContentHeight"] = {
			Arguments = {},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetFontObject"] = {
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetJustifyH"] = {
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetText"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
				{ Name = "ignoreMarkup", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleHTMLAPI:SetTextColor"] = {
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleLineAPI:ClearAllPoints"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleMessageFrameAPI:AddMessage"] = {
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
				{ Name = "messageID", Type = "number", Nilable = true },
			},
		},
		["SimpleMessageFrameAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			Arguments = {
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetFontObject"] = {
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetJustifyH"] = {
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetTextColor"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleRegionAPI:GetEffectiveScale"] = {
			Arguments = {},
			Returns = {
				{ Name = "effectiveScale", Type = "number", Nilable = false },
			},
		},
		["SimpleRegionAPI:SetAlpha"] = {
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
			},
		},
		["SimpleRegionAPI:SetIgnoreParentScale"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "ignore", Type = "bool", Nilable = false },
			},
		},
		["SimpleRegionAPI:SetScale"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:EnableMouse"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "enable", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleScriptRegionAPI:EnableMouseWheel"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "enable", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleScriptRegionAPI:GetCenter"] = {
			MayReturnNothing = true,
			Arguments = {},
			Returns = {
				{ Name = "x", Type = "uiUnit", Nilable = false },
				{ Name = "y", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:GetHeight"] = {
			Arguments = {
				{ Name = "ignoreRect", Type = "bool", Nilable = false, Default = false },
			},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:GetWidth"] = {
			Arguments = {
				{ Name = "ignoreRect", Type = "bool", Nilable = false, Default = false },
			},
			Returns = {
				{ Name = "width", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:Hide"] = {
			Arguments = {},
		},
		["SimpleScriptRegionAPI:HookScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = false },
				{ Name = "bindingType", Type = "number", Nilable = true },
			},
		},
		["SimpleScriptRegionAPI:IsMouseOver"] = {
			Arguments = {
				{ Name = "offsetTop", Type = "uiUnit", Nilable = false, Default = 0 },
				{ Name = "offsetBottom", Type = "uiUnit", Nilable = false, Default = 0 },
				{ Name = "offsetLeft", Type = "uiUnit", Nilable = false, Default = 0 },
				{ Name = "offsetRight", Type = "uiUnit", Nilable = false, Default = 0 },
			},
			Returns = {
				{ Name = "isMouseOver", Type = "bool", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:IsShown"] = {
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:SetScript"] = {
			Arguments = {
				{ Name = "scriptTypeName", Type = "cstring", Nilable = false },
				{ Name = "script", Type = "luaFunction", Nilable = true },
			},
		},
		["SimpleScriptRegionAPI:SetShown"] = {
			Arguments = {
				{ Name = "show", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleScriptRegionAPI:Show"] = {
			Arguments = {},
		},
		["SimpleScriptRegionResizingAPI:ClearAllPoints"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleScriptRegionResizingAPI:GetPoint"] = {
			MayReturnNothing = true,
			Arguments = {
				{ Name = "anchorIndex", Type = "luaIndex", Nilable = false, Default = 0 },
				{ Name = "resolveCollapsed", Type = "bool", Nilable = false, Default = false },
			},
			Returns = {
				{ Name = "point", Type = "FramePoint", Nilable = false },
				{ Name = "relativeTo", Type = "ScriptRegion", Nilable = false },
				{ Name = "relativePoint", Type = "FramePoint", Nilable = false },
				{ Name = "offsetX", Type = "uiUnit", Nilable = false },
				{ Name = "offsetY", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetAllPoints"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "relativeTo", Type = "ScriptRegion", Nilable = false },
				{ Name = "doResize", Type = "bool", Nilable = false, Default = true },
			},
		},
		["SimpleScriptRegionResizingAPI:SetHeight"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetPoint"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "point", Type = "FramePoint", Nilable = false },
				{ Name = "relativeTo", Type = "ScriptRegion", Nilable = false },
				{ Name = "relativePoint", Type = "FramePoint", Nilable = false },
				{ Name = "offsetX", Type = "uiUnit", Nilable = false },
				{ Name = "offsetY", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetSize"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "x", Type = "uiUnit", Nilable = false },
				{ Name = "y", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetWidth"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "width", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:GetVerticalScroll"] = {
			Arguments = {},
			Returns = {
				{ Name = "offset", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:SetScrollChild"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "scrollChild", Type = "SimpleFrame", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:SetVerticalScroll"] = {
			IsProtectedFunction = true,
			Arguments = {
				{ Name = "offset", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetEnabled"] = {
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetMinMaxValues"] = {
			Arguments = {
				{ Name = "minValue", Type = "number", Nilable = false },
				{ Name = "maxValue", Type = "number", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetObeyStepOnDrag"] = {
			Arguments = {
				{ Name = "obeyStepOnDrag", Type = "bool", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetValue"] = {
			Arguments = {
				{ Name = "value", Type = "number", Nilable = false },
				{ Name = "treatAsMouseEvent", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleSliderAPI:SetValueStep"] = {
			Arguments = {
				{ Name = "valueStep", Type = "number", Nilable = false },
			},
		},
		["SimpleStatusBarAPI:SetMinMaxValues"] = {
			Arguments = {
				{ Name = "minValue", Type = "number", Nilable = false },
				{ Name = "maxValue", Type = "number", Nilable = false },
				{ Name = "interpolation", Type = "StatusBarInterpolation", Nilable = false, Default = "Immediate" },
			},
		},
		["SimpleStatusBarAPI:SetStatusBarColor"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleStatusBarAPI:SetStatusBarTexture"] = {
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleStatusBarAPI:SetValue"] = {
			Arguments = {
				{ Name = "value", Type = "number", Nilable = false },
				{ Name = "interpolation", Type = "StatusBarInterpolation", Nilable = false, Default = "Immediate" },
			},
		},
		["SimpleTextureBaseAPI:SetAtlas"] = {
			Arguments = {
				{ Name = "atlas", Type = "textureAtlas", Nilable = false },
				{ Name = "useAtlasSize", Type = "bool", Nilable = false, Default = false },
				{ Name = "filterMode", Type = "FilterMode", Nilable = true },
				{ Name = "resetTexCoords", Type = "bool", Nilable = true },
				{ Name = "wrapModeHorizontal", Type = "cstring", Nilable = true },
				{ Name = "wrapModeVertical", Type = "cstring", Nilable = true },
			},
		},
		["SimpleTextureBaseAPI:SetBlendMode"] = {
			Arguments = {
				{ Name = "blendMode", Type = "BlendMode", Nilable = false },
			},
		},
		["SimpleTextureBaseAPI:SetColorTexture"] = {
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleTextureBaseAPI:SetRotation"] = {
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
				{ Name = "normalizedRotationPoint", Type = "vector2", Mixin = "Vector2DMixin", Nilable = true },
			},
		},
		["SimpleTextureBaseAPI:SetSnapToPixelGrid"] = {
			Arguments = {
				{ Name = "snap", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleTextureBaseAPI:SetTexture"] = {
			Arguments = {
				{ Name = "textureAsset", Type = "cstring", Nilable = true },
				{ Name = "wrapModeHorizontal", Type = "cstring", Nilable = true },
				{ Name = "wrapModeVertical", Type = "cstring", Nilable = true },
				{ Name = "filterMode", Type = "cstring", Nilable = true },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
	},
	events = {
		ADDON_LOADED = {
			SynchronousEvent = true,
			Payload = {
				{ Name = "addOnName", Type = "cstring", Nilable = false },
				{ Name = "containsBindings", Type = "bool", Nilable = false },
			},
		},
		DISPLAY_SIZE_CHANGED = {
			SynchronousEvent = true,
		},
		GLOBAL_MOUSE_DOWN = {
			SynchronousEvent = true,
			Payload = {
				{ Name = "button", Type = "cstring", Nilable = false },
			},
		},
		PLAYER_LOGIN = {
			SynchronousEvent = true,
		},
		PLAYER_REGEN_ENABLED = {
			SynchronousEvent = true,
		},
		SCREENSHOT_FAILED = {
			SynchronousEvent = true,
		},
		SCREENSHOT_SUCCEEDED = {
			SynchronousEvent = true,
		},
		UI_SCALE_CHANGED = {
			SynchronousEvent = true,
		},
	},
	undocumented = {
		"CreateFrame",
		"GetBuildInfo",
		"GetPhysicalScreenSize",
		"GetTime",
		"InCombatLockdown",
		"IsMacClient",
		"IsShiftKeyDown",
		"PlaySound",
		"bit.band",
		"bit.bnot",
		"bit.bor",
		"bit.bxor",
		"bit.lshift",
		"bit.rshift",
		"hooksecurefunc",
		"strtrim",
		"time",
	},
}
