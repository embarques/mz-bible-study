#!/usr/bin/env python3
"""Export a completed Bible-study PPTX to PDF via Microsoft PowerPoint (macOS).

Writes: same basename next to the .pptx, e.g.
  bible-studies/17 - UN ABRAZO QUE DA VIDA.pptx
  → bible-studies/17 - UN ABRAZO QUE DA VIDA.pdf

Usage:
  python3 scripts/export_pdf.py "bible-studies/17 - UN ABRAZO QUE DA VIDA.pptx"
  python3 scripts/export_pdf.py bible-studies/*.pptx
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def export_one(pptx: Path) -> Path:
    pptx = pptx.resolve()
    if not pptx.exists():
        raise FileNotFoundError(pptx)
    if pptx.suffix.lower() != ".pptx":
        raise ValueError(f"not a pptx: {pptx}")
    if pptx.name.startswith("~$"):
        raise ValueError(f"skip PowerPoint lock file: {pptx.name}")

    pdf = pptx.with_suffix(".pdf")
    # AppleScript needs POSIX paths; escape for string literal
    src = str(pptx).replace("\\", "\\\\").replace('"', '\\"')
    dst = str(pdf).replace("\\", "\\\\").replace('"', '\\"')

    script = f'''
tell application "Microsoft PowerPoint"
  activate
  open POSIX file "{src}"
  set thePres to active presentation
  save thePres in POSIX file "{dst}" as save as PDF
  close thePres saving no
end tell
'''
    result = subprocess.run(
        ["osascript", "-e", script],
        capture_output=True,
        text=True,
        timeout=300,
    )
    if result.returncode != 0:
        err = (result.stderr or result.stdout or "").strip()
        raise RuntimeError(f"PowerPoint PDF export failed for {pptx.name}: {err}")
    if not pdf.exists() or pdf.stat().st_size < 1000:
        raise RuntimeError(f"PDF missing or too small: {pdf}")
    return pdf


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__.strip())
        return 2
    paths = [Path(a) for a in argv[1:]]
    # expand globs that the shell already expanded; skip lock files
    ok = 0
    for p in paths:
        if p.name.startswith("~$"):
            print(f"SKIP: {p.name}")
            continue
        try:
            out = export_one(p)
            print(f"OK: {out} ({out.stat().st_size} bytes)")
            ok += 1
        except Exception as e:
            print(f"FAIL: {p}: {e}")
            return 1
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
