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

pins = root / "skills/brain/brain.json"
if pins.exists():
    try:
        data = json.loads(pins.read_text())
        for key in ("version", "tag", "repo", "platforms"):
            if key not in data:
                errors.append(f"skills/brain/brain.json: no {key}")
        for platform, pin in data.get("platforms", {}).items():
            if not re.fullmatch(r"[0-9a-f]{64}", pin.get("sha256", "")):
                errors.append(f"skills/brain/brain.json: {platform} has no sha256")
            if not pin.get("url") or not isinstance(pin.get("bytes"), int):
                errors.append(f"skills/brain/brain.json: {platform} needs a url and a size in bytes")
    except json.JSONDecodeError as e:
        errors.append(f"skills/brain/brain.json: {e}")

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
