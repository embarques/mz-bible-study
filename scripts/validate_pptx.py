#!/usr/bin/env python3
"""Backward-compatible wrapper. Prefer: `mzbs validate …` """
import sys
from pathlib import Path

from mz_bible_study.validate import validate_pptx

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python3 scripts/validate_pptx.py <file.pptx>")
        print("Prefer: mzbs validate <file.pptx>")
        raise SystemExit(2)
    raise SystemExit(validate_pptx(Path(sys.argv[1])))
