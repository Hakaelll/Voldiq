"""Independently inspect the owned PDF retained by the storage-history behavior flow.

Requires developer-only pypdf and pdfplumber; never runs from the application or routine gate.
Render the same PDF with Poppler and review its pages separately.
"""

import argparse
import hashlib
import json
import re
from datetime import datetime, timezone
from pathlib import Path

import pdfplumber
from pypdf import PdfReader


def inspect(path: Path) -> dict:
    reader = PdfReader(path, strict=True)
    assert not reader.is_encrypted
    assert reader.metadata.title == "Voldiq storage report"
    root = reader.trailer["/Root"]
    assert not any(root.get(key) for key in ("/OpenAction", "/AA", "/AcroForm"))
    names = root.get("/Names", {})
    if names:
        names = names.get_object()
        assert not any(names.get(key) for key in ("/JavaScript", "/EmbeddedFiles"))
    text = "\n".join(page.extract_text() for page in reader.pages)
    assert len(reader.pages) > 1
    assert "10005 matching files" in text
    assert "9955 omitted" in text
    assert "36893488147419113233 bytes" in text
    assert "Coverage: unknown" in text
    assert "Allocation unknown or partial: 10005" in text
    assert "Modified date unknown: 1" in text
    assert "Paths included: false. Filter text included: false" in text
    assert "Espa\u00f1ol" in text and "\u0627\u0644\u0639\u0631\u0628\u064a\u0629" in text
    assert "\\u{000A}\\u{202E}" in text
    body_text = re.sub(r"VOLDIQ / Storage report / Page \d+ of \d+", "", text)
    long_name = body_text.split("Name: LONG-NAME-", 1)[1].split("Node 10001.", 1)[0]
    assert re.sub(r"\s", "", long_name) == "abcdef" * 900 + "-END"
    assert "Name: record-48" in text and "Limitations" in text
    assert not re.search(r"[A-Z]:\\|private\.invalid", text)
    fonts = set()
    for number, page in enumerate(reader.pages, start=1):
        assert not page.get("/Annots") and not page.get("/AA")
        assert f"Page {number} of {len(reader.pages)}" in page.extract_text()
        assert abs(float(page.mediabox.width) - 595.28) < 0.01
        assert abs(float(page.mediabox.height) - 841.89) < 0.01
        resources = page["/Resources"]
        assert not resources.get("/XObject"), "The text report must not be a screenshot"
        for font_ref in resources["/Font"].values():
            font = font_ref.get_object()
            assert font.get("/ToUnicode")
            for descendant in font["/DescendantFonts"]:
                descriptor = descendant.get_object()["/FontDescriptor"]
                assert descriptor.get("/FontFile2") or descriptor.get("/FontFile3")
            fonts.add(str(font["/BaseFont"]))
    bounds = []
    with pdfplumber.open(path) as document:
        for number, page in enumerate(document.pages, start=1):
            chars = page.chars
            coordinates = (
                min(char["x0"] for char in chars),
                max(char["x1"] for char in chars),
                min(char["top"] for char in chars),
                max(char["bottom"] for char in chars),
            )
            assert coordinates[0] >= 41 and coordinates[1] <= page.width - 41
            assert coordinates[2] >= 41 and coordinates[3] <= page.height - 24
            bounds.append({"page": number, "characters": len(chars), "bounds": [round(value, 3) for value in coordinates]})
    return {
        "checkedUtc": datetime.now(timezone.utc).isoformat(),
        "scope": "Owned synthetic ranked PDF; independent structure/text/font/bounds inspection. Rendered pages and native interaction require separate review.",
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest().upper(),
        "bytes": path.stat().st_size,
        "pages": len(reader.pages),
        "matching": 10005,
        "shown": 50,
        "omitted": 9955,
        "embeddedFonts": sorted(fonts),
        "pageBounds": bounds,
        "checks": {"exactLargeTotal": True, "longNameComplete": True, "latinArabicText": True, "visibleControls": True, "pathsWithheld": True, "noActiveContent": True, "noScreenshot": True},
    }


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run the fixture inspector without Python optimization (-O).")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pdf", type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.pdf), indent=2, ensure_ascii=True))
