"""Tests for changelog-section.sh. Run: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("changelog-section.sh")

CHANGELOG = """# Changelog

## 0.4.4

- Pop a chat out into a mini chat.

### Fixes

- Replies show in full.

## 0.4.3

## 0.4.2

- The desktop app updates itself.
"""


def section(version, text=CHANGELOG):
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "CHANGELOG.md"
        path.write_text(text)
        return subprocess.run(
            [str(SCRIPT), version, str(path)], capture_output=True, text=True
        )


class ChangelogSectionTests(unittest.TestCase):
    def test_a_section_with_sub_headings_prints_up_to_the_next_version(self):
        result = section("0.4.4")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            "- Pop a chat out into a mini chat.\n\n### Fixes\n\n- Replies show in full.\n",
        )

    def test_the_last_section_prints_to_the_end_of_the_file(self):
        result = section("0.4.2")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "- The desktop app updates itself.\n")

    def test_an_empty_section_fails(self):
        result = section("0.4.3")

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn('"## 0.4.3" section of CHANGELOG.md is empty', result.stderr)

    def test_a_section_of_blank_lines_fails(self):
        result = section("0.4.3", "## 0.4.3\n\n   \n\n## 0.4.2\n\n- Old.\n")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is empty", result.stderr)

    def test_a_missing_section_fails(self):
        result = section("0.4.5")

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn('no "## 0.4.5" section', result.stderr)

    def test_a_version_prefix_does_not_match_a_longer_version(self):
        result = section("0.4", "## 0.4.4\n\n- New.\n")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('no "## 0.4" section', result.stderr)

    def test_the_changelog_of_the_repo_has_notes_for_its_version(self):
        root = SCRIPT.parent.parent
        cargo = (root / "Cargo.toml").read_text()
        version = cargo.split('\nversion = "', 1)[1].split('"', 1)[0]

        result = subprocess.run([str(SCRIPT), version], capture_output=True, text=True)

        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
