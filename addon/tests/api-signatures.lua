-- The documented WoW Forever 1.60.1.70170 API that GnomishRelay uses.
-- Written by scripts/wow-api.sh from Blizzard_APIDocumentationGenerated. Do not edit.
-- A patch can change the arguments, returns, or secret flags and keep the name. The diff shows it.
-- The scan does not know the type of each object, so methods has each widget type with a called name.
return {
	build = "1.60.1.70170",
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
		["C_Texture.GetAtlasInfo"] = {
			MayReturnNothing = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "atlas", Type = "textureAtlas", Nilable = false },
			},
			Returns = {
				{ Name = "info", Type = "AtlasInfo", Nilable = false },
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
		GetCursorPosition = {
			Returns = {
				{ Name = "posX", Type = "number", Nilable = false },
				{ Name = "posY", Type = "number", Nilable = false },
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
		IsMacClient = {
			Returns = {
				{ Name = "isMac", Type = "bool", Nilable = false },
			},
		},
		IsShiftKeyDown = {
			Returns = {
				{ Name = "down", Type = "bool", Nilable = false },
			},
		},
		Screenshot = {},
	},
	methods = {
		["DurationTextBindingObjectAPI:SetEnabled"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false },
			},
		},
		["FrameAPICharacterModelBase:SetRotation"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
				{ Name = "animate", Type = "bool", Nilable = false, Default = true },
			},
		},
		["FrameAPICooldown:SetRotation"] = {
			SecretArguments = "AllowedWhenUntainted",
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
		["FrameAPISimpleCheckout:SetFocus"] = {
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
		["LuaColorCurveObjectAPI:GetPoint"] = {
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "index", Type = "luaIndex", Nilable = false },
			},
			Returns = {
				{ Name = "point", Type = "LuaColorCurvePoint", Nilable = true },
			},
		},
		["LuaCurveObjectAPI:GetPoint"] = {
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "index", Type = "luaIndex", Nilable = false },
			},
			Returns = {
				{ Name = "point", Type = "vector2", Mixin = "Vector2DMixin", Nilable = true },
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
		["SimpleBrowserAPI:SetFocus"] = {
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
		["SimpleButtonAPI:SetEnabled"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.ButtonState },
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false, Default = false },
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
		["SimpleButtonAPI:SetNormalTexture"] = {
			CheckAllowChangeParent = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
		},
		["SimpleButtonAPI:SetPushedTexture"] = {
			CheckAllowChangeParent = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "asset", Type = "TextureAsset", Nilable = false },
			},
		},
		["SimpleButtonAPI:SetText"] = {
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text },
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "checked", Type = "bool", Nilable = false, Default = false },
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
		["SimpleEditBoxAPI:HasFocus"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.QueryFocus } },
			Arguments = {},
			Returns = {
				{ Name = "hasFocus", Type = "bool", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:HighlightText"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "start", Type = "number", Nilable = false, Default = 0 },
				{ Name = "stop", Type = "number", Nilable = false, Default = -1 },
			},
		},
		["SimpleEditBoxAPI:SetAltArrowKeyMode"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "altMode", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetAutoFocus"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "autoFocus", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetCursorPosition"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptedInput } },
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Cursor },
			Arguments = {
				{ Name = "cursorPosition", Type = "number", Nilable = false },
			},
		},
		["SimpleEditBoxAPI:SetEnabled"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleEditBoxAPI:SetFocus"] = {
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.ScriptedInput } },
			Arguments = {},
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
		["SimpleFontStringAPI:SetMaxLines"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "maxLines", Type = "number", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetNonSpaceWrap"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "wrap", Type = "bool", Nilable = false },
			},
		},
		["SimpleFontStringAPI:SetRotation"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
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
		["SimpleFrameAPI:GetEffectiveScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
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
			SecretReturnsForAspect = { Enum.SecretAspect.Shown },
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
		["SimpleFrameAPI:SetAlpha"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Alpha },
			Arguments = {
				{ Name = "alpha", Type = "SingleColorValue", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetClampRectInsets"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "left", Type = "uiUnit", Nilable = false },
				{ Name = "right", Type = "uiUnit", Nilable = false },
				{ Name = "top", Type = "uiUnit", Nilable = false },
				{ Name = "bottom", Type = "uiUnit", Nilable = false },
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
		["SimpleFrameAPI:SetResizable"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "resizable", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetResizeBounds"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "minWidth", Type = "uiUnit", Nilable = false },
				{ Name = "minHeight", Type = "uiUnit", Nilable = false },
				{ Name = "maxWidth", Type = "uiUnit", Nilable = true },
				{ Name = "maxHeight", Type = "uiUnit", Nilable = true },
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
		["SimpleFrameAPI:SetToplevel"] = {
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Toplevel },
			Arguments = {
				{ Name = "topLevel", Type = "bool", Nilable = false },
			},
		},
		["SimpleFrameAPI:SetUserPlaced"] = {
			SecretArguments = "AllowedWhenUntainted",
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
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "resizePoint", Type = "FramePoint", Nilable = true },
				{ Name = "alwaysStartFromMouse", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleFrameAPI:StopMovingOrSizing"] = {
			IsProtectedFunction = true,
			Arguments = {},
		},
		["SimpleFrameScriptObjectAPI:GetName"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.ObjectName },
			Arguments = {},
			Returns = {
				{ Name = "name", Type = "cstring", Nilable = false },
			},
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
		["SimpleRegionAPI:GetEffectiveScale"] = {
			SecretReturnsForAspect = { Enum.SecretAspect.Scale },
			Arguments = {},
			Returns = {
				{ Name = "effectiveScale", Type = "number", Nilable = false },
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
		["SimpleScriptRegionAPI:GetCenter"] = {
			MayReturnNothing = true,
			SecretWhenAnchoringSecret = true,
			Arguments = {},
			Returns = {
				{ Name = "x", Type = "uiUnit", Nilable = false },
				{ Name = "y", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:GetHeight"] = {
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenAnchoringSecret = true,
			Arguments = {
				{ Name = "ignoreRect", Type = "bool", Nilable = false, Default = false },
			},
			Returns = {
				{ Name = "height", Type = "uiUnit", Nilable = false },
			},
		},
		["SimpleScriptRegionAPI:GetWidth"] = {
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenAnchoringSecret = true,
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
		["SimpleScriptRegionAPI:IsMouseOver"] = {
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenAnchoringSecret = true,
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
		["SimpleScriptRegionResizingAPI:GetPoint"] = {
			ConstSecretAccessor = true,
			MayReturnNothing = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenAnchoringSecret = true,
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
		["SimpleSliderAPI:SetEnabled"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "enabled", Type = "bool", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetMinMaxValues"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "minValue", Type = "number", Nilable = false },
				{ Name = "maxValue", Type = "number", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetObeyStepOnDrag"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "obeyStepOnDrag", Type = "bool", Nilable = false },
			},
		},
		["SimpleSliderAPI:SetValue"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "value", Type = "number", Nilable = false },
				{ Name = "treatAsMouseEvent", Type = "bool", Nilable = false, Default = false },
			},
		},
		["SimpleSliderAPI:SetValueStep"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "valueStep", Type = "number", Nilable = false },
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
		["SimpleTextureBaseAPI:SetAtlas"] = {
			SecretArguments = "AllowedWhenTainted",
			Arguments = {
				{ Name = "atlas", Type = "textureAtlas", Nilable = false },
				{ Name = "useAtlasSize", Type = "bool", Nilable = false, NeverSecret = true, Default = false },
				{ Name = "filterMode", Type = "FilterMode", Nilable = true, NeverSecret = true },
				{ Name = "resetTexCoords", Type = "bool", Nilable = true, NeverSecret = true },
				{ Name = "wrapModeHorizontal", Type = "cstring", Nilable = true, NeverSecret = true },
				{ Name = "wrapModeVertical", Type = "cstring", Nilable = true, NeverSecret = true },
			},
		},
		["SimpleTextureBaseAPI:SetBlendMode"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "blendMode", Type = "BlendMode", Nilable = false },
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
		["SimpleTextureBaseAPI:SetRotation"] = {
			SecretArguments = "AllowedWhenTainted",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Rotation },
			Arguments = {
				{ Name = "radians", Type = "number", Nilable = false },
				{ Name = "normalizedRotationPoint", Type = "vector2", Mixin = "Vector2DMixin", Nilable = true },
			},
		},
		["SimpleTextureBaseAPI:SetSnapToPixelGrid"] = {
			SecretArguments = "AllowedWhenUntainted",
			Arguments = {
				{ Name = "snap", Type = "bool", Nilable = false, Default = false },
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
		"EnumerateFrames",
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
