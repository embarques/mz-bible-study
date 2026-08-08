#!/usr/bin/env python3
"""Backward-compatible wrapper. Prefer: `mzbs export-pdf …` """
import sys
from pathlib import Path

from mz_bible_study.export_pdf import export_one

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 scripts/export_pdf.py <file.pptx>…")
        print("Prefer: mzbs export-pdf <file.pptx>…")
        raise SystemExit(2)
    for arg in sys.argv[1:]:
        p = Path(arg)
        if p.name.startswith("~$"):
            print(f"SKIP: {p.name}")
            continue
        export_one(p)
