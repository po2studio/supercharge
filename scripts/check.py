"""Checks skill frontmatter and relative links in the repo's Markdown."""

import json
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
errors = []

for skill in sorted(root.glob("skills/*/SKILL.md")):
    text = skill.read_text()
    match = re.match(r"---\n(.*?)\n---\n", text, re.S)
    if not match:
        errors.append(f"{skill.relative_to(root)}: no frontmatter")
        continue
    fields = dict(
        line.split(":", 1) for line in match.group(1).splitlines() if ":" in line
    )
    name = fields.get("name", "").strip()
    if name != skill.parent.name:
        errors.append(f"{skill.relative_to(root)}: name '{name}' doesn't match its folder")
    description = fields.get("description", "").strip()
    if not description:
        errors.append(f"{skill.relative_to(root)}: no description")
    else:
        # The skills CLI shows only the first 57 characters as a hint, and the full text in a panel.
        first = re.split(r"\.(?:\s|$)", description, maxsplit=1)[0]
        if len(first) > 57:
            errors.append(
                f"{skill.relative_to(root)}: description is {len(first)} characters before its first full stop, over 57"
            )
        if len(description) > 1024:
            errors.append(f"{skill.relative_to(root)}: description is {len(description)} characters, over 1024")

PLATFORMS = ("linux-x64", "linux-arm64", "macos-arm64", "macos-x64")
pins = root / "skills/brain/brain.json"
brain_skill = root / "skills/brain/SKILL.md"
if brain_skill.exists() and not pins.exists():
    errors.append("skills/brain/brain.json: missing")
if pins.exists():
    name = "skills/brain/brain.json"
    data = None
    try:
        data = json.loads(pins.read_text())
    except json.JSONDecodeError as e:
        errors.append(f"{name}: {e}")
        data = {"unparsable": True}
    if not isinstance(data, dict):
        errors.append(f"{name}: must be a JSON object")
    elif "unparsable" not in data:
        version = data.get("version")
        if not isinstance(version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", version):
            errors.append(f"{name}: version must be X.Y.Z")
        if data.get("tag") != f"brain-v{version}":
            errors.append(f"{name}: tag must be brain-v{version}")
        if not re.fullmatch(r"[\w.-]+/[\w.-]+", str(data.get("repo", ""))):
            errors.append(f"{name}: repo must be owner/name")
        platforms = data.get("platforms")
        if not isinstance(platforms, dict) or set(platforms) != set(PLATFORMS):
            errors.append(f"{name}: platforms must be exactly {', '.join(PLATFORMS)}")
            platforms = {}
        sizes = []
        for platform, pin in platforms.items():
            pin = pin if isinstance(pin, dict) else {}
            if not isinstance(pin.get("asset"), str) or not pin["asset"]:
                errors.append(f"{name}: {platform} needs an asset")
            expected = f"https://github.com/{data.get('repo')}/releases/download/{data.get('tag')}/{pin.get('asset')}"
            if pin.get("url") != expected:
                errors.append(f"{name}: {platform} url must be {expected}")
            if not re.fullmatch(r"[0-9a-f]{64}", str(pin.get("sha256", ""))):
                errors.append(f"{name}: {platform} needs a sha256")
            if not isinstance(pin.get("bytes"), int) or pin["bytes"] <= 0:
                errors.append(f"{name}: {platform} needs a size in bytes")
            else:
                sizes.append(pin["bytes"])
        # The description is where the skills CLI shows the download size.
        if brain_skill.exists() and sizes:
            text = brain_skill.read_text()
            stated = re.search(r"description:.*?(\d+) MB", text)
            if "SIZE" in text.split("---")[1] or not stated:
                errors.append("skills/brain/SKILL.md: the description must state the download size in MB")
            elif not (min(sizes) / 1e6 - 1 <= int(stated.group(1)) <= max(sizes) / 1e6 + 1):
                errors.append(
                    f"skills/brain/SKILL.md: description says {stated.group(1)} MB, brain.json says {min(sizes) / 1e6:.0f} to {max(sizes) / 1e6:.0f} MB"
                )

for doc in sorted(root.rglob("*.md")):
    if {".git", "target", "assets"} & set(doc.relative_to(root).parts):
        continue
    for target in re.findall(r"\]\(([^)\s]+)\)", doc.read_text()):
        if re.match(r"[a-z]+:", target) or target.startswith("#"):
            continue
        if not (doc.parent / target.split("#")[0]).exists():
            errors.append(f"{doc.relative_to(root)}: broken link {target}")

print("\n".join(errors) or "ok")
sys.exit(1 if errors else 0)
