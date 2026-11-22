#!/usr/bin/env python3
"""Writes the binary files of the eval corpus: one file per format `brain` converts, a few that
cannot be converted, and the versions of a partner agreement. Plain standard library, and the
output does not vary from run to run, so the files in git change only when this script does.

    python3 brain/scripts/make-eval-formats.py        (from the repository root)

The text files of the same folders (HTML, CSV, JSON, YAML, source code, the transcript, the
email and the mailbox) are written by hand and are not touched here.
"""
import hashlib
import io
import struct
import zipfile
import zlib
from pathlib import Path

CORPUS = Path(__file__).resolve().parent.parent / "eval" / "corpus"
FORMATS = CORPUS / "formats"
DUPLICATES = CORPUS / "duplicates"


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def zipped(parts, stored_first=None):
    """A zip with fixed timestamps; `stored_first` is a (name, text) kept uncompressed first."""
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        if stored_first:
            info = zipfile.ZipInfo(stored_first[0], (2024, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            z.writestr(info, stored_first[1])
        for name, body in parts:
            info = zipfile.ZipInfo(name, (2024, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, body)
    return out.getvalue()


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


# ---- Word -------------------------------------------------------------------------------------

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'


def para(text, style=None, bullet=False):
    ppr = ""
    if style:
        ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>'
    elif bullet:
        ppr = '<w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>'
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{esc(text)}</w:t></w:r></w:p>'


def table(rows):
    out = "<w:tbl>"
    for row in rows:
        out += "<w:tr>" + "".join(f"<w:tc>{para(c)}</w:tc>" for c in row) + "</w:tr>"
    return out + "</w:tbl>"


def core(title, modified):
    return (
        '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/">'
        f"<dc:title>{esc(title)}</dc:title><dcterms:created>{modified}</dcterms:created>"
        f"<dcterms:modified>{modified}</dcterms:modified></cp:coreProperties>"
    )


def docx(title, modified, body):
    types = (
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        "</Types>"
    )
    rels = (
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
        "</Relationships>"
    )
    styles = (
        f"<w:styles {W}>"
        '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style>'
        "</w:styles>"
    )
    document = f"<w:document {W}><w:body>{body}</w:body></w:document>"
    return zipped(
        [
            ("[Content_Types].xml", types),
            ("_rels/.rels", rels),
            ("word/document.xml", document),
            ("word/styles.xml", styles),
            ("docProps/core.xml", core(title, modified)),
        ]
    )


def onboarding_docx():
    body = (
        para("Operator onboarding checklist", "Heading1")
        + para("Every new tour operator goes through the same steps before it appears on the booking site.")
        + para("Before going live", "Heading2")
        + para("Collect the signed data processing agreement and the company registration.", bullet=True)
        + para("Create the operator's staff accounts and send the invitations.", bullet=True)
        + para("Run one test booking of EUR 1 through the live payment provider and refund it.", bullet=True)
        + para(
            "An operator is activated only when the signed data processing agreement is back and the EUR 1 test booking has been refunded."
        )
        + para("Who does what", "Heading2")
        + table(
            [
                ["Step", "Owner", "Days"],
                ["Contract and DPA", "Legal", "3"],
                ["Accounts and invitations", "Support", "1"],
                ["Test booking and refund", "Finance", "2"],
            ]
        )
    )
    return docx("Operator onboarding checklist", "2024-10-02T09:30:00Z", body)


AGREEMENT = [
    "This partner agreement is made between Hrafn ehf and the partner named in the order form and governs the resale of Hrafn bookings through the partner's own website.",
    "The partner may display Hrafn availability and prices on its website and may take bookings on Hrafn's behalf, provided that every booking is confirmed through the Hrafn booking interface and the guest receives the standard confirmation email.",
    "Hrafn pays the partner a commission of {commission} percent of the net booking value, calculated monthly and paid within thirty days of the end of each calendar month, after deducting refunds and cancellations that occurred in that month.",
    "Either party may end this agreement by giving {notice} days written notice to the other party. Bookings already confirmed before the end date remain valid and the commission on them is still paid.",
    "The partner shall not alter the prices supplied by Hrafn, shall not combine Hrafn bookings with products from other suppliers without Hrafn's written consent, and shall remove any Hrafn content from its website within seven days of the end of this agreement.",
    "Each party shall keep the other party's commercial information confidential during the agreement and for three years after it ends, and shall protect any personal data of guests in line with the data processing agreement signed with the order form.",
    "This agreement is governed by Icelandic law, and any dispute that the parties cannot settle between themselves shall be brought before the District Court of Reykjavik.",
]


def agreement_docx(commission, notice, modified, extra=""):
    body = para("Partner agreement", "Heading1") + "".join(
        para(p.format(commission=commission, notice=notice)) for p in AGREEMENT
    )
    if extra:
        body += para(extra)
    return docx("Partner agreement", modified, body)


# ---- PowerPoint -------------------------------------------------------------------------------

A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'


def shape(text, kind=None):
    ph = f'<p:nvSpPr><p:nvPr><p:ph type="{kind}"/></p:nvPr></p:nvSpPr>' if kind else ""
    return f"<p:sp>{ph}<p:txBody><a:p><a:r><a:t>{esc(text)}</a:t></a:r></a:p></p:txBody></p:sp>"


def pptx():
    slides = [
        shape("Mission Control rollout", "title")
        + shape("Pilot with 12 operators starts on 3 March; general availability follows on 28 April."),
        shape("What operators get", "title")
        + shape("One screen for availability, prices and today's bookings.")
        + shape("Alerts when a departure is close to full."),
        shape("Support", "title") + shape("A named contact for every pilot operator during March."),
    ]
    notes = (
        f"<p:notes {A}>" + shape("Slide image", "sldImg") + shape(
            "Remind the room that the pilot leaves out the Westfjords operators until the ferry timetable feed is ready.",
            "body",
        )
        + "</p:notes>"
    )
    parts = [
        (
            "[Content_Types].xml",
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>',
        ),
        ("docProps/core.xml", core("All hands Q1 2025", "2025-02-27T14:00:00Z")),
    ]
    for i, s in enumerate(slides, 1):
        parts.append((f"ppt/slides/slide{i}.xml", f"<p:sld {A}>{s}</p:sld>"))
    parts.append(("ppt/notesSlides/notesSlide1.xml", notes))
    parts.append(
        (
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            '<Relationships><Relationship Id="rId1" Target="../slides/slide1.xml"/></Relationships>',
        )
    )
    return zipped(parts)


# ---- Excel ------------------------------------------------------------------------------------


def col(n):
    return "ABCDEFGHIJ"[n]


def xlsx():
    sheets = {
        "Q3 2024": [
            ["Region", "Bookings", "Revenue EUR"],
            ["Reykjanes", 1204, 318500],
            ["Snæfellsnes", 877, 241900],
            ["Eastfjords", 391, 102300],
            ["Highlands", 256, 98750],
        ],
        "Q4 2024": [
            ["Region", "Bookings", "Revenue EUR"],
            ["Reykjanes", 980, 266100],
            ["Snæfellsnes", 702, 190450],
            ["Eastfjords", 512, 133800],
            ["Highlands", 118, 41200],
        ],
    }
    strings = []

    def sid(s):
        if s not in strings:
            strings.append(s)
        return strings.index(s)

    sheet_xml = []
    for rows in sheets.values():
        out = ""
        for r, row in enumerate(rows, 1):
            cells = ""
            for c, v in enumerate(row):
                ref = f"{col(c)}{r}"
                cells += (
                    f'<c r="{ref}" t="s"><v>{sid(v)}</v></c>'
                    if isinstance(v, str)
                    else f'<c r="{ref}"><v>{v}</v></c>'
                )
            out += f'<row r="{r}">{cells}</row>'
        sheet_xml.append(
            '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
            + out
            + "</sheetData></worksheet>"
        )
    ns = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
    workbook = (
        f"<workbook {ns}><sheets>"
        + "".join(
            f'<sheet name="{n}" sheetId="{i}" r:id="rId{i}"/>' for i, n in enumerate(sheets, 1)
        )
        + "</sheets></workbook>"
    )
    wb_rels = (
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + "".join(
            f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>'
            for i in range(1, len(sheets) + 1)
        )
        + f'<Relationship Id="rId{len(sheets) + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>'
        "</Relationships>"
    )
    sst = (
        f'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{len(strings)}" uniqueCount="{len(strings)}">'
        + "".join(f"<si><t>{esc(s)}</t></si>" for s in strings)
        + "</sst>"
    )
    root_rels = (
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
        "</Relationships>"
    )
    types = (
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        "</Types>"
    )
    parts = [
        ("[Content_Types].xml", types),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", wb_rels),
        ("xl/sharedStrings.xml", sst),
        ("docProps/core.xml", core("Regional sales 2024", "2025-01-08T08:00:00Z")),
    ]
    for i, x in enumerate(sheet_xml, 1):
        parts.append((f"xl/worksheets/sheet{i}.xml", x))
    return zipped(parts)


# ---- OpenDocument -----------------------------------------------------------------------------

ODF = (
    'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
    'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
    'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
    'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0"'
)


def odf(mime, title, date, body):
    manifest = (
        '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0">'
        f'<manifest:file-entry manifest:full-path="/" manifest:media-type="{mime}"/>'
        "</manifest:manifest>"
    )
    meta = (
        f"<office:document-meta {ODF}><office:meta><dc:title>{esc(title)}</dc:title>"
        f"<dc:date>{date}</dc:date></office:meta></office:document-meta>"
    )
    content = f"<office:document-content {ODF}><office:body>{body}</office:body></office:document-content>"
    return zipped(
        [("META-INF/manifest.xml", manifest), ("content.xml", content), ("meta.xml", meta)],
        stored_first=("mimetype", mime),
    )


def odf_cells(row):
    return "<table:table-row>" + "".join(
        f"<table:table-cell><text:p>{esc(str(c))}</text:p></table:table-cell>" for c in row
    ) + "</table:table-row>"


def odt():
    body = (
        "<office:text>"
        '<text:h text:outline-level="1">Refund exceptions</text:h>'
        "<text:p>Refunds are paid in full when a guest cancels at least 14 days before departure.</text:p>"
        '<text:h text:outline-level="2">Outside the 14-day window</text:h>'
        "<text:p>A refund outside the window needs approval from two people in finance. "
        "A single approver may sign off at most EUR 300; a refund of EUR 500 therefore needs both.</text:p>"
        "</office:text>"
    )
    return odf("application/vnd.oasis.opendocument.text", "Refund exceptions", "2024-11-20T10:00:00", body)


def ods():
    rows = [
        ["Vendor", "Renews", "Notice", "Annual cost EUR"],
        ["Lindi Hosting", "2025-09-01", "60 days", 48200],
        ["Kolbrún Analytics", "2025-03-15", "30 days", 12900],
        ["Fjörður Payments", "2026-01-01", "90 days", 0],
    ]
    body = (
        '<office:spreadsheet><table:table table:name="Vendor contracts">'
        + "".join(odf_cells(r) for r in rows)
        + "</table:table></office:spreadsheet>"
    )
    return odf("application/vnd.oasis.opendocument.spreadsheet", "Vendor contracts", "2025-01-30T12:00:00", body)


def odp():
    pages = [
        ("Roadmap 2025", "The voucher wallet for guests ships in the third quarter, after the payments rework."),
        ("Later", "Group quotes for schools follow in the fourth quarter."),
    ]
    body = "<office:presentation>" + "".join(
        f'<draw:page draw:name="page{i}"><draw:frame><draw:text-box><text:p>{esc(t)}</text:p>'
        f"<text:p>{esc(b)}</text:p></draw:text-box></draw:frame></draw:page>"
        for i, (t, b) in enumerate(pages, 1)
    ) + "</office:presentation>"
    return odf("application/vnd.oasis.opendocument.presentation", "Product roadmap 2025", "2025-02-10T09:00:00", body)


# ---- PDF --------------------------------------------------------------------------------------

PAD = bytes.fromhex("28BF4E5E4E758A4164004E56FFFA01082E2E00B6D0683E802F0CA9FE6453697A")


def rc4(key, data):
    s = list(range(256))
    j = 0
    for i in range(256):
        j = (j + s[i] + key[i % len(key)]) % 256
        s[i], s[j] = s[j], s[i]
    out = bytearray()
    i = j = 0
    for b in data:
        i = (i + 1) % 256
        j = (j + s[i]) % 256
        s[i], s[j] = s[j], s[i]
        out.append(b ^ s[(s[i] + s[j]) % 256])
    return bytes(out)


def pdf(objects, trailer_extra=""):
    """`objects` is {number: bytes of the object's body}; object 1 is the catalog."""
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for n in sorted(objects):
        offsets[n] = len(out)
        out += f"{n} 0 obj\n".encode() + objects[n] + b"\nendobj\n"
    xref = len(out)
    size = max(objects) + 1
    out += f"xref\n0 {size}\n0000000000 65535 f \n".encode()
    for n in range(1, size):
        out += f"{offsets[n]:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {size} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


def pdf_string(s):
    raw = s.encode("cp1252")
    return b"(" + raw.replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)") + b")"


def stream(dictionary, data):
    return b"<< " + dictionary + f" /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"


def text_pdf(pages, title, mod_date, password=None):
    """A PDF with a text layer: each page is a list of lines. With `password`, RC4-40 encrypted."""
    doc_id = hashlib.md5(title.encode()).digest()
    key = None
    trailer = f"/Info 4 0 R"
    objects = {}
    page_ids = [5 + 2 * i for i in range(len(pages))]
    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = ("<< /Type /Pages /Kids [" + " ".join(f"{p} 0 R" for p in page_ids) + f"] /Count {len(pages)} >>").encode()
    objects[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    if password is not None:
        user = (password.encode() + PAD)[:32]
        owner_key = hashlib.md5((b"owner" + PAD)[:32]).digest()[:5]
        o = rc4(owner_key, user)
        p = -4
        key = hashlib.md5(user + o + struct.pack("<i", p) + doc_id).digest()[:5]
        u = rc4(key, PAD)
        objects[4] = b"<< /Producer (brain eval) >>"
        enc_id = 5 + 2 * len(pages)
        objects[enc_id] = (
            f"<< /Filter /Standard /V 1 /R 2 /O <{o.hex()}> /U <{u.hex()}> /P {p} >>"
        ).encode()
        trailer = f"/Info 4 0 R /Encrypt {enc_id} 0 R /ID [<{doc_id.hex()}> <{doc_id.hex()}>]"
    else:
        objects[4] = b"<< /Title " + pdf_string(title) + b" /ModDate (D:" + mod_date.encode() + b"000000Z) >>"
    for i, lines in enumerate(pages):
        content = b"BT /F1 11 Tf 14 TL 56 780 Td " + b" T* ".join(pdf_string(l) + b" Tj" for l in lines) + b" ET"
        page_id, content_id = page_ids[i], page_ids[i] + 1
        if key is not None:
            content = rc4(hashlib.md5(key + struct.pack("<I", content_id)[:3] + b"\0\0").digest()[:10], content)
        objects[page_id] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        ).encode()
        objects[content_id] = stream(b"", content)
    return pdf(objects, trailer)


def scanned_pdf():
    """A page that is one picture and has no text layer: what a scanner makes."""
    side = 32
    pixels = bytes((x * 7 + y * 5) % 256 for y in range(side) for x in range(side))
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>",
        4: stream(
            f"/Type /XObject /Subtype /Image /Width {side} /Height {side} /ColorSpace /DeviceGray /BitsPerComponent 8".encode(),
            pixels,
        ),
        5: stream(b"", b"q 480 0 0 480 56 200 cm /Im1 Do Q"),
    }
    return pdf(objects)


# ---- the rest ---------------------------------------------------------------------------------


def png():
    side = 16
    raw = b"".join(
        b"\0" + bytes(v for x in range(side) for v in (x * 16 % 256, row * 16 % 256, 128))
        for row in range(side)
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", side, side, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    write(FORMATS / "operator-onboarding-checklist.docx", onboarding_docx())
    write(FORMATS / "all-hands-2025-q1.pptx", pptx())
    write(FORMATS / "regional-sales-2024.xlsx", xlsx())
    write(FORMATS / "refund-exceptions.odt", odt())
    write(FORMATS / "vendor-contracts.ods", ods())
    write(FORMATS / "product-roadmap-2025.odp", odp())
    write(
        FORMATS / "regional-switchover-drill.pdf",
        text_pdf(
            [
                [
                    "Regional switchover drill, 9 October 2024",
                    "",
                    "Summary",
                    "On 9 October 2024 we moved all booking traffic from the Keflavik site to the",
                    "Reykjavik site in 47 seconds. No booking was lost and no guest saw an error page.",
                    "The target for the drill was under 60 seconds.",
                ],
                [
                    "Findings",
                    "The payment webhooks needed 12 extra seconds to drain before the move.",
                    "The status page was updated by hand and should be automated before the next drill.",
                    "The next drill is planned for April 2025.",
                ],
            ],
            "Regional switchover drill",
            "20241010",
        ),
    )
    # What cannot be converted: a scan, a picture, a password-protected PDF, and two corrupt files.
    write(FORMATS / "scanned-contract.pdf", scanned_pdf())
    write(FORMATS / "office-photo.png", png())
    write(
        FORMATS / "encrypted-payroll.pdf",
        text_pdf([["Payroll 2024", "Net pay per employee"]], "Payroll", "20241231", password="secret"),
    )
    write(FORMATS / "corrupt-report.docx", b"PK\x03\x04" + bytes((i * 37 + 11) % 256 for i in range(900)))
    write(FORMATS / "encrypted-offer-letter.docx", bytes.fromhex("D0CF11E0A1B11AE1") + bytes(2048))
    # Three versions of one document: the newest is the one that should be found.
    write(DUPLICATES / "partner-agreement_v1.docx", agreement_docx(8, 30, "2024-01-15T10:00:00Z"))
    write(DUPLICATES / "partner-agreement_v2.docx", agreement_docx(10, 60, "2024-06-03T10:00:00Z"))
    write(DUPLICATES / "partner-agreement_final_final.docx", agreement_docx(12, 90, "2024-09-20T10:00:00Z"))


if __name__ == "__main__":
    main()
