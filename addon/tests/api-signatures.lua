-- The documented WoW Forever 1.60.1.70009 API that GnomishRelay uses.
-- Written by scripts/wow-api.sh from Blizzard_APIDocumentationGenerated. Do not edit.
-- A patch can change the arguments, returns, or secret flags and keep the name. The diff shows it.
-- The scan does not know the type of each object, so methods has each widget type with a called name.
return {
	build = "1.60.1.70009",
	functions = {
		["C_AddOns.EnableAddOn"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
				{ Name = "character", Type = "cstring", Nilable = false, Default = "0" },
			},
		},
		["C_AddOns.IsAddOnLoaded"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
			},
			Returns = {
				{ Name = "loadedOrLoading", Type = "bool", Nilable = false },
				{ Name = "loaded", Type = "bool", Nilable = false },
			},
		},
		["C_AddOns.LoadAddOn"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
			},
			Returns = {
				{ Name = "loaded", Type = "bool", Nilable = true },
				{ Name = "value", Type = "string", Nilable = true },
			},
		},
		["C_CVar.SetCVar"] = {
			RequiresNonReadOnlyCVar = true,
			RequiresNonSecureCVar = true,
			RequiresValidAndPublicCVar = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "name", Type = "cstring", Nilable = false },
				{ Name = "value", Type = "cstring", Nilable = true },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["C_Timer.After"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "seconds", Type = "number", Nilable = false },
				{ Name = "callback", Type = "TimerCallback", Nilable = false },
			},
		},
		["C_Timer.NewTicker"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "seconds", Type = "number", Nilable = false },
				{ Name = "callback", Type = "TickerCallback", Nilable = false },
				{ Name = "iterations", Type = "number", Nilable = true },
			},
			Returns = {
				{ Name = "cbObject", Type = "TickerCallback", Nilable = false },
			},
		},
		GetBuildInfo = {
			Returns = {
				{ Name = "buildVersion", Type = "cstring", Nilable = false },
				{ Name = "buildNumber", Type = "cstring", Nilable = false },
				{ Name = "buildDate", Type = "cstring", Nilable = false },
				{ Name = "interfaceVersion", Type = "number", Nilable = false },
				{ Name = "localizedVersion", Type = "cstring", Nilable = false },
				{ Name = "buildInfo", Type = "string", Nilable = false },
			},
		},
		GetPhysicalScreenSize = {
			Returns = {
				{ Name = "sizeX", Type = "number", Nilable = false },
				{ Name = "sizeY", Type = "number", Nilable = false },
			},
		},
		GetTime = {
			Returns = {
				{ Name = "time", Type = "number", Nilable = false },
			},
		},
		Screenshot = {},
	},
	methods = {
		["FrameAPIModelSceneFrameActorBase:Hide"] = {
			Arguments = {},
		},
		["FrameAPIModelSceneFrameActorBase:IsShown"] = {
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetScale"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetShown"] = {
			SecretArguments = "AllowedWhenUntainted",
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
		["FrameAPITooltip:SetText"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, ConditionalSecret = true },
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "alpha", Type = "number", Nilable = false, ConditionalSecret = true, Default = 1 },
				{ Name = "wrap", Type = "bool", Nilable = false, ConditionalSecret = true, Default = false },
			},
		},
		["SimpleAnimAPI:HookScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = false },
				{ Name = "bindingType", Type = "ScriptBindingType", Nilable = false, Default = "Extrinsic" },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleAnimAPI:SetScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = true },
			},
		},
		["SimpleAnimGroupAPI:HookScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = false },
				{ Name = "bindingType", Type = "ScriptBindingType", Nilable = false, Default = "Extrinsic" },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleAnimGroupAPI:SetScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = true },
			},
		},
		["SimpleAnimScaleAPI:SetScale"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scaleX", Type = "number", Nilable = false },
				{ Name = "scaleY", Type = "number", Nilable = false },
			},
		},
		["SimpleBrowserAPI:ClearFocus"] = {
			Arguments = {},
		},
		["SimpleButtonAPI:GetText"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Text },
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleButtonAPI:RegisterForClicks"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "buttons", Type = "ClickButton", Nilable = false, StrideIndex = 1 },
			},
		},
		["SimpleButtonAPI:SetHighlightTexture"] = {
			CheckAllowChangeParent = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
				{ Name = "blendMode", Type = "BlendMode", Nilable = true },
			},
		},
		["SimpleButtonAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
			},
		},
		["SimpleEditBoxAPI:ClearFocus"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptedInput } },
			Arguments = {},
		},
		["SimpleEditBoxAPI:GetText"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Text },
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetAutoFocus"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "autoFocus", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetJustifyH"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetMaxBytes"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "maxBytes", Type = "number", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetTextColor"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleFontAPI:SetFontObject"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleFontAPI:SetJustifyH"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleFontAPI:SetTextColor"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontStringAPI:GetStringHeight"] = {
			SecretWhenAnchoringSecret = true,
			Arguments = {},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleFontStringAPI:GetText"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Text },
			Arguments = {},
			Returns = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleFontStringAPI:GetUnboundedStringWidth"] = {
			SecretWhenAnchoringSecret = true,
			Arguments = {},
			Returns = {
				{ Name = "width", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetFont"] = {
			RequiresValidFontAsset = true,
			RequiresValidFontHeight = true,
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetJustifyH"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetNonSpaceWrap"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "wrap", Type = "bool", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetText"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
			},
		},
		["SimpleFontStringAPI:SetTextColor"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.VertexColor, Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleFontStringAPI:SetWordWrap"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "wrap", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:CreateFontString"] = {
			SecretArguments = "NotAllowed",
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
			SecretArguments = "NotAllowed",
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
		["SimpleFrameAPI:Hide"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleFrameAPI:IsShown"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Shown },
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:RegisterEvent"] = {
			AddsForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.EventRegistrations } },
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.EventRegistrations } },
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "eventName", Type = "cstring", Nilable = false },
			},
			Returns = {
				{ Name = "registered", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:RegisterForDrag"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "buttons", Type = "MouseButton", Nilable = false, StrideIndex = 1 },
			},
		},
		["SimpleFrameAPI:SetClampedToScreen"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "clampedToScreen", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetFrameLevel"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.FrameLevel },
			Arguments = {
				{ Name = "frameLevel", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetFrameStrata"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "strata", Type = "FrameStrata", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetIgnoreParentScale"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "ignore", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetMovable"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "movable", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetScale"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Scale },
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetShown"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Shown },
			Arguments = {
				{ Name = "shown", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleFrameAPI:Show"] = {
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetFontObject"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetJustifyH"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "textType", Type = "HTMLTextType", Nilable = false },
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleHTMLAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
				{ Name = "ignoreMarkup", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleHTMLAPI:SetTextColor"] = {
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "fontFile", Type = "cstring", Nilable = false },
				{ Name = "height", Type = "uiFontHeight", Nilable = false },
				{ Name = "flags", Type = "TBFFlags", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetFontObject"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "font", Type = "SimpleFont", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetJustifyH"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "justifyH", Type = "JustifyHorizontal", Nilable = false },
			},
		},
		["SimpleMessageFrameAPI:SetTextColor"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleRegionAPI:SetIgnoreParentScale"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "ignore", Type = "bool", Nilable = false },
			},
		},
		["SimpleRegionAPI:SetScale"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Scale },
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:EnableMouse"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "enable", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleScriptRegionAPI:EnableMouseWheel"] = {
			IsProtectedFunction = true,
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "enable", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleScriptRegionAPI:Hide"] = {
			Arguments = {},
		},
		["SimpleScriptRegionAPI:HookScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = false },
				{ Name = "bindingType", Type = "ScriptBindingType", Nilable = false, Default = "Extrinsic" },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:IsShown"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Shown },
			Arguments = {},
			Returns = {
				{ Name = "isShown", Type = "bool", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:SetScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = true },
			},
		},
		["SimpleScriptRegionAPI:SetShown"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Shown },
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
		["SimpleScriptRegionResizingAPI:SetAllPoints"] = {
			CheckAllowInheritForbiddenLayoutAspects = true,
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "relativeTo", Type = "ScriptRegion", Nilable = false },
				{ Name = "doResize", Type = "bool", Nilable = false, Default = true },
			},
		},
		["SimpleScriptRegionResizingAPI:SetHeight"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetPoint"] = {
			CheckAllowInheritForbiddenLayoutAspects = true,
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "x", Type = "uiUnit", Nilable = false },
				{ Name = "y", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionResizingAPI:SetWidth"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "width", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:GetVerticalScroll"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.ScrollOffset },
			Arguments = {},
			Returns = {
				{ Name = "offset", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:SetScrollChild"] = {
			CheckAllowChangeParent = true,
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scrollChild", Type = "SimpleFrame", Nilable = false },
			},
		},
		["SimpleScrollFrameAPI:SetVerticalScroll"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.ScrollOffset },
			Arguments = {
				{ Name = "offset", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetMinMaxValues"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "minValue", Type = "number", Nilable = false },
				{ Name = "maxValue", Type = "number", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetValue"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "value", Type = "number", Nilable = false },
				{ Name = "treatAsMouseEvent", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleStatusBarAPI:SetMinMaxValues"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.BarValue },
			Arguments = {
				{ Name = "minValue", Type = "number", Nilable = false },
				{ Name = "maxValue", Type = "number", Nilable = false },
				{ Name = "interpolation", Type = "StatusBarInterpolation", Nilable = false, NeverSecret = true, Default = "Immediate" },
			},
		},
		["SimpleStatusBarAPI:SetStatusBarColor"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.VertexColor, Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleStatusBarAPI:SetStatusBarTexture"] = {
			CheckAllowChangeParent = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
			Returns = {
				{ Name = "success", Type = "bool", Nilable = false },
			},
		},
		["SimpleStatusBarAPI:SetValue"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.BarValue },
			Arguments = {
				{ Name = "value", Type = "number", Nilable = false },
				{ Name = "interpolation", Type = "StatusBarInterpolation", Nilable = false, NeverSecret = true, Default = "Immediate" },
			},
		},
		["SimpleTextureBaseAPI:SetColorTexture"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.SetTexture } },
			SecretArguments = "AllowedWhenTainted",
			Arguments = {
				{ Name = "colorR", Type = "number", Nilable = false },
				{ Name = "colorG", Type = "number", Nilable = false },
				{ Name = "colorB", Type = "number", Nilable = false },
				{ Name = "a", Type = "SingleColorValue", Nilable = true },
			},
		},
		["SimpleTextureBaseAPI:SetTexture"] = {
			SecretArguments = "AllowedWhenTainted",
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
		PLAYER_LOGIN = {
			SynchronousEvent = true,
		},
		SCREENSHOT_FAILED = {
			SynchronousEvent = true,
		},
		SCREENSHOT_SUCCEEDED = {
			SynchronousEvent = true,
		},
	},
	undocumented = {
		"CreateFrame",
		"InCombatLockdown",
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
