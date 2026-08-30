"""Validate a Bible-study PPTX package before delivery.

Exit 0 = OK. Exit 1 = corruption risk (do not deliver).
"""
from __future__ import annotations

import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

P = "{http://schemas.openxmlformats.org/presentationml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
ST = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"


def fail(msg: str) -> None:
    print(f"FAIL: {msg}")


def validate_pptx(path: Path) -> int:
    """Return 0 if OK, 1 if invalid."""
    errors: list[str] = []
    path = Path(path)
    if not path.exists():
        print(f"FAIL: file not found: {path}")
        return 1

    try:
        z = zipfile.ZipFile(path)
    except zipfile.BadZipFile as e:
        print(f"FAIL: not a valid zip/pptx: {e}")
        return 1

    bad = z.testzip()
    if bad:
        errors.append(f"zip CRC error in {bad}")

    names = set(z.namelist())

    if "_rels/.rels" not in names:
        errors.append(
            "missing `_rels/.rels` (zip filter must not skip `.rels` via startswith('.'))"
        )

    ct = z.read("[Content_Types].xml").decode("utf-8", "replace")
    if "ns0:" in ct or "xmlns:ns0=" in ct:
        errors.append("[Content_Types].xml has ns0: prefix (ElementTree corruption)")
    if "<Types xmlns=" not in ct and "<Types xmlns='" not in ct:
        errors.append("[Content_Types].xml missing default xmlns Types")

    overrides = re.findall(r'PartName="([^"]+)"', ct)
    slide_overrides = [
        o for o in overrides if re.fullmatch(r"/ppt/slides/slide\d+\.xml", o)
    ]
    for o in slide_overrides:
        part = o.lstrip("/")
        if part not in names:
            errors.append(f"Content_Types Override missing file: {o}")

    slide_files = sorted(
        n for n in names if re.fullmatch(r"ppt/slides/slide\d+\.xml", n)
    )
    for sf in slide_files:
        part = "/" + sf
        if part not in slide_overrides:
            errors.append(f"slide file has no Content_Types Override: {sf}")

    rels_xml = z.read("ppt/_rels/presentation.xml.rels").decode("utf-8", "replace")
    if "ns0:Types" in rels_xml:
        errors.append("presentation.xml.rels looks corrupted (ns0:Types)")
    if re.search(
        r'xmlns:ns0="http://schemas.openxmlformats.org/package/2006/content-types"',
        rels_xml,
    ):
        errors.append("presentation.xml.rels has content-types namespace (wrong file?)")

    rels = ET.fromstring(z.read("ppt/_rels/presentation.xml.rels"))
    rid_map = {r.get("Id"): r for r in rels}
    slide_rels = [r for r in rels if r.get("Type") == ST]
    for r in slide_rels:
        t = r.get("Target") or ""
        full = "ppt/" + t if not t.startswith("ppt/") else t
        if full not in names:
            errors.append(f"slide Relationship target missing: {t}")

    pres = ET.fromstring(z.read("ppt/presentation.xml"))
    sld_lst_el = pres.find(f"{P}sldIdLst")
    sld_lst = list(sld_lst_el) if sld_lst_el is not None else []
    if len(sld_lst) != len(slide_rels):
        errors.append(
            f"sldIdLst count ({len(sld_lst)}) != slide rels ({len(slide_rels)})"
        )

    for s in sld_lst:
        rid = s.get(f"{R}id")
        rel = rid_map.get(rid)
        if rel is None:
            errors.append(f"sldId r:id not in rels: {rid}")
            continue
        t = rel.get("Target") or ""
        full = "ppt/" + t if not t.startswith("ppt/") else t
        if full not in names:
            errors.append(f"sldIdLst points to missing slide: {t}")

    for name in names:
        m = re.fullmatch(r"ppt/slides/_rels/slide(\d+)\.xml\.rels", name)
        if not m:
            continue
        sn = m.group(1)
        txt = z.read(name).decode("utf-8", "replace")
        for nm in re.findall(r"notesSlide(\d+)", txt):
            if nm != sn:
                errors.append(
                    f"slide{sn}.rels points to notesSlide{nm} (must match or remove notes rel)"
                )
            notes = f"ppt/notesSlides/notesSlide{nm}.xml"
            if notes not in names:
                errors.append(f"slide{sn}.rels notes target missing: {notes}")

    for name in names:
        if not (name.endswith(".xml") or name.endswith(".rels")):
            continue
        try:
            ET.fromstring(z.read(name))
        except ET.ParseError as e:
            errors.append(f"XML parse error {name}: {e}")

    if errors:
        print(f"INVALID: {path}")
        for e in errors:
            fail(e)
        print(f"{len(errors)} error(s). Do NOT deliver this pptx.")
        return 1

    print(f"OK: {path}")
    print(f"  slides in order: {len(sld_lst)}")
    print(f"  slide files in zip: {len(slide_files)}")
    print(f"  Content_Types slide overrides: {len(slide_overrides)}")
    return 0
