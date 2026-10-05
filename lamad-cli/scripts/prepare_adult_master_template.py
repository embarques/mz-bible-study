#!/usr/bin/env python3
"""Create adult `master-template.pptx` from a finished gold study deck.

The builder expects a **62-slide prototype** package (`slide1.xml` … `slide62.xml`
in `sldIdLst`). A finished study (e.g. Estudio 4 after manual QA) has dynamic
slide allocation and sequential ordering — this script maps corrected layouts
from the gold deck back onto prototype slide numbers, keeps the skeleton package
(diagrams, SmartArt, Content_Types for protos), merges media, and repairs
`notesSlide` relationships so `lamad validate` passes.

Visual authority: `bible-studies/4 - DIOS BUSCA DISCÍPULOS CONFORME A SU CORAZÓN.pptx`
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import tempfile
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEFAULT_GOLD = REPO / "bible-studies/4 - DIOS BUSCA DISCÍPULOS CONFORME A SU CORAZÓN.pptx"
DEFAULT_SKELETON = REPO / "lamad-cli/template/adult/master-template.pptx"
DEFAULT_OUT = REPO / "template/adult/master-template.pptx"
DEFAULT_MIRROR = REPO / "lamad-cli/template/adult/master-template.pptx"

# Map prototype slide number → source slide number in the finished gold deck.
# See `lamad-cli/src/build/adult/SLIDE_MAP.md`.
PROTO_FROM_GOLD: dict[int, int] = {
    1: 1,
    2: 2,
    3: 3,
    4: 4,
    5: 5,
    6: 10,
    7: 11,
    8: 12,
    9: 13,
    10: 14,
    11: 15,
    12: 16,
    13: 17,
    14: 19,
    15: 20,
    16: 21,
    17: 22,
    18: 28,
    19: 29,
    20: 30,
    21: 31,
    22: 34,
    23: 35,
    24: 37,
    25: 38,
    26: 39,
    27: 40,
    28: 41,
    29: 43,
    30: 20,  # definición proto — tema II skipped definición in study 4
    31: 44,
    32: 45,
    33: 51,
    34: 52,
    35: 53,
    36: 54,
    37: 57,
    38: 58,
    39: 67,
    40: 68,
    41: 69,
    42: 70,
    43: 71,
    44: 74,
    45: 75,
    46: 76,
    47: 77,
    48: 84,
    49: 85,
    50: 86,
    51: 87,
    52: 88,
    53: 90,
    54: 91,
    55: 94,
    56: 95,
    57: 96,
    58: 97,
    59: 98,
    60: 94,
    61: 95,
    62: 99,
}

NOTES_RE = re.compile(r'<Relationship\b[^>]*Type="[^"]*notesSlide"[^>]*/>')
TARGET_RE = re.compile(r"notesSlide(\d+)")
MEDIA_TARGET_RE = re.compile(r'Target="../media/([^"]+)"')


def fix_notes_rels(work: Path) -> int:
    rels_dir = work / "ppt/slides/_rels"
    fixed = 0
    for rel in rels_dir.glob("slide*.xml.rels"):
        sn = rel.stem.replace("slide", "").replace(".xml", "")
        txt = rel.read_text(encoding="utf-8")
        m = TARGET_RE.search(txt)
        if m and m.group(1) != sn:
            rel.write_text(NOTES_RE.sub("", txt), encoding="utf-8")
            fixed += 1
    return fixed


def copy_slide_from_gold(work: Path, gold: Path, proto: int, src: int) -> None:
    slides = work / "ppt/slides"
    rels = slides / "_rels"
    src_xml = gold / "ppt/slides" / f"slide{src}.xml"
    dst_xml = slides / f"slide{proto}.xml"
    src_rels = gold / "ppt/slides/_rels" / f"slide{src}.xml.rels"
    dst_rels = rels / f"slide{proto}.xml.rels"
    if not src_xml.is_file():
        raise SystemExit(f"gold missing slide{src}.xml (proto {proto})")
    shutil.copy2(src_xml, dst_xml)
    if src_rels.is_file():
        shutil.copy2(src_rels, dst_rels)
    elif dst_rels.is_file():
        dst_rels.unlink()


def merge_media_from_gold(work: Path, gold: Path) -> None:
    """Copy any media referenced by remapped slide rels."""
    rels_dir = work / "ppt/slides/_rels"
    gold_media = gold / "ppt/media"
    work_media = work / "ppt/media"
    work_media.mkdir(parents=True, exist_ok=True)
    needed: set[str] = set()
    for rel in rels_dir.glob("slide*.xml.rels"):
        txt = rel.read_text(encoding="utf-8")
        needed.update(MEDIA_TARGET_RE.findall(txt))
    for name in needed:
        src = gold_media / name
        dst = work_media / name
        if src.is_file() and not dst.is_file():
            shutil.copy2(src, dst)


def reset_sldid_list(work: Path, count: int = 62) -> None:
    pres_path = work / "ppt/presentation.xml"
    pres = pres_path.read_text(encoding="utf-8")
    items = "".join(
        f'<p:sldId id="{256 + i}" r:id="rId{900 + i}"/>' for i in range(count)
    )
    pres = re.sub(
        r"(?s)<p:sldIdLst>.*?</p:sldIdLst>",
        f"<p:sldIdLst>{items}</p:sldIdLst>",
        pres,
        count=1,
    )
    pres_path.write_text(pres, encoding="utf-8")

    rels_path = work / "ppt/_rels/presentation.xml.rels"
    rels = rels_path.read_text(encoding="utf-8")
    rels = re.sub(
        r'<(?:[\w]+:)?Relationship\b[^>]*Type="[^"]*/relationships/slide"[^>]*/>\s*',
        "",
        rels,
    )
    chunks = "".join(
        f'<Relationship Id="rId{900 + i}" '
        f'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" '
        f'Target="slides/slide{i + 1}.xml"/>'
        for i in range(count)
    )
    rels = rels.replace("</Relationships>", f"{chunks}</Relationships>")
    rels_path.write_text(rels, encoding="utf-8")

    app_path = work / "docProps/app.xml"
    if app_path.is_file():
        app = app_path.read_text(encoding="utf-8")
        app = re.sub(r"<Slides>\d+</Slides>", f"<Slides>{count}</Slides>", app)
        app_path.write_text(app, encoding="utf-8")


def zip_dir(src: Path, dest: Path) -> None:
    with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as zout:
        for root, _dirs, files in os.walk(src):
            for fn in files:
                if fn.startswith(".DS_Store") or fn.startswith("._") or fn.startswith("~$"):
                    continue
                fp = Path(root) / fn
                zout.write(fp, fp.relative_to(src).as_posix())


def write_template(work: Path, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    tmp = output.with_suffix(".tmp.pptx")
    zip_dir(work, tmp)
    tmp.replace(output)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--gold", type=Path, default=DEFAULT_GOLD, help="Finished QA gold deck")
    ap.add_argument(
        "--skeleton",
        type=Path,
        default=DEFAULT_SKELETON,
        help="Existing 62-slide prototype package (diagrams / Content_Types)",
    )
    ap.add_argument("-o", "--output", type=Path, default=DEFAULT_OUT)
    ap.add_argument(
        "--mirror",
        type=Path,
        default=DEFAULT_MIRROR,
        help="Secondary copy (lamad-cli fallback path)",
    )
    args = ap.parse_args()

    if not args.gold.is_file():
        raise SystemExit(f"gold deck not found: {args.gold}")
    if not args.skeleton.is_file():
        raise SystemExit(f"skeleton template not found: {args.skeleton}")

    with tempfile.TemporaryDirectory() as td:
        work = Path(td) / "work"
        gold = Path(td) / "gold"
        with zipfile.ZipFile(args.skeleton) as zin:
            zin.extractall(work)
        with zipfile.ZipFile(args.gold) as zin:
            zin.extractall(gold)

        for proto, src in PROTO_FROM_GOLD.items():
            copy_slide_from_gold(work, gold, proto, src)

        merge_media_from_gold(work, gold)
        reset_sldid_list(work, len(PROTO_FROM_GOLD))
        n = fix_notes_rels(work)

        write_template(work, args.output)
        if args.mirror.resolve() != args.output.resolve():
            shutil.copy2(args.output, args.mirror)

    print(f"Wrote {args.output}")
    if args.mirror.is_file():
        print(f"Mirrored {args.mirror}")
    print(f"Remapped {len(PROTO_FROM_GOLD)} prototype slides from {args.gold.name}")
    print(f"Fixed {n} notesSlide rels")


if __name__ == "__main__":
    main()
