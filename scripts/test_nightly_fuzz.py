"""Tests for the fuzz matrix of nightly.yml. Run: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import re
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def fuzz_targets():
    cargo = tomllib.loads((ROOT / "fuzz" / "Cargo.toml").read_text())
    return sorted(binary["name"] for binary in cargo["bin"])


def nightly_matrix():
    workflow = (ROOT / ".github" / "workflows" / "nightly.yml").read_text()
    line = re.search(r"^\s*target: \[(.*)\]\s*$", workflow, re.M)
    return sorted(name.strip() for name in line.group(1).split(","))


class NightlyFuzzTest(unittest.TestCase):
    # A target that is missing from the matrix never gets the long nightly run, and nothing fails.
    def test_the_nightly_matrix_lists_every_fuzz_target(self):
        self.assertEqual(nightly_matrix(), fuzz_targets())


if __name__ == "__main__":
    unittest.main()
