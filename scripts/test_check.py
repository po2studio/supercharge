"""Tests for the `skills/brain/brain.json` rules in check.py: `python3 -m unittest scripts/test_check.py`."""

import copy
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CHECK = Path(__file__).resolve().parent / "check.py"
PLATFORMS = ["linux-x64", "linux-arm64", "macos-arm64", "macos-x64"]
SKILL = "---\nname: brain\ndescription: Search your documents (175 MB download on first use). Answers from files.\n---\n\n# brain\n"
GOOD = {
    "version": "0.1.0",
    "tag": "brain-v0.1.0",
    "repo": "o/r",
    "platforms": {
        p: {
            "asset": f"brain-{p}",
            "url": f"https://github.com/o/r/releases/download/brain-v0.1.0/brain-{p}",
            "sha256": "a" * 64,
            "bytes": 175_000_000,
        }
        for p in PLATFORMS
    },
}


def check(pins, raw=None):
    """Runs check.py on a tree holding only the brain skill; returns (exit code, output)."""
    with tempfile.TemporaryDirectory() as tmp:
        (Path(tmp) / "scripts").mkdir()
        shutil.copy(CHECK, Path(tmp) / "scripts/check.py")
        skill = Path(tmp) / "skills/brain"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").write_text(SKILL)
        if raw is not None:
            (skill / "brain.json").write_text(raw)
        elif pins is not None:
            (skill / "brain.json").write_text(json.dumps(pins))
        run = subprocess.run([sys.executable, str(Path(tmp) / "scripts/check.py")], capture_output=True, text=True)
        return run.returncode, run.stdout


class BrainPins(unittest.TestCase):
    def assert_rejected(self, pins, needle):
        code, out = check(pins)
        self.assertEqual(code, 1, out)
        self.assertIn(needle, out)

    def assert_rejected_text(self, raw, needle):
        code, out = check(None, raw)
        self.assertEqual(code, 1, out)
        self.assertIn(needle, out)

    def test_complete_pins_pass(self):
        self.assertEqual(check(GOOD), (0, "ok\n"))

    def test_pins_are_required(self):
        self.assert_rejected(None, "brain.json: missing")

    def test_the_pins_must_be_an_object(self):
        for text in ("null", "[]", '"x"', "0"):
            self.assert_rejected_text(text, "must be a JSON object")

    def test_an_empty_or_broken_file_is_rejected(self):
        self.assert_rejected_text("{}", "tag must be")
        self.assert_rejected_text("{", "brain.json:")

    def test_all_four_platforms_are_required(self):
        pins = copy.deepcopy(GOOD)
        pins["platforms"] = {}
        self.assert_rejected(pins, "platforms must be exactly")
        pins = copy.deepcopy(GOOD)
        del pins["platforms"]["macos-x64"]
        self.assert_rejected(pins, "platforms must be exactly")

    def test_each_platform_needs_asset_url_sha256_and_bytes(self):
        for key, bad, needle in [
            ("asset", "", "needs an asset"),
            ("url", "https://elsewhere/x", "url must be"),
            ("sha256", "abc", "needs a sha256"),
            ("bytes", 0, "needs a size in bytes"),
        ]:
            pins = copy.deepcopy(GOOD)
            pins["platforms"]["linux-x64"][key] = bad
            self.assert_rejected(pins, needle)
            del pins["platforms"]["linux-x64"][key]
            code, _ = check(pins)
            self.assertEqual(code, 1, key)

    def test_tag_follows_the_version(self):
        pins = copy.deepcopy(GOOD)
        pins["tag"] = "brain-v9.9.9"
        self.assert_rejected(pins, "tag must be brain-v0.1.0")

    def test_the_description_states_the_pinned_size(self):
        pins = copy.deepcopy(GOOD)
        for pin in pins["platforms"].values():
            pin["bytes"] = 400_000_000
        self.assert_rejected(pins, "description says 175 MB")


if __name__ == "__main__":
    unittest.main()
