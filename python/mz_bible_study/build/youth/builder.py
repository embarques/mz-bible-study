"""Youth audience builder — current working Monte de Sion youth deck pipeline."""
from __future__ import annotations

import json
from pathlib import Path

from mz_bible_study.audience import DEFAULT_AUDIENCE
from mz_bible_study.paths import generated_dir


class YouthBuilder:
    """Build a youth-study PPTX from JSON + section images (existing OOXML logic)."""

    audience = DEFAULT_AUDIENCE

    def build(
        self,
        study_path: Path,
        output: Path,
        *,
        base: Path,
        export_pdf: bool = False,
    ) -> Path:
        # Import locally to keep fill logic in study.py (unchanged behavior).
        from mz_bible_study.build import study as youth_fill
        from mz_bible_study.export_pdf import export_one
        from mz_bible_study.validate import validate_pptx

        study_path = study_path.resolve()
        output = output.resolve()
        base = base.resolve()
        study = json.loads(study_path.read_text(encoding="utf-8"))
        work = generated_dir() / f"build-{study.get('numero', 'x')}"
        youth_fill.unzip_pptx(base, work)
        youth_fill.fix_package_ns0(work)
        order = youth_fill.apply_study(work, study)
        youth_fill.set_active_order(work, order)
        output.parent.mkdir(parents=True, exist_ok=True)
        youth_fill.zip_pptx(work, output)
        print(f"Wrote {output}")

        if validate_pptx(output) != 0:
            youth_fill.die("validation failed — not exporting PDF")
        if export_pdf:
            export_one(output)
        return output
