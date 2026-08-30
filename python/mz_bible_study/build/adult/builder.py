"""Adult audience builder — placeholder (not implemented yet)."""
from __future__ import annotations

from pathlib import Path

from mz_bible_study.build.base import AdultBuilderNotImplemented


class AdultBuilder:
    """Placeholder for the adult study PPTX pipeline.

    Templates and section layouts differ from youth. Fill rules will be added
    after ``template/adult/master-template.pptx`` is provided and mapped.
    """

    audience = "adult"

    def build(
        self,
        study_path: Path,
        output: Path,
        *,
        base: Path,
        export_pdf: bool = False,
    ) -> Path:
        raise AdultBuilderNotImplemented(
            "Adult report builder is not implemented yet. "
            "Use --audience youth (default), or wait until the adult template "
            f"and fill rules are wired. (study={study_path.name}, "
            f"template={base}, output={output.name}, export_pdf={export_pdf})"
        )
