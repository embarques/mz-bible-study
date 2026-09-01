#!/usr/bin/env python3
"""Create `lamad-cli/template/adult/master-template.pptx` from the gold deck.

Copies the reference study PPTX (layout/colours/fonts) and repairs stale
`notesSlide` relationships so `lamad validate` passes.
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
DEFAULT_GOLD = REPO / "lamad-cli/template/adult/24 EL VALOR DE LA MODESTIA.pptx"
DEFAULT_OUT = REPO / "lamad-cli/template/adult/master-template.pptx"

NOTES_RE = re.compile(r'<Relationship\b[^>]*Type="[^"]*notesSlide"[^>]*/>')
TARGET_RE = re.compile(r"notesSlide(\d+)")


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


def zip_dir(src: Path, dest: Path) -> None:
    with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as zout:
        for root, _dirs, files in os.walk(src):
            for fn in files:
                if fn.startswith(".DS_Store") or fn.startswith("._") or fn.startswith("~$"):
                    continue
                fp = Path(root) / fn
                zout.write(fp, fp.relative_to(src).as_posix())


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--gold", type=Path, default=DEFAULT_GOLD)
    ap.add_argument("-o", "--output", type=Path, default=DEFAULT_OUT)
    args = ap.parse_args()
    if not args.gold.is_file():
        raise SystemExit(f"gold deck not found: {args.gold}")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as td:
        work = Path(td)
        with zipfile.ZipFile(args.gold) as zin:
            zin.extractall(work)
        n = fix_notes_rels(work)
        tmp = args.output.with_suffix(".tmp.pptx")
        zip_dir(work, tmp)
        tmp.replace(args.output)
    print(f"Wrote {args.output} (fixed {n} notesSlide rels)")


if __name__ == "__main__":
    main()
