"""Tests for curseforge_versions.py. Run: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import unittest

import curseforge_versions

TOC = "## Interface: 16001, 20506\n## Title: Gnomish Relay\n"
FOREVER = {"gameVersionTypeID": 88568, "name": "1.60.1"}
ANNIVERSARY = {"gameVersionTypeID": 73246, "name": "2.5.6"}


class CurseForgeVersionsTest(unittest.TestCase):
    def test_an_interface_number_gives_its_game_version(self):
        self.assertEqual(curseforge_versions.game_version(16001), "1.60.1")
        self.assertEqual(curseforge_versions.game_version(20506), "2.5.6")

    def test_the_toc_lists_every_interface_number(self):
        self.assertEqual(curseforge_versions.interfaces(TOC), [16001, 20506])

    def test_nothing_is_missing_when_curseforge_lists_each_version(self):
        self.assertEqual(curseforge_versions.missing([FOREVER, ANNIVERSARY], TOC), [])

    def test_a_version_that_curseforge_lacks_is_missing(self):
        self.assertEqual(curseforge_versions.missing([FOREVER], TOC), ["2.5.6"])

    def test_a_version_of_another_game_type_does_not_count(self):
        classic = {"gameVersionTypeID": 67408, "name": "2.5.6"}
        self.assertEqual(curseforge_versions.missing([FOREVER, classic], TOC), ["2.5.6"])

    def test_an_interface_with_no_known_game_type_is_refused(self):
        with self.assertRaisesRegex(ValueError, "120001"):
            curseforge_versions.missing([], "## Interface: 120001\n")

    def test_the_addon_toc_names_only_known_game_types(self):
        toc = (curseforge_versions.Path(__file__).parent.parent / "addon/GnomishRelay/GnomishRelay.toc").read_text()
        for interface in curseforge_versions.interfaces(toc):
            self.assertTrue(curseforge_versions.game_type(interface))


if __name__ == "__main__":
    unittest.main()
