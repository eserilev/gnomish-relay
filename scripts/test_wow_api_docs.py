"""Tests for wow_api_docs.py. Run: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import tempfile
import unittest
from pathlib import Path

import wow_api_docs

VIDEO_DOC = """local Video =
{
	Name = "Video",
	Type = "System",
	Namespace = "C_VideoOptions",
	Environment = "All",

	Functions =
	{
		{
			Name = "GetGameWindowSizes",
			Type = "Function",
			SecretArguments = "AllowedWhenUntainted",
			Documentation = { "Prose that a patch can change freely." },

			Arguments =
			{
				{ Name = "monitor", Type = "number", Nilable = false, Documentation = { "More prose." } },
				{ Name = "fullscreen", Type = "bool", Nilable = true, Default = false },
			},

			Returns =
			{
				{ Name = "sizes", Type = "table", InnerType = "vector2", Nilable = false },
			},
		},
	},

	Events =
	{
		{
			Name = "ScreenshotSucceeded",
			Type = "Event",
			LiteralName = "SCREENSHOT_SUCCEEDED",
			SynchronousEvent = true,
			SecretWhenUnitThreatValuesRestricted = true,
			Payload =
			{
				{ Name = "path", Type = "cstring", Nilable = false },
			},
		},
	},

	Tables =
	{
		{
			Name = "Flags",
			Type = "Constants",
			Values =
			{
				{ Name = "BOTH", Type = "Flags", Value = Enum.Flags.A + Enum.Flags.B },
			},
		},
	},
};

APIDocumentation:AddDocumentationTable(Video);
"""

FRAME_DOC = """local SimpleFrameAPI =
{
	Name = "SimpleFrameAPI",
	Type = "ScriptObject",

	Functions =
	{
		{
			Name = "RegisterEvent",
			Type = "Function",
			ChecksForbiddenAspects = { { Argument = "self", Aspect = Enum.ForbiddenAspect.EventRegistrations } },

			Arguments =
			{
				{ Name = "eventName", Type = "cstring", Nilable = false },
			},
		},
	},
};

APIDocumentation:AddDocumentationTable(SimpleFrameAPI);
"""

GLOBAL_DOC = """local Client =
{
	Name = "Client",
	Type = "System",

	Functions =
	{
		{
			Name = "Screenshot",
			Type = "Function",
		},
	},
};
"""


def docs_of(*texts):
    docs = wow_api_docs.Docs()
    for text in texts:
        docs.add(wow_api_docs.parse_doc(text))
    return docs


class ParseTests(unittest.TestCase):
    def test_parse_keeps_arguments_returns_and_secret_flags_of_a_function(self):
        docs = docs_of(VIDEO_DOC)

        entry = docs.functions["C_VideoOptions.GetGameWindowSizes"]

        self.assertEqual(entry["SecretArguments"], "AllowedWhenUntainted")
        self.assertEqual(
            entry["Arguments"],
            [
                {"Name": "monitor", "Type": "number", "Nilable": False},
                {"Name": "fullscreen", "Type": "bool", "Nilable": True, "Default": False},
            ],
        )
        self.assertEqual(entry["Returns"], [{"Name": "sizes", "Type": "table", "InnerType": "vector2", "Nilable": False}])

    def test_parse_drops_names_and_prose(self):
        docs = docs_of(VIDEO_DOC)

        entry = docs.functions["C_VideoOptions.GetGameWindowSizes"]

        self.assertNotIn("Name", entry)
        self.assertNotIn("Type", entry)
        self.assertNotIn("Documentation", entry)
        self.assertNotIn("Documentation", entry["Arguments"][0])

    def test_parse_keys_an_event_by_its_literal_name_with_payload_and_flags(self):
        docs = docs_of(VIDEO_DOC)

        entry = docs.events["SCREENSHOT_SUCCEEDED"]

        self.assertEqual(
            entry,
            {
                "SynchronousEvent": True,
                "SecretWhenUnitThreatValuesRestricted": True,
                "Payload": [{"Name": "path", "Type": "cstring", "Nilable": False}],
            },
        )

    def test_parse_names_a_global_function_without_a_namespace(self):
        docs = docs_of(GLOBAL_DOC)

        self.assertEqual(docs.functions, {"Screenshot": {}})

    def test_parse_names_a_widget_method_by_its_object(self):
        docs = docs_of(FRAME_DOC)

        self.assertEqual(docs.methods_named("RegisterEvent"), ["SimpleFrameAPI:RegisterEvent"])
        self.assertEqual(docs.methods_named("Register"), [])

    def test_parse_keeps_a_bare_enum_name_as_a_name(self):
        docs = docs_of(FRAME_DOC)

        flag = docs.functions["SimpleFrameAPI:RegisterEvent"]["ChecksForbiddenAspects"]

        self.assertEqual(flag, [{"Argument": "self", "Aspect": "Enum.ForbiddenAspect.EventRegistrations"}])
        self.assertIsInstance(flag[0]["Aspect"], wow_api_docs.Ident)

    def test_parse_refuses_the_same_function_twice_with_different_entries(self):
        changed = GLOBAL_DOC.replace('Type = "Function",', 'Type = "Function",\n\t\t\tHasRestrictions = true,')

        with self.assertRaises(ValueError):
            docs_of(GLOBAL_DOC, changed)

    def test_read_docs_reads_every_lua_file_of_a_folder(self):
        with tempfile.TemporaryDirectory() as folder:
            Path(folder, "VideoDocumentation.lua").write_text(VIDEO_DOC, encoding="utf-8")
            Path(folder, "ClientDocumentation.lua").write_text(GLOBAL_DOC, encoding="utf-8")

            docs = wow_api_docs.read_docs(folder)

        self.assertEqual(sorted(docs.functions), ["C_VideoOptions.GetGameWindowSizes", "Screenshot"])


class LuaTests(unittest.TestCase):
    def test_lua_entry_writes_one_flag_and_one_field_per_line(self):
        docs = docs_of(VIDEO_DOC)

        text = wow_api_docs.lua_entry("SCREENSHOT_SUCCEEDED", docs.events["SCREENSHOT_SUCCEEDED"], "\t")

        self.assertEqual(
            text,
            "\tSCREENSHOT_SUCCEEDED = {\n"
            "\t\tSecretWhenUnitThreatValuesRestricted = true,\n"
            "\t\tSynchronousEvent = true,\n"
            "\t\tPayload = {\n"
            '\t\t\t{ Name = "path", Type = "cstring", Nilable = false },\n'
            "\t\t},\n"
            "\t},\n",
        )

    def test_lua_entry_quotes_a_key_with_a_dot_and_writes_bare_enum_names(self):
        docs = docs_of(FRAME_DOC)

        text = wow_api_docs.lua_entry("SimpleFrameAPI:RegisterEvent", docs.functions["SimpleFrameAPI:RegisterEvent"], "")

        self.assertTrue(text.startswith('["SimpleFrameAPI:RegisterEvent"] = {\n'))
        self.assertIn("Aspect = Enum.ForbiddenAspect.EventRegistrations", text)

    def test_lua_entry_writes_an_empty_entry_on_one_line(self):
        self.assertEqual(wow_api_docs.lua_entry("Screenshot", {}, "\t"), "\tScreenshot = {},\n")

    def test_a_new_secret_flag_changes_the_written_entry(self):
        before = docs_of(GLOBAL_DOC).functions["Screenshot"]
        after_text = GLOBAL_DOC.replace('Type = "Function",', 'Type = "Function",\n\t\t\tSecretReturns = true,')
        after = docs_of(after_text).functions["Screenshot"]

        old = wow_api_docs.lua_entry("Screenshot", before, "")
        new = wow_api_docs.lua_entry("Screenshot", after, "")

        self.assertNotEqual(old, new)
        self.assertIn("SecretReturns = true,", new)


if __name__ == "__main__":
    unittest.main()
