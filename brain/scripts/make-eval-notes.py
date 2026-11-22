#!/usr/bin/env python3
"""Writes the notes of the eval corpus (`eval/corpus/notes/`): what an agent would write after
answering a question from several files, with the sha256 each source has now. Rerun it after
changing a corpus file a note cites; `brain eval` fails while an expected note is stale.

    python3 brain/scripts/make-eval-notes.py        (from the repository root)

Two notes are stale on purpose, the traps of `eval/questions/notes.yaml`: one cites an earlier
version of a file (a hash nothing has), one a file that no longer exists.
"""
import hashlib
from pathlib import Path

CORPUS = Path(__file__).resolve().parent.parent / "eval" / "corpus"
NOTES = CORPUS / "notes"


def sha(path):
    return hashlib.sha256((CORPUS / path).read_bytes()).hexdigest()


def note(file, title, summary, aliases, questions, sources, verified, body):
    lines = ["---", f"title: {title}", f"summary: {summary}", f"aliases: [{', '.join(aliases)}]", "questions:"]
    lines += [f"  - {q}" for q in questions]
    lines.append("sources:")
    for path, hash in sources:
        lines += [f"  - path: {path}", f"    hash: {hash}"]
    lines += [f"verified: {verified}", "---", "", body.strip(), ""]
    NOTES.mkdir(exist_ok=True)
    (NOTES / file).write_text("\n".join(lines))


note(
    "trip-limits.md",
    "Business trips: approval, meals and hotels",
    "A trip needs written approval ten days ahead; meals abroad are a flat EUR 45 a day; Reykjavik hotels are capped at ISK 22,000 a night.",
    ["travel limits", "trip rules"],
    ["Which approval and meal limits apply when I travel abroad for work?"],
    [("policies/travel-expenses.md", sha("policies/travel-expenses.md")), ("policies/expenses.md", sha("policies/expenses.md"))],
    "2025-02-10",
    """
- Approval: in writing from the line manager at least 10 days before departure.
- Meals outside Iceland: a flat EUR 45 per day, no receipts. Meals in Iceland: ISK 4,500 each.
- Lodging: ISK 22,000 a night in Reykjavik, EUR 160 abroad, breakfast included where possible.
- Claims go in within 14 days of returning.
""",
)

note(
    "working-from-home.md",
    "Working from home: days, hours and money",
    "Up to three days a week from home, reachable 10:00-15:00; a one-off ISK 60,000 grant and ISK 5,000 a month for internet.",
    ["remote work summary", "heimavinna"],
    ["What are the rules and the allowances for working from home?"],
    [
        ("policies/remote-work.md", sha("policies/remote-work.md")),
        ("policies/home-office-allowance.md", sha("policies/home-office-allowance.md")),
        ("icelandic/heimavinna.md", sha("icelandic/heimavinna.md")),
    ],
    "2025-02-10",
    """
- Up to three days a week; office days are normally Tuesday and Thursday.
- Reachable during core hours, 10:00 to 15:00 Icelandic time.
- Working from abroad: at most 20 days a year, two weeks' notice to People.
- Money: a one-off ISK 60,000 for desk, chair or monitor arm within the first 90 days, and ISK 5,000 a month towards home internet.
""",
)

note(
    "puffin-lead-background.md",
    "Who leads Project Puffin, and where they come from",
    "Guðmundur Ólafsson, the CTO, leads Puffin; before Hrafn he led the routing team at Kolibri Systems in Copenhagen.",
    ["puffin lead", "CTO background"],
    ["Who leads the database migration and what did they do before joining Hrafn?"],
    [("product/project-puffin.md", sha("product/project-puffin.md")), ("people/bio-gudmundur.md", sha("people/bio-gudmundur.md"))],
    "2025-02-10",
    """
Guðmundur Ólafsson leads Puffin, the MySQL to PostgreSQL migration of the bookings database. He is the CTO; he joined in March 2020 and spent seven years at Kolibri Systems, a Copenhagen logistics software firm, before that.
""",
)

# The traps: stale, so they rank below the raw files they went out of date against.
note(
    "sick-leave-summary.md",
    "Sick leave and doctor's notes",
    "A doctor's note is needed from the first day of illness.",
    ["sick note", "doctor's note"],
    ["Do I need a doctor's note if I'm ill for a day?"],
    [("policies/sick-leave.md", hashlib.sha256(b"an earlier version of policies/sick-leave.md\n").hexdigest())],
    "2024-06-01",
    """
Staff who are ill need a doctor's note from the first day, sent to the People team. Full pay continues for the statutory period.
""",
)

note(
    "hotel-cap-2023.md",
    "Hotel cap in Reykjavik",
    "Hotels in Reykjavik are capped at ISK 18,000 a night.",
    ["hotel limit", "nightly hotel cap"],
    ["How much can the company pay per night for a hotel in Reykjavik?"],
    [("policies/travel-policy-2023.md", hashlib.sha256(b"policies/travel-policy-2023.md, deleted\n").hexdigest())],
    "2023-09-01",
    """
The hotel cap in Reykjavik is ISK 18,000 per night. Private rentals are fine if they are cheaper.
""",
)
