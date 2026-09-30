-- The documented WoW Forever 1.60.1.70124 API that GnomishRelaySelfTest, GnomishRelaySelfTest_Off, GnomishRelaySelfTest_Slot uses.
-- Written by scripts/wow-api.sh from Blizzard_APIDocumentationGenerated. Do not edit.
-- A patch can change the arguments, returns, or secret flags and keep the name. The diff shows it.
-- The scan does not know the type of each object, so methods has each widget type with a called name.
return {
	build = "1.60.1.70124",
	functions = {
		["C_AddOns.DisableAddOn"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
				{ Name = "character", Type = "cstring", Nilable = false, Default = "0" },
			},
		},
		["C_AddOns.EnableAddOn"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
				{ Name = "character", Type = "cstring", Nilable = false, Default = "0" },
			},
		},
		["C_AddOns.GetAddOnInfo"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "name", Type = "uiAddon", Nilable = false },
			},
			Returns = {
				{ Name = "name", Type = "cstring", Nilable = false },
				{ Name = "title", Type = "cstring", Nilable = false },
				{ Name = "notes", Type = "cstring", Nilable = false },
				{ Name = "loadable", Type = "bool", Nilable = false },
				{ Name = "reason", Type = "cstring", Nilable = false },
				{ Name = "security", Type = "cstring", Nilable = false },
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
		["C_CVar.GetCVar"] = {
			SecretArguments = "NotAllowed",
			Arguments = {
				{ Name = "name", Type = "cstring", Nilable = false },
			},
			Returns = {
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
		["C_CombatLog.IsCombatLogRestricted"] = {
			Returns = {
				{ Name = "restricted", Type = "bool", Nilable = false },
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
		GetFramerate = {
			Returns = {
				{ Name = "framerate", Type = "number", Nilable = false },
			},
		},
		GetScreenHeight = {
			Returns = {
				{ Name = "height", Type = "number", Nilable = false },
			},
		},
		GetScreenWidth = {
			Returns = {
				{ Name = "width", Type = "number", Nilable = false },
			},
		},
		GetServerTime = {
			Returns = {
				{ Name = "time", Type = "number", Nilable = false },
			},
		},
		GetTimePreciseSec = {
			Returns = {
				{ Name = "time", Type = "number", Nilable = false },
			},
		},
		UnitDetailedThreatSituation = {
			MayReturnNothing = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenUnitThreatValuesRestricted = true,
			Arguments = {
				{ Name = "unit", Type = "UnitToken", Nilable = false },
				{ Name = "mobGUID", Type = "UnitToken", Nilable = false },
			},
			Returns = {
				{ Name = "isTanking", Type = "bool", Nilable = false },
				{ Name = "status", Type = "number", Nilable = false },
				{ Name = "scaledPercentage", Type = "number", Nilable = false },
				{ Name = "rawPercentage", Type = "number", Nilable = false },
				{ Name = "rawThreat", Type = "number", Nilable = false },
			},
		},
		UnitExists = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "unit", Type = "UnitToken", Nilable = true },
			},
			Returns = {
				{ Name = "result", Type = "bool", Nilable = false },
			},
		},
		UnitGroupRolesAssigned = {
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenUnitIdentityRestricted = true,
			Arguments = {
				{ Name = "unit", Type = "UnitToken", Nilable = true },
			},
			Returns = {
				{ Name = "result", Type = "cstring", Nilable = false },
			},
		},
		UnitHealth = {
			SecretArguments = "AllowedWhenUntainted",
			SecretReturns = true,
			Arguments = {
				{ Name = "unit", Type = "UnitTokenPvPRestrictedForAddOns", Nilable = false },
				{ Name = "usePredicted", Type = "bool", Nilable = false, Default = true },
			},
			Returns = {
				{ Name = "result", Type = "number", Nilable = false },
			},
		},
		UnitName = {
			SecretArguments = "AllowedWhenTainted",
			SecretWhenUnitNameIdentityRestricted = true,
			Arguments = {
				{ Name = "unit", Type = "UnitToken", Nilable = false },
			},
			Returns = {
				{ Name = "unitName", Type = "cstring", Nilable = false },
				{ Name = "unitServer", Type = "cstring", Nilable = false },
			},
		},
		UnitPower = {
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenUnitPowerRestricted = true,
			Arguments = {
				{ Name = "unitToken", Type = "UnitTokenPvPRestrictedForAddOns", Nilable = false },
				{ Name = "powerType", Type = "PowerType", Nilable = true },
				{ Name = "unmodified", Type = "bool", Nilable = false, Default = false },
			},
			Returns = {
				{ Name = "power", Type = "number", Nilable = false },
			},
		},
		issecretvalue = {
			SecretArguments = "AllowedWhenUntainted",
			SecureHooksAllowed = false,
			Arguments = {
				{ Name = "value", Type = "LuaValueReference", Nilable = false },
			},
			Returns = {
				{ Name = "isSecret", Type = "bool", Nilable = false },
			},
		},
	},
	methods = {
		["FrameAPIModelSceneFrameActorBase:GetScale"] = {
			Arguments = {},
			Returns = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:Hide"] = {
			Arguments = {},
		},
		["FrameAPIModelSceneFrameActorBase:SetAlpha"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "alpha", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:SetScale"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["FrameAPIModelSceneFrameActorBase:Show"] = {
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
		["SimpleAnimScaleAPI:GetScale"] = {
			Arguments = {},
			Returns = {
				{ Name = "scaleX", Type = "number", Nilable = false },
				{ Name = "scaleY", Type = "number", Nilable = false },
			},
		},
		["SimpleAnimScaleAPI:SetScale"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scaleX", Type = "number", Nilable = false },
				{ Name = "scaleY", Type = "number", Nilable = false },
			},
		},
		["SimpleButtonAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
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
		["SimpleEditBoxAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		["SimpleFontAPI:SetAlpha"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
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
		["SimpleFontStringAPI:GetStringHeight"] = {
			SecretWhenAnchoringSecret = true,
			Arguments = {},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
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
		["SimpleFontStringAPI:SetText"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false, Default = "" },
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
		["SimpleFrameAPI:GetEffectiveScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
			Arguments = {},
			Returns = {
				{ Name = "effectiveScale", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:GetScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
			Arguments = {},
			Returns = {
				{ Name = "frameScale", Type = "number", Nilable = false },
			},
		},
		["SimpleFrameAPI:Hide"] = {
			IsProtectedFunction = true,
			Arguments = {},
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
		["SimpleFrameAPI:SetAlpha"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
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
		["SimpleFrameAPI:SetScale"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Scale },
			Arguments = {
				{ Name = "scale", Type = "number", Nilable = false },
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
		["SimpleHTMLAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
			Arguments = {
				{ Name = "text", Type = "cstring", Nilable = false },
				{ Name = "ignoreMarkup", Type = "bool", Nilable = false, Default = false },
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
		["SimpleRegionAPI:GetEffectiveScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
			Arguments = {},
			Returns = {
				{ Name = "effectiveScale", Type = "number", Nilable = false },
			},
		},
		["SimpleRegionAPI:GetScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
			Arguments = {},
			Returns = {
				{ Name = "scale", Type = "number", Nilable = false },
			},
		},
		["SimpleRegionAPI:SetAlpha"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
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
		["SimpleScriptRegionAPI:SetScript"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptBindings } },
			RequiresAssignableScript = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "scriptTypeName", Type = "ScriptTypeName", Nilable = false },
				{ Name = "script", Type = "LuaFunctionReference", Nilable = true },
			},
		},
		["SimpleScriptRegionAPI:Show"] = {
			Arguments = {},
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
		["SimpleTextureBaseAPI:SetSnapToPixelGrid"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "snap", Type = "bool", Nilable = false, Default = false },
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
		COMBAT_LOG_EVENT_UNFILTERED = {
			CallbackEvent = true,
			HasRestrictions = true,
			SynchronousEvent = true,
		},
		PLAYER_ENTERING_WORLD = {
			SynchronousEvent = true,
			Payload = {
				{ Name = "isInitialLogin", Type = "bool", Nilable = false },
				{ Name = "isReloadingUi", Type = "bool", Nilable = false },
			},
		},
		PLAYER_LOGIN = {
			SynchronousEvent = true,
		},
		PLAYER_REGEN_DISABLED = {
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
		VARIABLES_LOADED = {
			SynchronousEvent = true,
		},
	},
	undocumented = {
		"IsInGroup",
		"bit.band",
		"bit.bnot",
		"bit.bor",
		"bit.bxor",
		"bit.lshift",
		"bit.rshift",
		"date",
	},
}
