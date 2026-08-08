"""Export a completed Bible-study PPTX to PDF via Microsoft PowerPoint (macOS)."""
from __future__ import annotations

import subprocess
from pathlib import Path


def export_one(pptx: Path) -> Path:
    """Export one pptx to a sibling PDF. Returns the PDF path."""
    pptx = pptx.resolve()
    if not pptx.exists():
        raise FileNotFoundError(pptx)
    if pptx.suffix.lower() != ".pptx":
        raise ValueError(f"not a pptx: {pptx}")
    if pptx.name.startswith("~$"):
        raise ValueError(f"skip PowerPoint lock file: {pptx.name}")

    pdf = pptx.with_suffix(".pdf")
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
    print(f"OK: {pdf} ({pdf.stat().st_size} bytes)")
    return pdf
